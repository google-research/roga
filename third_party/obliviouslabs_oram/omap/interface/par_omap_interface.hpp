#pragma once
#include <stdint.h>

typedef void* par_omap_t;
typedef void* initializer_t;
#ifndef __BINDGEN__
#include <functional>
#include <string_view>

#include "common/cmp_intrinsics.hpp"
#endif

struct Key16 {
  uint8_t bytes[16];
#ifndef __BINDGEN__
  bool operator<(const Key16& other) const {
    return obliCheckLess<16>(bytes, other.bytes);
  }
  bool operator==(const Key16& other) const {
    return obliCheckEqual<16>(bytes, other.bytes);
  }
#endif
};

#ifndef __BINDGEN__
namespace std {
template <>
struct hash<Key16> {
  std::size_t operator()(const Key16& k) const {
    return std::hash<std::string_view>()(
        std::string_view((const char*)k.bytes, 16));
  }
};
}  // namespace std
#endif

typedef Key16 ParOMapK;
typedef uint64_t ParOMapV;

/**
 * @brief Binds to Parallel OMap implementation.
 *
 */
struct ParOMapBinding {
  par_omap_t omap;            // Pointer to the ParOMap instance.
  initializer_t initializer;  // Pointer to the ParOMap initializer.

  ParOMapBinding();

  /**
   * @brief Initializes an empty parallel OMap, assuming sufficiently large
   * enclave size.
   *
   * @param size Maximum number of key-value pairs the map can hold.
   * @param numCores Number of available cores.
   */
  void InitEmpty(uint32_t size, uint32_t numCores);

  /**
   * @brief Initializes an empty parallel OMap. Uses at most cacheBytes bytes of
   * EPC memory.
   *
   * @param size Maximum number of key-value pairs the map can hold.
   * @param numCores Number of available cores.
   * @param cacheBytes Maximum number of bytes to use for caching.
   */
  void InitEmptyExternal(uint32_t size, uint32_t numCores, uint64_t cacheBytes);

  /**
   * @brief Inserts a batch of key-value pairs into the map. The operation is
   * oblivious. The method can be called either at the initialization stage, or
   * after initilization. The insertion during initialization is more efficient.
   * During initialization, the keys must be unique (throughout the
   * initialization), and the batch size doesn't affect the efficiency per
   * key-value pair.
   * Otherwise, the batch may be arbitrarily ordered and may contain duplicate
   * keys. If the batch consists of duplicate keys, an arbitrary one of these
   * key-value pairs is inserted. Larger batch size helps increase the
   * throughput.
   *
   * @param batchSize Number of key-value pairs to insert.
   * @param keys Array of keys to insert.
   * @param vals Array of values to insert.
   * @param existFlags Outputs array of flags indicating whether each key
   * already exists
   */
  void InsertBatch(uint32_t batchSize, const ParOMapK* keys,
                   const ParOMapV* vals, bool* existFlags);

  /**
   * @brief Finds the values of a batch of keys. The operation is oblivious.
   *
   * @param batchSize Number of keys to look up.
   * @param keys Array of keys to look up.
   * @param vals Output array of values found.
   * @param existFlags Outputs array of flags indicating whether each key
   * exists.
   */
  void FindBatch(uint32_t batchSize, const ParOMapK* keys, ParOMapV* vals,
                 bool* existFlags);

  /**
   * @brief Finds the values of a batch of keys without writing back the blocks
   * to ORAM immediately. The maintenance is deferred until FindBatchMaintain is
   * called. This method is oblivious.
   *
   * @param batchSize Number of keys to look up.
   * @param keys Array of keys to look up.
   * @param vals Output array of values found.
   * @param existFlags Outputs array of flags indicating whether each key
   * exists.
   */
  void FindBatchDeferMaintain(uint32_t batchSize, const ParOMapK* keys,
                              ParOMapV* vals, bool* existFlags);

  /**
   * @brief Performs write back of all deferred FindBatch accesses.
   *
   */
  void FindBatchMaintain();

  /**
   * @brief Erases a batch of key-value pairs from the map. The operation is
   * oblivious.
   *
   * @param batchSize Number of keys to erase.
   * @param keys Array of keys to erase.
   * @param existFlags Outputs array of flags indicating whether each key
   * exists.
   */
  void EraseBatch(uint32_t batchSize, const ParOMapK* keys, bool* existFlags);

  /**
   * @brief Start initializing a non-empty map, assuming sufficiently large EPC.
   * The method cannot be called together with InitEmpty or InitEmptyExternal.
   * Only InsertBatch can be called during initialization (i.e. until FinishInit
   * is called).
   *
   * @param size Maximum number of key-value pairs the map can hold.
   * @param initSize Number of key-value pairs to insert.
   * @param numCores Number of available cores.
   */
  void StartInit(uint32_t size, uint32_t initSize, uint32_t numCores);

  /**
   * @brief Start initializing a non-empty map that uses at most cacheBytes
   * bytes of EPC memory. The method cannot be called together with InitEmpty or
   * InitEmptyExternal. Only InsertBatch can be called during initialization
   * (i.e. untilFinishInit is called).
   *
   * @param size Maximum number of key-value pairs the map can hold.
   * @param initSize Number of key-value pairs to insert.
   * @param numCores Number of available cores.
   * @param cacheBytes Maximum number of bytes to use for caching.
   */
  void StartInitExternal(uint32_t size, uint32_t initSize, uint32_t numCores,
                         uint64_t cacheBytes);

  /**
   * @brief Finish the initialization.
   *
   */
  void FinishInit();
};