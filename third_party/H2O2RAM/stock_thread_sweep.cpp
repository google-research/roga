#include <omp.h>
#include <tbb/global_control.h>

#include <chrono>
#include <iostream>
#include <vector>

#include "hash_planner.hpp"
#include "oram.hpp"

void run_capacity_experiment(size_t capacity, size_t num_ops) {
  std::cout << "\n=========================================================="
            << std::endl;
  std::cout << " Stock H2O2RAM Multi-Thread Sweep (Capacity N = " << capacity
            << ", " << num_ops << " ops)" << std::endl;
  std::cout << "=========================================================="
            << std::endl;
  std::cout << "Initializing ObliviousRAM with capacity " << capacity << "..."
            << std::endl;

  ORAM::ObliviousRAM<size_t, uint64_t> oram(capacity);

  std::cout << "Populating ORAM..." << std::endl;
  for (size_t i = 0; i < capacity; i++) {
    oram.insert(i, i * 10);
  }
  std::cout << "Population complete. Starting thread count sweep..."
            << std::endl;
  std::cout << "----------------------------------------------------------"
            << std::endl;
  std::cout << " Threads |      Ops | Total Time (ms) |     us/op |   Ops/sec"
            << std::endl;
  std::cout << "----------------------------------------------------------"
            << std::endl;

  std::vector<size_t> threads = {1, 4, 16, 32, 64};

  for (size_t t : threads) {
    omp_set_num_threads(t);
    tbb::global_control control(tbb::global_control::max_allowed_parallelism,
                                t);

    auto start = std::chrono::high_resolution_clock::now();
#pragma omp parallel for schedule(static)
    for (size_t i = 0; i < num_ops; i++) {
      uint64_t key = (i * 13) % capacity;
      uint64_t val = oram[key];
      (void)val;
    }
    auto end = std::chrono::high_resolution_clock::now();

    std::chrono::duration<double, std::milli> dur = end - start;
    double us_per_op = (dur.count() * 1000.0) / num_ops;
    double ops_per_sec = (num_ops / dur.count()) * 1000.0;

    printf(" %7zu | %8zu | %15.2f | %9.2f | %9.0f\n", t, num_ops, dur.count(),
           us_per_op, ops_per_sec);
  }
  std::cout << "=========================================================="
            << std::endl;
}

int main() {
  // Capacities: 262k (2^18), 1.04M (2^20), 4.19M (2^22)
  run_capacity_experiment(262144, 10000);
  run_capacity_experiment(1048576, 10000);
  run_capacity_experiment(4194304, 10000);
  return 0;
}
