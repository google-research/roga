// Based on oblivious algorithms from ROSTL (https://eprint.iacr.org/2022/1333.pdf).
// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Oblivious compaction and distribution algorithms.

use crate::oblivious::ct::{cswap_fast, Cmov};
use crate::oblivious::reduction::Reducible;

#[inline]
fn compact_pow2<T: Cmov>(arr: &mut [T], payload: &[usize], z: usize) {
    let n = arr.len();
    if n == 2 {
        let m = payload[1] - payload[0];
        cswap_fast(arr, 0, 1, ((!m) & (payload[2] - payload[1])) != z);
        return;
    }
    let half = n / 2;
    let m = payload[half] - payload[0];
    let (zleft, zright) = (z & (half - 1), (z + m) & (half - 1));
    let (left, right) = arr.split_at_mut(half);

    compact_pow2(left, &payload[..half + 1], zleft);
    compact_pow2(right, &payload[half..], zright);

    let s = (zleft + m >= half) ^ (z >= half);
    for i in 0..half {
        cswap_fast(arr, i, half + i, s ^ (i >= zright));
    }
}

/// Stably compacts an array `arr` of length n using oblivious compaction.
/// The payload array `payload` is the prefix sum of valid elements.
/// Uses `https://eprint.iacr.org/2022/1333.pdf`
#[inline]
pub fn compact_payload<T: Cmov>(arr: &mut [T], payload: &[usize]) {
    debug_assert_eq!(arr.len() + 1, payload.len());
    let n = arr.len();
    if n <= 1 {
        return;
    }

    let n1 = 1 << (usize::BITS - 1 - n.leading_zeros());
    let n2 = n - n1;
    if n2 == 0 {
        return compact_pow2(arr, payload, 0);
    }

    let m = payload[n2] - payload[0];
    let (left, right) = arr.split_at_mut(n2);
    compact_payload(left, &payload[..n2 + 1]);
    compact_pow2(right, &payload[n2..], (n1 - n2 + m) % n1);

    for i in 0..n2 {
        cswap_fast(arr, i, n1 + i, i >= m);
    }
}

#[inline]
fn distribute_pow2<T: Cmov>(arr: &mut [T], payload: &[usize], z: usize) {
    let n = arr.len();
    if n == 2 {
        let m = payload[1] - payload[0];
        cswap_fast(arr, 0, 1, ((!m) & (payload[2] - payload[1])) != z);
        return;
    }
    let half = n / 2;
    let m = payload[half] - payload[0];
    let (zleft, zright) = (z & (half - 1), (z + m) & (half - 1));

    let s = (zleft + m >= half) ^ (z >= half);
    for i in 0..half {
        cswap_fast(arr, i, half + i, s ^ (i >= zright));
    }

    let (left, right) = arr.split_at_mut(half);
    distribute_pow2(left, &payload[..half + 1], zleft);
    distribute_pow2(right, &payload[half..], zright);
}

/// Distributes the elements of arr according to the prefix sum payload (reverse of compaction).
#[inline]
pub fn distribute_payload<T: Cmov>(arr: &mut [T], payload: &[usize]) {
    debug_assert_eq!(arr.len() + 1, payload.len());
    let n = arr.len();
    if n <= 1 {
        return;
    }

    let n1 = 1 << (usize::BITS - 1 - n.leading_zeros());
    let n2 = n - n1;
    if n2 == 0 {
        return distribute_pow2(arr, payload, 0);
    }

    let m = payload[n2] - payload[0];
    for i in 0..n2 {
        cswap_fast(arr, i, n1 + i, i >= m);
    }

    let (left, right) = arr.split_at_mut(n2);
    distribute_payload(left, &payload[..n2 + 1]);
    distribute_pow2(right, &payload[n2..], (n1 - n2 + m) % n1);
}
/// Computes the prefix sums of real elements to form compaction markers.
pub fn compact_marks<V, T: Reducible<V>>(records: &[T], marks: &mut Vec<usize>) {
    let n = records.len();
    marks.resize(n + 1, 0);
    marks[0] = 0;
    for i in 0..n {
        marks[i + 1] = marks[i] + records[i].ct_is_real() as usize;
    }
}

use crate::OramValue;

/// Sorts blocks by keys computed via `key_of` using djbsort, then reduces runs of equal items.
pub fn sort_reduce<V: OramValue, T: Reducible<V> + Cmov, F: Fn(&T) -> u64>(
    blocks: &mut [T],
    sort_keys: &mut Vec<u64>,
    key_of: F,
) -> crate::OramMetrics {
    let mut metrics = crate::OramMetrics::default();
    let n = blocks.len();
    sort_keys.resize(n, 0u64);
    for i in 0..n {
        sort_keys[i] = key_of(&blocks[i]);
    }
    {
        let _t = crate::timing_scope!(&mut metrics.merge_detail_sort);
        crate::oblivious::djbsort::sort_with_payload(sort_keys, blocks);
    }
    let reduce_metrics = crate::oblivious::reduction::reduce_equal_runs(blocks);
    metrics.merge_detail_reduce = reduce_metrics.merge_detail_reduce;
    #[cfg(feature = "profile")]
    {
        metrics.merge_accumulate = metrics.merge_detail_sort + metrics.merge_detail_reduce;
    }
    metrics
}

/// Sorts, reduces, and stably compacts blocks, marking excess slots as dummy.
/// Returns the number of real surviving elements (`reduced_len`).
pub fn sort_reduce_compact<V: OramValue, T: Reducible<V> + Cmov, F: Fn(&T) -> u64>(
    blocks: &mut [T],
    sort_keys: &mut Vec<u64>,
    compact_marks_buf: &mut Vec<usize>,
    key_of: F,
) -> usize {
    let n = blocks.len();
    let _metrics = sort_reduce(blocks, sort_keys, key_of);
    compact_marks(blocks, compact_marks_buf);
    let reduced_len = compact_marks_buf[n];
    compact_payload(blocks, compact_marks_buf);
    for i in 0..n {
        let keep_real = crate::ct::ct_lt(i as u64, reduced_len as u64);
        blocks[i].conditional_dummy(crate::ct::ct_not(keep_real));
    }
    reduced_len
}
