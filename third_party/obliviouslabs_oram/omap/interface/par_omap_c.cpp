#include "par_omap_c.h"

#include <omp.h>

#include "odsl/par_omap.hpp"

using Bytes16 = Bytes<16>;
using Bytes4 = Bytes<4>;
using ParMapType = ODSL::ParOMap<Bytes16, Bytes4, uint32_t>;

extern "C" par_omap_ptr ParOMapNew(uint32_t capacity, uint32_t numCores) {
  uint32_t shardCount = ParMapType::GetSuitableShardCount(numCores, true);
  omp_set_num_threads(shardCount);
  auto* omap = new ParMapType(capacity, shardCount);
  omap->Init();
  return static_cast<par_omap_ptr>(omap);
}

extern "C" void ParOMapInsertBatch(par_omap_ptr omap, uint32_t batchSize,
                                   const void* keys, const void* vals) {
  auto* map = static_cast<ParMapType*>(omap);
  const Bytes16* keyArr = static_cast<const Bytes16*>(keys);
  const Bytes4* valArr = static_cast<const Bytes4*>(vals);
  map->InsertBatch(keyArr, keyArr + batchSize, valArr);
}

extern "C" void ParOMapFindBatch(par_omap_ptr omap, uint32_t batchSize,
                                 const void* keys, void* vals,
                                 uint8_t* existFlags) {
  auto* map = static_cast<ParMapType*>(omap);
  const Bytes16* keyArr = static_cast<const Bytes16*>(keys);
  Bytes4* valArr = static_cast<Bytes4*>(vals);
  auto flags = map->FindBatch(keyArr, keyArr + batchSize, valArr);
  for (uint32_t i = 0; i < batchSize; ++i) {
    existFlags[i] = flags[i];
  }
}

extern "C" void ParOMapDestroy(par_omap_ptr omap) {
  delete static_cast<ParMapType*>(omap);
}
