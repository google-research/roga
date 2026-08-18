#include <iostream>
#include <vector>

#include "hash_planner.hpp"

int main() {
  std::cout << "=========================================================="
            << std::endl;
  std::cout << "  Stock H2O2RAM Hash Planner Calibration (Large Capacities) "
            << std::endl;
  std::cout << "=========================================================="
            << std::endl;

  // Test larger capacities: 2^18 (262k), 2^20 (1M), 2^22 (4M)
  std::vector<size_t> powers = {18, 20, 22};

  for (size_t p : powers) {
    size_t n = 1ULL << p;
    size_t op_num = n;
    size_t delta_inv_log2 = 40;

    std::cout << "\n[Profiling] Finding optimal hash plan for n = " << n
              << " (2^" << p << "), op_num = " << op_num << "..." << std::endl;

    ORAM::OHashBase<size_t, 16>* plan =
        ORAM::determine_hash<size_t, 16>(n, op_num, delta_inv_log2);

    if (plan) {
      std::cout << "  -> Selected Optimal Plan created successfully for n = "
                << n << std::endl;
      delete plan;
    } else {
      std::cout << "  -> Plan creation returned null for n = " << n
                << std::endl;
    }
  }

  std::cout << "\n=========================================================="
            << std::endl;
  std::cout << " Profiling Complete! Large capacity plans generated."
            << std::endl;
  std::cout << "=========================================================="
            << std::endl;

  return 0;
}
