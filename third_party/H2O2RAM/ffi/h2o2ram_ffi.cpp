#include "h2o2ram_ffi.h"

#include <omp.h>
#include <openssl/sha.h>
#include <tbb/task_arena.h>

#include <cstddef>
#include <cstring>
#include <memory>
#include <mutex>
#include <new>

#include "hash_planner.hpp"
#include "oblivious_operations.hpp"
#include "omap.hpp"
#include "types.hpp"

#ifndef H2O2RAM_KEY_MAX
#define H2O2RAM_KEY_MAX 32
#endif
#ifndef H2O2RAM_VAL_SIZE
#define H2O2RAM_VAL_SIZE 32
#endif

namespace h2o2ram_detail {
constexpr size_t KEY_MAX = H2O2RAM_KEY_MAX;
constexpr size_t VAL_SIZE = H2O2RAM_VAL_SIZE;

// Miss returns a recycled block, usually a copy of another key's slot, so
// the slot carries its index to prove it belongs here.
struct Slot {
  uint64_t index;
  uint64_t key_len;
  uint8_t key[KEY_MAX];
  uint8_t val[VAL_SIZE];
};

static_assert(sizeof(Slot) == 16 + KEY_MAX + VAL_SIZE, "Slot is padded");
static_assert(sizeof(Slot) % 8 == 0,
              "KEY_MAX + VAL_SIZE must be a multiple of 8");

using Map = ORAM::ObliviousMap<uint64_t, Slot>;
using BlockT = ORAM::Block<uint64_t, sizeof(Slot) + sizeof(uint64_t)>;

static_assert(sizeof(BlockT) == sizeof(Slot) + sizeof(uint64_t),
              "Block is padded");
static_assert(offsetof(BlockT, value) == sizeof(uint64_t),
              "Block id must sit directly before value");

// operator[] returns a reference to a Block's value, so its id sits just
// before. Needed to hand unwanted blocks back instead of leaking capacity.
inline uint64_t& block_id_of(Slot& s) {
  auto* p = reinterpret_cast<uint8_t*>(&s) - offsetof(BlockT, value);
  return reinterpret_cast<BlockT*>(p)->id;
}

// ObliviousRAM's own marker for an invalid block.
constexpr uint64_t DEAD_BLOCK = (uint64_t)-1;

// ObliviousRAM doubles from LINEAR_SCAN_THRESHOLD up to MIN_CAPACITY.
inline size_t effective_capacity() {
  size_t cap = LINEAR_SCAN_THRESHOLD;
  while (cap < (size_t)MIN_CAPACITY) cap <<= 1;
  return cap;
}

// Top bit cleared: ObliviousMap<uint64_t> uses the key as the index
// verbatim, and Block::dummy() reads that bit as the dummy flag.
inline uint64_t index_of(const uint8_t* key, size_t key_len) {
  uint8_t md[SHA256_DIGEST_LENGTH];
  SHA256(key, key_len, md);
  uint64_t idx;
  std::memcpy(&idx, md, sizeof(idx));
  return idx & ~(1ULL << 63);
}

// Real data only if tagged with the index it was looked up under.
// Length check uses unsigned wraparound: 0 underflows to UINT64_MAX.
inline bool slot_live(const Slot& s, uint64_t idx) {
  return (s.index == idx) & (s.key_len - 1 < KEY_MAX);
}

// Compares the full KEY_MAX window, so timing never depends on the slot.
inline bool key_matches(const Slot& s, const uint8_t* key, size_t key_len) {
  uint64_t diff = s.key_len ^ (uint64_t)key_len;
  for (size_t i = 0; i < KEY_MAX; i++) {
    uint8_t want = i < key_len ? key[i] : 0;
    diff |= (uint64_t)(uint8_t)(s.key[i] ^ want);
  }
  return diff == 0;
}

// OpenMP half of the core cap. Its thread count is a per-thread ICV, so
// set it around the call and restore. threads == 0 means unrestricted.
class ThreadBound {
  unsigned threads;
  int prev;

 public:
  explicit ThreadBound(unsigned threads_)
      : threads(threads_), prev(omp_get_max_threads()) {
    if (threads) omp_set_num_threads((int)threads);
  }

  ~ThreadBound() {
    if (threads) omp_set_num_threads(prev);
  }
};
}  // namespace h2o2ram_detail

struct h2o2ram_map {
  // Declared first: must outlive the map it scopes.
  tbb::task_arena arena;
  unsigned threads;
  std::unique_ptr<h2o2ram_detail::Map> omap;
  size_t live = 0;
  size_t capacity = h2o2ram_detail::effective_capacity();

  explicit h2o2ram_map(unsigned threads_)
      : arena(threads_ == 0 ? tbb::task_arena::automatic : (int)threads_),
        threads(threads_) {
    // Inside the arena too: construction and autotuning are the most
    // parallel phase.
    h2o2ram_detail::ThreadBound bound(threads);
    arena.execute([&] { omap = std::make_unique<h2o2ram_detail::Map>(); });
  }

  // Applies this map's core cap to both runtimes.
  template <typename F>
  int run(F&& f) {
    h2o2ram_detail::ThreadBound bound(threads);
    int rc = 0;
    arena.execute([&] { rc = f(); });
    return rc;
  }
};

extern "C" {
size_t h2o2ram_key_max(void) { return h2o2ram_detail::KEY_MAX; }
size_t h2o2ram_val_size(void) { return h2o2ram_detail::VAL_SIZE; }
size_t h2o2ram_capacity(void) { return h2o2ram_detail::effective_capacity(); }

h2o2ram_map* h2o2ram_map_new(unsigned threads) {
  // determine_hash mutates an unsynchronised static cache and the global
  // DepthCounter. Only construction reaches them, so serialise it.
  static std::mutex construction_lock;
  try {
    std::lock_guard<std::mutex> guard(construction_lock);
    auto* map = new h2o2ram_map(threads);
    return map;
  } catch (...) {
    return nullptr;
  }
}

unsigned h2o2ram_threads(const h2o2ram_map* map) {
  if (map == nullptr) return 0;
  return map->threads ? map->threads : (unsigned)map->arena.max_concurrency();
}

void h2o2ram_map_free(h2o2ram_map* map) { delete map; }

size_t h2o2ram_len(const h2o2ram_map* map) { return map ? map->live : 0; }

int h2o2ram_insert(h2o2ram_map* map, const uint8_t* key, size_t key_len,
                   const uint8_t* val) {
  using namespace h2o2ram_detail;

  if (map == nullptr || key == nullptr || val == nullptr) return H2O2RAM_EINVAL;
  if (key_len == 0 || key_len > KEY_MAX) return H2O2RAM_EINVAL;

  const uint64_t idx = index_of(key, key_len);

  Slot fresh;
  fresh.index = idx;
  fresh.key_len = key_len;
  std::memcpy(fresh.key, key, key_len);
  std::memset(fresh.key + key_len, 0, KEY_MAX - key_len);
  std::memcpy(fresh.val, val, VAL_SIZE);

  return map->run([&] {
    // Read-modify-write, never ObliviousRAM::insert: insert() appends a
    // second block under the same id and a rebuild can then return
    // either. operator[] extracts the existing element first.
    Slot& s = (*map->omap)[idx];

    const bool live = slot_live(s, idx);
    const bool same = live & key_matches(s, key, key_len);
    // Bitwise, not &&, to avoid short circuit branches on slot content.
    const bool room = map->live < map->capacity;
    const bool claim = !live & room;
    // Refused: hand the block back instead of leaking a capacity slot.
    // A live slot is kept even on collision, it is another key's data.
    const bool drop = !live & !room;

    // Branchless, so the trace does not reveal whether the key existed.
    // Scrub first, then claim overwrites it.
    const Slot empty{};
    ORAM::CMOV(!live, s, empty);
    ORAM::CMOV(same | claim, s, fresh);
    ORAM::CMOV(drop, block_id_of(s), DEAD_BLOCK);

    map->live += (size_t)claim;

    // Selected, not branched. In precedence order: same implies live.
    int rc = H2O2RAM_FULL;
    ORAM::CMOV(claim, rc, (int)H2O2RAM_OK);
    ORAM::CMOV(live, rc, (int)H2O2RAM_COLLISION);
    ORAM::CMOV(same, rc, (int)H2O2RAM_OK);
    return rc;
  });
}

int h2o2ram_get(h2o2ram_map* map, const uint8_t* key, size_t key_len,
                uint8_t* val_out) {
  using namespace h2o2ram_detail;

  if (map == nullptr || key == nullptr || val_out == nullptr)
    return H2O2RAM_EINVAL;
  if (key_len == 0 || key_len > KEY_MAX) return H2O2RAM_EINVAL;

  const uint64_t idx = index_of(key, key_len);

  return map->run([&] {
    Slot& s = (*map->omap)[idx];

    const bool live = slot_live(s, idx);
    const bool same = live & key_matches(s, key, key_len);

    Slot out{};
    ORAM::CMOV(same, out, s);
    std::memcpy(val_out, out.val, VAL_SIZE);

    // Scrub the recycled miss block and hand it back, else every key
    // ever missed occupies a capacity slot forever. Live slots stay.
    const Slot empty{};
    ORAM::CMOV(!live, s, empty);
    ORAM::CMOV(!live, block_id_of(s), DEAD_BLOCK);

    int rc = H2O2RAM_NOT_FOUND;
    ORAM::CMOV(same, rc, (int)H2O2RAM_OK);
    return rc;
  });
}
}
