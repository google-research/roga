/* C ABI for the H2O2RAM oblivious map.
 *
 * Insert and lookup only. Erase is absent: ObliviousRAM::erase extracts an
 * element without decrementing the size, so it is not a delete.
 *
 * Keys vary up to H2O2RAM_KEY_MAX, values are exactly H2O2RAM_VAL_SIZE. Both,
 * and the capacity, are compile time parameters; query them below.
 */
#ifndef H2O2RAM_FFI_H
#define H2O2RAM_FFI_H

#include <stddef.h>
#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Return codes. */
#define H2O2RAM_OK 0
#define H2O2RAM_NOT_FOUND 1
#define H2O2RAM_FULL 2 /* at capacity and the key is new */
#define H2O2RAM_COLLISION                             \
  3 /* index taken by a different key, see note below \
     */
#define H2O2RAM_EINVAL 4
#define H2O2RAM_ENOMEM 5

typedef struct h2o2ram_map h2o2ram_map;

/* Compile time geometry. */
size_t h2o2ram_key_max(void);
size_t h2o2ram_val_size(void);

/* Max distinct keys, the requested capacity rounded up to 128 * 2^k. */
size_t h2o2ram_capacity(void);

/* NULL on failure. Expensive: builds the whole table hierarchy, and autotunes
 * first if this machine has no tuning file for the block size.
 *
 * threads caps the cores this map may use, construction included; 0 means the
 * whole machine. Covers both TBB and OpenMP. */
h2o2ram_map* h2o2ram_map_new(unsigned threads);
void h2o2ram_map_free(h2o2ram_map* map);

/* The core cap, resolved to the real count when created with threads == 0. */
unsigned h2o2ram_threads(const h2o2ram_map* map);

/* Distinct keys stored. */
size_t h2o2ram_len(const h2o2ram_map* map);

/* key_len in [1, h2o2ram_key_max()], val exactly h2o2ram_val_size() bytes.
 * Overwriting an existing key consumes no capacity.
 *
 * H2O2RAM_COLLISION: two distinct keys hit the same 63 bit index. The stored
 * entry is kept and the new one rejected. ~5e-8 at 1e6 keys. */
int h2o2ram_insert(h2o2ram_map* map, const uint8_t* key, size_t key_len,
                   const uint8_t* val);

/* Writes h2o2ram_val_size() bytes to val_out on H2O2RAM_OK, zeroes otherwise.
 * Mutable map: a lookup mutates ORAM state and is not safe to call
 * concurrently on one map. */
int h2o2ram_get(h2o2ram_map* map, const uint8_t* key, size_t key_len,
                uint8_t* val_out);

#ifdef __cplusplus
}
#endif

#endif /* H2O2RAM_FFI_H */
