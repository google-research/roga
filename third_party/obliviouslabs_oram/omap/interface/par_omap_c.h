#ifndef PAR_OMAP_C_H
#define PAR_OMAP_C_H

#include <stdint.h>

#ifdef __cplusplus
extern "C" {
#endif

typedef void* par_omap_ptr;

// Create a new ParOMap<Bytes<16>, Bytes<4>> with the given capacity and core
// count.
par_omap_ptr ParOMapNew(uint32_t capacity, uint32_t numCores);

// Insert a batch of (key, value) pairs. Keys are 16 bytes each, values are 4
// bytes each.
void ParOMapInsertBatch(par_omap_ptr omap, uint32_t batchSize, const void* keys,
                        const void* vals);

// Find a batch of keys. Keys are 16 bytes each, values output are 4 bytes each.
// existFlags outputs 1 if key was found, 0 otherwise.
void ParOMapFindBatch(par_omap_ptr omap, uint32_t batchSize, const void* keys,
                      void* vals, uint8_t* existFlags);

// Destroy the ParOMap instance.
void ParOMapDestroy(par_omap_ptr omap);

#ifdef __cplusplus
}
#endif

#endif
