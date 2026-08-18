#include <benchmark/benchmark.h>

#include <cassert>
#include <random>
#include <vector>

#include "hash_planner.hpp"
#include "oram.hpp"
#include "types.hpp"

class ORAMDataFixture128 : public benchmark::Fixture {
 public:
  size_t n;
  using IndexType = size_t;
  std::vector<ORAM::Block<IndexType, 128 - sizeof(IndexType)>> raw_data;
  std::random_device rd;
  ORAM::ObliviousRAM<size_t, ORAM::Block<IndexType, 128 - sizeof(IndexType)>>*
      oram;

  void SetUp(const ::benchmark::State& state) override {
    std::mt19937 gen(rd());
    n = state.range(0);
    raw_data.resize(n);
    for (size_t i = 0; i < n; i++) raw_data[i].id = i;
    std::shuffle(raw_data.begin(), raw_data.end(), gen);
    oram =
        new ORAM::ObliviousRAM<size_t,
                               ORAM::Block<IndexType, 128 - sizeof(IndexType)>>(
            raw_data.begin(), raw_data.end());
  }

  void TearDown(const ::benchmark::State&) override {
    raw_data.clear();
    raw_data.shrink_to_fit();
    delete oram;
  }
};

BENCHMARK_DEFINE_F(ORAMDataFixture128, ORAM)
(benchmark::State& state) {
  for (auto _ : state) {
    for (uint32_t i = 0; i < n; i++) assert((*oram)[i].id == raw_data[i].id);
  }
}

static void CustomizedArgsN(benchmark::internal::Benchmark* b) {
  // Test large capacities: 2^18 (262k) and 2^20 (1M)
  b->Args({1 << 18});
  b->Args({1 << 20});
}

BENCHMARK_REGISTER_F(ORAMDataFixture128, ORAM)
    ->Apply(CustomizedArgsN)
    ->MeasureProcessCPUTime()
    ->UseRealTime();

BENCHMARK_MAIN();
