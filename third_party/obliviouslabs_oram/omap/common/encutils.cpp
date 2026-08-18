#include <cstdlib>
#include <iostream>
#include <stdexcept>
#include <utility>
#ifdef ENCLAVE_MODE_ENCLAVE
// #include "../Enclave.h"
#include "Enclave_t.h"
#endif

#include <x86intrin.h>

#include "common/encutils.hpp"
#define memset_s(s, smax, c, n) memset(s, c, n);

RandGen default_rand;
// random AES key:
uint8_t KEY[AES_BLOCK_SIZE] = {0};
EVP_CIPHER_CTX* aes_ctx = nullptr;

// We have two versions of bearssl. For the enclave we need the sgx prepacked
// one, for non enclave we need one installed in the OS. This file allows us to
// use SGX library functions instead of bearssl inside of the enclave whenever
// they are available.

#ifndef ENCLAVE_MODE_ENCLAVE
void handleErrors(void) {
#if __cpp_exceptions || defined(__EXCEPTIONS)
  throw std::runtime_error("BearSSL error");
#else
  std::cerr << "BearSSL error" << std::endl;
  std::abort();
#endif
}
#endif

void __attribute__((noinline)) sgxsd_br_clear_stack() {
  uint8_t stack[4096];
  memset_s(&stack, sizeof(stack), 0, sizeof(stack));
  _mm256_zeroall();
}

#ifndef ENCLAVE_MODE_ENCLAVE
uint64_t secure_hash_with_salt(const uint8_t* data, size_t data_size,
                               const uint8_t (&salt)[16]) {
  uint64_t res;
  SHA256_CTX ctx;
  SHA256_Init(&ctx);
  SHA256_Update(&ctx, salt, sizeof(salt));
  SHA256_Update(&ctx, data, data_size);
  unsigned char hash[SHA256_DIGEST_LENGTH];
  SHA256_Final(hash, &ctx);
  memcpy(&res, hash, 8);
  return res;
}
#else
#include <cstring>

#include "sgx_tcrypto.h"
#include "sgx_trts.h"
uint64_t secure_hash_with_salt(const uint8_t* data, size_t data_size,
                               const uint8_t (&salt)[16]) {
  sgx_sha_state_handle_t sha_handle;
  sgx_sha256_hash_t hash;
  uint64_t result = 0;

  sgx_status_t status = sgx_sha256_init(&sha_handle);
  if (status != SGX_SUCCESS) {
    // Handle error
    return 0;
  }

  // Hash the salt
  status = sgx_sha256_update(salt, sizeof(salt), sha_handle);
  if (status != SGX_SUCCESS) {
    // Handle error
    sgx_sha256_close(sha_handle);
    return 0;
  }

  // Hash the data
  status = sgx_sha256_update(data, data_size, sha_handle);
  if (status != SGX_SUCCESS) {
    // Handle error
    sgx_sha256_close(sha_handle);
    return 0;
  }

  // Finalize the hash
  status = sgx_sha256_get_hash(sha_handle, &hash);
  sgx_sha256_close(sha_handle);
  if (status != SGX_SUCCESS) {
    // Handle error
    return 0;
  }

  // Use the first 8 bytes of the hash as the result
  memcpy(&result, hash, sizeof(result));

  return result;
}
#endif

bool sgxsd_aes_gcm_run(bool encrypt, const void* p_src, uint32_t src_len,
                       void* p_dst, const uint8_t p_iv[SGXSD_AES_GCM_IV_SIZE],
                       const void* p_aad, uint32_t aad_len,
                       uint8_t p_mac[SGXSD_AES_GCM_MAC_SIZE],
                       aex_ctx_t* aes_ctx_ptr) {
  if (((p_src == NULL || p_dst == NULL) && src_len != 0) || p_iv == NULL ||
      (p_aad == NULL && aad_len != 0) || p_mac == NULL) {
    return false;
  }

  EVP_CIPHER_CTX* ctx = EVP_CIPHER_CTX_new();
  if (ctx == NULL) return false;

  bool success = false;
  int len = 0;

  if (encrypt) {
    if (EVP_EncryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL) != 1)
      goto out;
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, SGXSD_AES_GCM_IV_SIZE,
                            NULL) != 1)
      goto out;
    if (EVP_EncryptInit_ex(ctx, NULL, NULL, KEY, p_iv) != 1) goto out;

    if (aad_len != 0) {
      if (EVP_EncryptUpdate(ctx, NULL, &len, (const uint8_t*)p_aad, aad_len) !=
          1)
        goto out;
    }
    if (src_len != 0) {
      if (EVP_EncryptUpdate(ctx, (uint8_t*)p_dst, &len, (const uint8_t*)p_src,
                            src_len) != 1)
        goto out;
    }
    if (EVP_EncryptFinal_ex(ctx, (uint8_t*)p_dst + len, &len) != 1) goto out;
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_GET_TAG, SGXSD_AES_GCM_MAC_SIZE,
                            p_mac) != 1)
      goto out;
    success = true;
  } else {
    if (EVP_DecryptInit_ex(ctx, EVP_aes_256_gcm(), NULL, NULL, NULL) != 1)
      goto out;
    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_IVLEN, SGXSD_AES_GCM_IV_SIZE,
                            NULL) != 1)
      goto out;
    if (EVP_DecryptInit_ex(ctx, NULL, NULL, KEY, p_iv) != 1) goto out;

    if (EVP_CIPHER_CTX_ctrl(ctx, EVP_CTRL_GCM_SET_TAG, SGXSD_AES_GCM_MAC_SIZE,
                            p_mac) != 1)
      goto out;

    if (aad_len != 0) {
      if (EVP_DecryptUpdate(ctx, NULL, &len, (const uint8_t*)p_aad, aad_len) !=
          1)
        goto out;
    }
    if (src_len != 0) {
      if (EVP_DecryptUpdate(ctx, (uint8_t*)p_dst, &len, (const uint8_t*)p_src,
                            src_len) != 1)
        goto out;
    }
    if (EVP_DecryptFinal_ex(ctx, (uint8_t*)p_dst + len, &len) > 0) {
      success = true;
    }
  }

out:
  EVP_CIPHER_CTX_free(ctx);
  if (!success && !encrypt && p_dst != NULL && src_len != 0) {
    memset(p_dst, 0, src_len);
  }
  return success;
}

void aes_256_gcm_encrypt(uint64_t plaintextSize, uint8_t* plaintext,
                         const uint8_t iv[SGXSD_AES_GCM_IV_SIZE],
                         uint8_t tag[SGXSD_AES_GCM_MAC_SIZE],
                         uint8_t* ciphertext, aex_ctx_t* aes_ctx_ptr) {
  sgxsd_aes_gcm_run(true, plaintext, plaintextSize, ciphertext, iv, nullptr, 0,
                    tag, aes_ctx_ptr);
}

bool aes_256_gcm_decrypt(uint64_t ciphertextSize, uint8_t* ciphertext,
                         const uint8_t iv[SGXSD_AES_GCM_IV_SIZE],
                         uint8_t tag[SGXSD_AES_GCM_MAC_SIZE],
                         uint8_t* plaintext, aex_ctx_t* aes_ctx_ptr) {
  return sgxsd_aes_gcm_run(false, ciphertext, ciphertextSize, plaintext, iv,
                           nullptr, 0, tag, aes_ctx_ptr);
}

bool sgxsd_aes_ctr_run(bool encrypt, const void* p_src, uint32_t src_len,
                       void* p_dst, const uint8_t p_iv[SGXSD_AES_GCM_IV_SIZE],
                       aex_ctx_t* aes_ctx_ptr) {
  if (((p_src == NULL || p_dst == NULL) && src_len != 0) || p_iv == NULL ||
      aes_ctx_ptr == NULL) {
    return false;
  }
  if (src_len != 0) {
    uint8_t iv16[16];
    memcpy(iv16, p_iv, 12);
    memset(iv16 + 12, 0, 4);

    if (EVP_EncryptInit_ex(aes_ctx_ptr, NULL, NULL, NULL, iv16) != 1) {
      return false;
    }
    int out_len = 0;
    if (EVP_EncryptUpdate(aes_ctx_ptr, (uint8_t*)p_dst, &out_len,
                          (const uint8_t*)p_src, src_len) != 1) {
      return false;
    }
    int final_len = 0;
    if (EVP_EncryptFinal_ex(aes_ctx_ptr, (uint8_t*)p_dst + out_len,
                            &final_len) != 1) {
      return false;
    }
  }
  return true;
}

void aes_256_ctr_encrypt(uint64_t plaintextSize, uint8_t* plaintext,
                         const uint8_t iv[SGXSD_AES_GCM_IV_SIZE],
                         uint8_t* ciphertext, aex_ctx_t* aes_ctx_ptr) {
  sgxsd_aes_ctr_run(true, plaintext, plaintextSize, ciphertext, iv,
                    aes_ctx_ptr);
}

bool aes_256_ctr_decrypt(uint64_t ciphertextSize, uint8_t* ciphertext,
                         const uint8_t iv[SGXSD_AES_GCM_IV_SIZE],
                         uint8_t* plaintext, aex_ctx_t* aes_ctx_ptr) {
  return sgxsd_aes_ctr_run(false, ciphertext, ciphertextSize, plaintext, iv,
                           aes_ctx_ptr);
}

#ifndef ENCLAVE_MODE_ENCLAVE
RandGen::RandGen() { new (this) RandGen(rd()); }
RandGen::RandGen(uint64_t seed) : engine(seed) {}
uint64_t RandGen::rand64() {
  std::uniform_int_distribution<uint64_t> d;
  return d(engine);
}
uint32_t RandGen::rand32() {
  std::uniform_int_distribution<uint32_t> d;
  return d(engine);
}
uint8_t RandGen::rand1() {
  std::uniform_int_distribution<short> d(0, 1);
  return d(engine);
}

void read_rand(uint8_t* output, size_t size) {
  FILE* fp = fopen("/dev/urandom", "rb");
  if (fp == NULL) {
    perror("Failed to open /dev/urandom");
    return;  // Failure
  }

  size_t read = fread(output, 1, size, fp);
  fclose(fp);

  if (read != size) {
    perror("Failed to read enough bytes");
    // Handle the error, not enough data was read
    return;  // Failure
  }

  return;  // Success
}

#else
RandGen::RandGen() {}

uint64_t RandGen::rand64() {
  uint64_t output;
  sgx_read_rand((uint8_t*)&output, sizeof(output));
  return output;
}

uint32_t RandGen::rand32() {
  uint32_t output;
  sgx_read_rand((uint8_t*)&output, sizeof(output));
  return output;
}
uint8_t RandGen::rand1() {
  uint8_t output;
  sgx_read_rand((uint8_t*)&output, sizeof(output));
  return output & 1;
}

void read_rand(uint8_t* output, size_t size) { sgx_read_rand(output, size); }

#endif
struct GlobalRandomKeySetter {
  GlobalRandomKeySetter() {
    read_rand(KEY, AES_BLOCK_SIZE);
    aes_ctx = EVP_CIPHER_CTX_new();
    EVP_EncryptInit_ex(aes_ctx, EVP_aes_256_ctr(), NULL, KEY, NULL);
  }
  ~GlobalRandomKeySetter() {
    if (aes_ctx) {
      EVP_CIPHER_CTX_free(aes_ctx);
    }
  }
};

GlobalRandomKeySetter global_random_key_setter;