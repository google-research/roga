#include <chrono>
#include <iostream>

#include "hash_planner.hpp"
#include "oram.hpp"

int main() {
  std::cout << "==========================================" << std::endl;
  std::cout << "   Stock H2O2RAM Quick Benchmark          " << std::endl;
  std::cout << "==========================================" << std::endl;

  size_t map_size = 4096;
  size_t num_ops = 1000;

  std::cout << "Creating ObliviousRAM with capacity " << map_size << "..."
            << std::endl;
  ORAM::ObliviousRAM<size_t, uint64_t> oram(map_size);

  std::cout << "Running " << num_ops << " inserts..." << std::endl;
  auto start = std::chrono::high_resolution_clock::now();
  for (uint64_t i = 0; i < num_ops; i++) {
    oram.insert(i, i * 10);
  }
  auto mid = std::chrono::high_resolution_clock::now();

  std::cout << "Running " << num_ops << " lookups..." << std::endl;
  size_t errors = 0;
  for (uint64_t i = 0; i < num_ops; i++) {
    uint64_t val = oram[i];
    if (val != i * 10) {
      errors++;
    }
  }
  auto end = std::chrono::high_resolution_clock::now();

  std::chrono::duration<double, std::milli> insert_dur = mid - start;
  std::chrono::duration<double, std::milli> lookup_dur = end - mid;
  std::chrono::duration<double, std::milli> total_dur = end - start;

  std::cout << "------------------------------------------" << std::endl;
  std::cout << "Results:" << std::endl;
  std::cout << "  Insert time : " << insert_dur.count() << " ms ("
            << (insert_dur.count() * 1000.0 / num_ops) << " us/op)"
            << std::endl;
  std::cout << "  Lookup time : " << lookup_dur.count() << " ms ("
            << (lookup_dur.count() * 1000.0 / num_ops) << " us/op)"
            << std::endl;
  std::cout << "  Total time  : " << total_dur.count() << " ms ("
            << (total_dur.count() * 1000.0 / (2 * num_ops)) << " us/op)"
            << std::endl;
  std::cout << "  Errors      : " << errors << std::endl;
  std::cout << "==========================================" << std::endl;

  return 0;
}
