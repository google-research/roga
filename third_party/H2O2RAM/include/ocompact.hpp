#pragma once

#include <assert.h>
#include <omp.h>

#include <algorithm>
#include <execution>
#include <iomanip>
#include <iostream>
#include <iterator>
#include <numeric>
#include <random>
#include <stdexcept>
#include <thread>
#include <variant>
#include <vector>

#include "depth_counter.hpp"
#include "iterator_stride.hpp"
#include "oblivious_operations.hpp"
namespace ORAM {
template <typename Iter, typename Ref>
inline auto get_iter_from_ref(const Iter& base_iter, Ref& x) {
  if constexpr (std::is_pointer_v<Iter>) {
    return &x;
  } else if constexpr (requires { Iter(&x); }) {
    return Iter(&x);
  } else {
    return base_iter + (&x - &*base_iter);
  }
}

template <typename Iter, typename StrideIter>
inline void maybe_set_stride(Iter& iter, const StrideIter& stride_source) {
  if constexpr (requires { iter.set_stride(stride_source.get_stride()); }) {
    iter.set_stride(stride_source.get_stride());
  }
}

template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt,
          std::random_access_iterator OffsetsIt>
void _or_off_compact_serial(
    DataIt data_first, FlagIt flag_first, OffsetsIt offsets_first,
    const typename std::iterator_traits<OffsetsIt>::value_type start_index,
    const typename std::iterator_traits<OffsetsIt>::value_type z,
    const typename std::iterator_traits<OffsetsIt>::value_type n) {
  using offset_type = typename std::iterator_traits<OffsetsIt>::value_type;
  if (n <= 1) return;
  if (n == 2) {
    offset_type p1 = 1 +
                     (start_index == 0 ? 0 : offsets_first[start_index - 1]) -
                     offsets_first[start_index];
    offset_type p2 =
        offsets_first[start_index + 1] - offsets_first[start_index];
    obliSwap(data_first[start_index], data_first[start_index + 1],
             (p1 & p2) ^ z);
    obliSwap(flag_first[start_index], flag_first[start_index + 1],
             (p1 & p2) ^ z);
    return;
  }
  const offset_type mod = n / 2 - 1;
  const offset_type m = offsets_first[start_index + mod] -
                        (start_index == 0 ? 0 : offsets_first[start_index - 1]);
  _or_off_compact_serial(data_first, flag_first, offsets_first, start_index,
                         z & mod, n / 2);
  _or_off_compact_serial(data_first, flag_first, offsets_first,
                         start_index + n / 2, (z + m) & mod, n / 2);
  const offset_type s = (((z & mod) + m) >= n / 2) ^ (z >= n / 2);
  const auto data_st = data_first + start_index;
  const auto flag_st = flag_first + start_index;
  std::for_each(data_st, data_st + n / 2, [z, m, s, data_st, n, mod](auto& x) {
    auto cur_iter = get_iter_from_ref(data_st, x);
    maybe_set_stride(cur_iter, data_st);
    offset_type i = cur_iter - data_st;
    obliSwap(x, *(cur_iter + n / 2), (i >= ((z + m) & mod)) ^ s);
  });
  std::for_each(flag_st, flag_st + n / 2, [z, m, s, flag_st, n, mod](auto& x) {
    auto cur_iter = get_iter_from_ref(flag_st, x);
    maybe_set_stride(cur_iter, flag_st);
    offset_type i = cur_iter - flag_st;
    obliSwap(x, *(cur_iter + n / 2), (i >= ((z + m) & mod)) ^ s);
  });
}

template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt,
          std::random_access_iterator OffsetsIt>
void _or_off_compact(
    DataIt data_first, FlagIt flag_first, OffsetsIt offsets_first,
    const typename std::iterator_traits<OffsetsIt>::value_type start_index,
    const typename std::iterator_traits<OffsetsIt>::value_type z,
    const typename std::iterator_traits<OffsetsIt>::value_type n, int threads) {
  using offset_type = typename std::iterator_traits<OffsetsIt>::value_type;
  if (n <= 1) return;
  if (n == 2) {
    offset_type p1 = 1 +
                     (start_index == 0 ? 0 : offsets_first[start_index - 1]) -
                     offsets_first[start_index];
    offset_type p2 =
        offsets_first[start_index + 1] - offsets_first[start_index];
    obliSwap(data_first[start_index], data_first[start_index + 1],
             (p1 & p2) ^ z);
    obliSwap(flag_first[start_index], flag_first[start_index + 1],
             (p1 & p2) ^ z);
    return;
  }
  const offset_type mod = n / 2 - 1;
  const offset_type m = offsets_first[start_index + mod] -
                        (start_index == 0 ? 0 : offsets_first[start_index - 1]);

  if (threads <= 1 || n <= (1 << 12)) {
    _or_off_compact_serial(data_first, flag_first, offsets_first, start_index,
                           z, n);
    return;
  } else {
#pragma omp parallel sections num_threads(threads)
    {
#pragma omp section
      {
        _or_off_compact(data_first, flag_first, offsets_first, start_index,
                        z & mod, n / 2, threads / 2);
      }
#pragma omp section
      {
        _or_off_compact(data_first, flag_first, offsets_first,
                        start_index + n / 2, (z + m) & mod, n / 2, threads / 2);
      }
    }
  }
  const offset_type s = (((z & mod) + m) >= n / 2) ^ (z >= n / 2);
  const auto data_st = data_first + start_index;
  const auto flag_st = flag_first + start_index;
  if (n >= (1 << 12) && threads > 1) {
    std::for_each(std::execution::par_unseq, data_st, data_st + n / 2,
                  [z, m, s, data_st, n, mod](auto& x) {
                    auto cur_iter = get_iter_from_ref(data_st, x);
                    maybe_set_stride(cur_iter, data_st);
                    offset_type i = cur_iter - data_st;
                    obliSwap(x, *(cur_iter + n / 2),
                             (i >= ((z + m) & mod)) ^ s);
                  });
    std::for_each(std::execution::par_unseq, flag_st, flag_st + n / 2,
                  [z, m, s, flag_st, n, mod](auto& x) {
                    auto cur_iter = get_iter_from_ref(flag_st, x);
                    maybe_set_stride(cur_iter, flag_st);
                    offset_type i = cur_iter - flag_st;
                    obliSwap(x, *(cur_iter + n / 2),
                             (i >= ((z + m) & mod)) ^ s);
                  });
  } else {
    std::for_each(
        data_st, data_st + n / 2, [z, m, s, data_st, n, mod](auto& x) {
          auto cur_iter = get_iter_from_ref(data_st, x);
          maybe_set_stride(cur_iter, data_st);
          offset_type i = cur_iter - data_st;
          obliSwap(x, *(cur_iter + n / 2), (i >= ((z + m) & mod)) ^ s);
        });
    std::for_each(
        flag_st, flag_st + n / 2, [z, m, s, flag_st, n, mod](auto& x) {
          auto cur_iter = get_iter_from_ref(flag_st, x);
          maybe_set_stride(cur_iter, flag_st);
          offset_type i = cur_iter - flag_st;
          obliSwap(x, *(cur_iter + n / 2), (i >= ((z + m) & mod)) ^ s);
        });
  }
}

template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt,
          std::unsigned_integral PrefixSumTp, typename ExecPolicy>
  requires std::is_execution_policy_v<ExecPolicy>
void _or_off_compact_entry(DataIt data_first, FlagIt flag_first,
                           const ExecPolicy& exec_par, PrefixSumTp n,
                           int threads = omp_get_num_threads()) {
  auto unary_op = [](auto x) -> PrefixSumTp {
    // should be `x > 0' if flags are not correctly initialized
    return x;
  };
  (void)exec_par;
  std::vector<PrefixSumTp> offset_prefix_sum(n);
  std::transform_inclusive_scan(flag_first, flag_first + n,
                                offset_prefix_sum.begin(),
                                std::plus<PrefixSumTp>{}, unary_op);

  _or_off_compact(data_first, flag_first, offset_prefix_sum.begin(), 0, 0, n,
                  threads);
}

// 1s first
template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt>
void or_compact_power_2(DataIt data_first, FlagIt flag_first, size_t n,
                        size_t threads = omp_get_max_threads()) {
  if (n <= 1) return;
  if (n == 2) {
    obliSwap(*data_first, *(data_first + 1), !*flag_first);
    obliSwap(*flag_first, *(flag_first + 1), !*flag_first);
    return;
  }
  assert(std::has_single_bit(n));
  omp_set_nested(1);
  if (n <= PARTIAL_SUM_PARALLEL_THRESHOLD)
    _or_off_compact_entry<DataIt, FlagIt, uint32_t,
                          std::execution::sequenced_policy>(
        data_first, flag_first, std::execution::seq, n, threads);
  else if (n < std::numeric_limits<int32_t>::max())
    _or_off_compact_entry<DataIt, FlagIt, uint32_t,
                          std::execution::parallel_unsequenced_policy>(
        data_first, flag_first, std::execution::par_unseq, n, threads);
  else
    _or_off_compact_entry<DataIt, FlagIt, size_t,
                          std::execution::parallel_unsequenced_policy>(
        data_first, flag_first, std::execution::par_unseq, n, threads);
}

template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt, std::unsigned_integral NumTp,
          RandomEngine EngineTp>
void _ocompact_by_half_rand_cyclic_shift(DataIt data_first, FlagIt flag_first,
                                         const NumTp n, const NumTp Z,
                                         EngineTp& gen) {
  assert(std::has_single_bit(Z));
  using DataTp = typename std::iterator_traits<DataIt>::value_type;
  using FlagTp = typename std::iterator_traits<FlagIt>::value_type;
  // cyclically shift each bucket
  const NumTp b = n / Z;
  if (b <= 1 || b * sizeof(DataTp) < sysconf(_SC_LEVEL1_DCACHE_SIZE)) {
    or_compact_power_2(data_first, flag_first, n);
    return;
  }
  std::uniform_int_distribution<NumTp> dist(0, b - 1);
  std::vector<NumTp> shifts(Z);
  for (NumTp i = 0; i < Z; i++) shifts[i] = dist(gen);

#pragma omp parallel for if (b >= 32768 / sizeof(*data_first))
  for (NumTp i = 0; i < Z; i++) {
    NumTp start = i * b;
    NumTp end = (i + 1) * b;
    std::rotate(data_first + start, data_first + start + shifts[i],
                data_first + end);
    std::rotate(flag_first + start, flag_first + start + shifts[i],
                flag_first + end);
  }

  if (sizeof(DataTp) > sysconf(_SC_LEVEL1_DCACHE_SIZE)) {
#pragma omp parallel for
    for (NumTp i = 0; i < b; i++) {
      IteratorStride<DataTp> data_stride_iter(data_first + i, b);
      IteratorStride<FlagTp> flag_stride_iter(flag_first + i, b);
      _or_off_compact_entry(data_stride_iter, flag_stride_iter,
                            std::execution::seq, (uint32_t)Z, 1);
    }
  } else {
    std::vector<DataTp> data_temp(Z);
    std::vector<FlagTp> flag_temp(Z);
#pragma omp parallel for firstprivate(data_temp, flag_temp)
    for (NumTp i = 0; i < b; i++) {
      for (NumTp j = 0; j < Z; j++) {
        data_temp[j] = data_first[i + j * b];
        flag_temp[j] = flag_first[i + j * b];
      }
      _or_off_compact_entry(data_temp.begin(), flag_temp.begin(),
                            std::execution::seq, (uint32_t)Z, 1);
      for (NumTp j = 0; j < Z; j++) {
        data_first[i + j * b] = data_temp[j];
        flag_first[i + j * b] = flag_temp[j];
      }
    }
  }

  // recursion on the middle half
  if constexpr (std::is_same<size_t, decltype(n)>::value) {
    if (n / 2 < (size_t)std::numeric_limits<int32_t>::max())
      _ocompact_by_half_rand_cyclic_shift<DataIt, FlagIt, uint32_t, EngineTp>(
          data_first + n / 4, flag_first + n / 4, n / 2, Z, gen);
    else
      _ocompact_by_half_rand_cyclic_shift<DataIt, FlagIt, size_t, EngineTp>(
          data_first + n / 4, flag_first + n / 4, n / 2, Z, gen);
  } else
    _ocompact_by_half_rand_cyclic_shift<DataIt, FlagIt, uint32_t, EngineTp>(
        data_first + n / 4, flag_first + n / 4, n / 2, Z, gen);
}

// 1s first
template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt>
void ocompact_by_half(DataIt data_first, FlagIt flag_first, size_t n, size_t Z,
                      uint32_t seed = std::random_device{}()) {
  static std::mt19937 gen(seed);
  omp_set_num_threads(omp_get_max_threads());
  assert(std::has_single_bit(n));
  assert(std::has_single_bit(Z));
  if (n < (size_t)std::numeric_limits<int32_t>::max())
    _ocompact_by_half_rand_cyclic_shift<DataIt, FlagIt, uint32_t, std::mt19937>(
        data_first, flag_first, (uint32_t)n, Z, gen);
  else
    _ocompact_by_half_rand_cyclic_shift<DataIt, FlagIt, size_t, std::mt19937>(
        data_first, flag_first, n, Z, gen);
}

template <std::random_access_iterator DataIt,
          std::random_access_iterator FlagIt, std::unsigned_integral NumTp>
void _ocompact_by_half_rand_exp(DataIt data_first, FlagIt flag_first,
                                const NumTp n, const NumTp Z, const NumTp depth,
                                const NumTp budget) {
  using DataTp = typename std::iterator_traits<DataIt>::value_type;
  using FlagTp = typename std::iterator_traits<FlagIt>::value_type;
  // cyclically shift each bucket
  const NumTp b = n / Z;
  if (b <= 1 || b * sizeof(DataTp) < sysconf(_SC_LEVEL1_DCACHE_SIZE)) {
    or_compact_power_2(data_first, flag_first, n);
    return;
  }
  // compact two compacted arrays with totally half distinguisable items
  if (b == 2 && depth != 0) {
#pragma omp parallel for
    for (NumTp i = 0; i < Z; i++) {
      obliSwap(data_first[2 * (Z - 1 - i)], data_first[2 * i + 1],
               (!flag_first[2 * (Z - 1 - i)]) & flag_first[2 * i + 1]);
      obliSwap(flag_first[2 * (Z - 1 - i)], flag_first[2 * i + 1],
               (!flag_first[2 * (Z - 1 - i)]) & flag_first[2 * i + 1]);
    }
    return;
  }

  if (sizeof(DataTp) > sysconf(_SC_LEVEL1_DCACHE_SIZE)) {
#pragma omp parallel for
    for (NumTp i = 0; i < b; i++) {
      IteratorStride<DataTp> data_stride_iter(data_first + i, b);
      IteratorStride<FlagTp> flag_stride_iter(flag_first + i, b);
      _or_off_compact_entry(data_stride_iter, flag_stride_iter,
                            std::execution::seq, (uint32_t)Z, 1);
    }
  } else {
    std::vector<DataTp> data_temp(Z);
    std::vector<FlagTp> flag_temp(Z);
#pragma omp parallel for firstprivate(data_temp, flag_temp)
    for (NumTp i = 0; i < b; i++) {
      for (NumTp j = 0; j < Z; j++) {
        data_temp[j] = data_first[i + j * b];
        flag_temp[j] = flag_first[i + j * b];
      }
      _or_off_compact_entry(data_temp.begin(), flag_temp.begin(),
                            std::execution::seq, (uint32_t)Z, 1);
      for (NumTp j = 0; j < Z; j++) {
        data_first[i + j * b] = data_temp[j];
        flag_first[i + j * b] = flag_temp[j];
      }
    }
  }

  // recursion on the middle half
  if constexpr (std::is_same<size_t, decltype(n)>::value) {
    if (n / 2 < (size_t)std::numeric_limits<int32_t>::max())
      _ocompact_by_half_rand_exp<DataIt, FlagIt, uint32_t>(
          data_first + n / 4, flag_first + n / 4, n / 2,
          //  depth % 2 == 0 && budget > 1 || b == 4 ? Z : Z * 2,
          Z * 2, depth + 1, depth % 2 == 0 && budget > 1 ? budget - 1 : budget);
    else
      _ocompact_by_half_rand_exp<DataIt, FlagIt, size_t>(
          data_first + n / 4, flag_first + n / 4, n / 2,
          //    depth % 2 == 0 && budget > 1 || b == 4 ? Z : Z * 2,
          Z * 2, depth + 1, depth % 2 == 0 && budget > 1 ? budget - 1 : budget);
  } else
    _ocompact_by_half_rand_exp<DataIt, FlagIt, uint32_t>(
        data_first + n / 4, flag_first + n / 4, n / 2,
        //  depth % 2 == 0 && budget > 1 || b == 4 ? Z : Z * 2,
        Z * 2, depth + 1, depth % 2 == 0 && budget > 1 ? budget - 1 : budget);
}

template <std::random_access_iterator DataIt1,
          std::random_access_iterator DataIt2,
          std::random_access_iterator FlagIt1,
          std::random_access_iterator FlagIt2, std::unsigned_integral NumTp>
void _ocompact_two_compacted_arrays_serial(DataIt1 data_first_1,
                                           FlagIt1 flag_first_1,
                                           DataIt2 data_first_2,
                                           FlagIt2 flag_first_2, NumTp n) {
  if (n == 1) {
    obliSwap(*data_first_1, *data_first_2, (!*flag_first_1) & (*flag_first_2));
    obliSwap(*flag_first_1, *flag_first_2, (!*flag_first_1) & (*flag_first_2));
    return;
  }
  for (NumTp i = 0; i < n; i++) {
    obliSwap(data_first_1[i], data_first_2[i],
             (!flag_first_1[i]) & flag_first_2[i]);
    obliSwap(flag_first_1[i], flag_first_2[i],
             (!flag_first_1[i]) & flag_first_2[i]);
  }

  if (n / 2 < std::numeric_limits<int32_t>::max()) {
    _ocompact_two_compacted_arrays_serial(
        data_first_1, flag_first_1, data_first_1 + n / 2, flag_first_1 + n / 2,
        (uint32_t)(n / 2));
    _ocompact_two_compacted_arrays_serial(
        data_first_2, flag_first_2, data_first_2 + n / 2, flag_first_2 + n / 2,
        (uint32_t)(n / 2));
  } else {
    _ocompact_two_compacted_arrays_serial(data_first_1, flag_first_1,
                                          data_first_1 + n / 2,
                                          flag_first_1 + n / 2, n / 2);
    _ocompact_two_compacted_arrays_serial(data_first_2, flag_first_2,
                                          data_first_2 + n / 2,
                                          flag_first_2 + n / 2, n / 2);
  }
}

template <std::random_access_iterator DataIt1,
          std::random_access_iterator DataIt2,
          std::random_access_iterator FlagIt1,
          std::random_access_iterator FlagIt2, std::unsigned_integral NumTp>
void _ocompact_two_compacted_arrays_serial(DataIt1 data_first_1,
                                           FlagIt1 flag_first_1,
                                           DataIt2 data_first_2,
                                           FlagIt2 flag_first_2, NumTp n,
                                           NumTp bit_budget) {
  assert(std::has_single_bit(n));
  if (bit_budget == 0) {
    _ocompact_two_compacted_arrays_serial(data_first_1, flag_first_1,
                                          data_first_2, flag_first_2, n);
    return;
  }
  if (n == 1)  // will never be used in our oram, but for completeness
  {
    if ((!*flag_first_1) & (*flag_first_2)) {
      obliSwap(*data_first_1, *data_first_2,
               (!*flag_first_1) & (*flag_first_2));
      obliSwap(*flag_first_1, *flag_first_2,
               (!*flag_first_1) & (*flag_first_2));
    }
    return;
  }
  NumTp cnt = 0;
  for (NumTp i = 0; i < n; i++) {
    obliSwap(data_first_1[i], data_first_2[i],
             (!flag_first_1[i]) & flag_first_2[i]);
    obliSwap(flag_first_1[i], flag_first_2[i],
             (!flag_first_1[i]) & flag_first_2[i]);
    cnt += (NumTp)flag_first_2[i];
  }

  if (cnt == 0) {
    if (n / 2 < std::numeric_limits<uint32_t>::max())
      _ocompact_two_compacted_arrays_serial(
          data_first_1, flag_first_1, data_first_1 + n / 2,
          flag_first_1 + n / 2, (uint32_t)(n / 2), (uint32_t)(bit_budget - 1));
    else
      _ocompact_two_compacted_arrays_serial(
          data_first_1, flag_first_1, data_first_1 + n / 2,
          flag_first_1 + n / 2, n / 2, bit_budget - 1);
  } else {
    if (n / 2 < std::numeric_limits<uint32_t>::max())
      _ocompact_two_compacted_arrays_serial(
          data_first_2, flag_first_2, data_first_2 + n / 2,
          flag_first_2 + n / 2, (uint32_t)(n / 2), (uint32_t)(bit_budget - 1));
    else
      _ocompact_two_compacted_arrays_serial(
          data_first_2, flag_first_2, data_first_2 + n / 2,
          flag_first_2 + n / 2, n / 2, bit_budget - 1);
  }
}

}  // namespace ORAM