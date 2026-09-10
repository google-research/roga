// Based on djbsort by D. J. Bernstein (public domain).
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

//! Data-oblivious constant-time key-payload sorting network based on DJB's design.

use crate::oblivious::ct::{cswap_fast_ptr, Cmov};

#[inline(always)]
unsafe fn minmax_by_raw<P: Cmov, F: Fn(&P, &P) -> u8>(
    payloads_ptr: *mut P,
    i: usize,
    j: usize,
    greater: &F,
) {
    let a_ptr = payloads_ptr.add(i);
    let b_ptr = payloads_ptr.add(j);
    let should_swap = greater(&*a_ptr, &*b_ptr) != 0;
    cswap_fast_ptr(a_ptr, b_ptr, should_swap);
}

#[inline(always)]
unsafe fn cascade_by_raw<P: Cmov, F: Fn(&P, &P) -> u8>(
    payloads_ptr: *mut P,
    j: usize,
    p: usize,
    q: usize,
    greater: &F,
) {
    let mut a = *payloads_ptr.add(j + p);

    let mut r = q;
    while r > p {
        let b_ptr = payloads_ptr.add(j + r);
        let should_swap = greater(&a, &*b_ptr) != 0;
        cswap_fast_ptr(&mut a, b_ptr, should_swap);
        r >>= 1;
    }

    *payloads_ptr.add(j + p) = a;
}

/// Sorts `payloads` with a fixed sorting network and a constant-time comparator.
///
/// `greater(a, b)` must return either `0` or `1`, must not branch on secret data,
/// and must not perform secret-dependent memory accesses. The sorting network's
/// sequence of comparisons depends only on `payloads.len()`.
pub fn sort_by<P: Cmov, F: Fn(&P, &P) -> u8>(payloads: &mut [P], greater: F) {
    let n = payloads.len();
    if n < 2 {
        return;
    }

    let payloads_ptr = payloads.as_mut_ptr();
    let mut top: usize = 1;
    while top < n - top {
        top += top;
    }

    let mut p = top;
    while p >= 1 {
        let mut i: usize = 0;
        while i + 2 * p <= n {
            let mut k: usize = 0;
            while k < p {
                unsafe {
                    minmax_by_raw(payloads_ptr, i + k, i + k + p, &greater);
                }
                k += 1;
            }
            i += 2 * p;
        }

        let mut j = i;
        while j + p < n {
            unsafe {
                minmax_by_raw(payloads_ptr, j, j + p, &greater);
            }
            j += 1;
        }

        i = 0;
        j = 0;
        let mut q = top;
        while q > p {
            if j != i {
                loop {
                    if j + q == n {
                        q >>= 1;
                        continue;
                    }
                    unsafe {
                        cascade_by_raw(payloads_ptr, j, p, q, &greater);
                    }
                    j += 1;
                    if j == i + p {
                        i += 2 * p;
                        break;
                    }
                }
                if q <= p {
                    break;
                }
            }

            while i + p + q <= n {
                let mut k: usize = 0;
                while k < p {
                    unsafe {
                        cascade_by_raw(payloads_ptr, i + k, p, q, &greater);
                    }
                    k += 1;
                }
                i += 2 * p;
            }

            j = i;
            while j + q < n {
                unsafe {
                    cascade_by_raw(payloads_ptr, j, p, q, &greater);
                }
                j += 1;
            }

            q >>= 1;
        }

        p >>= 1;
    }
}

#[inline(always)]
fn concat_wire(
    first_start: usize,
    first_stride: usize,
    first_len: usize,
    second_start: usize,
    second_stride: usize,
    index: usize,
) -> usize {
    if index < first_len {
        first_start + index * first_stride
    } else {
        second_start + (index - first_len) * second_stride
    }
}

#[inline]
unsafe fn odd_even_merge_by_raw<P: Cmov, F: Fn(&P, &P) -> u8>(
    payloads_ptr: *mut P,
    first_start: usize,
    first_stride: usize,
    first_len: usize,
    second_start: usize,
    second_stride: usize,
    second_len: usize,
    greater: &F,
) {
    if first_len == 0 || second_len == 0 {
        return;
    }
    if first_len == 1 && second_len == 1 {
        minmax_by_raw(payloads_ptr, first_start, second_start, greater);
        return;
    }

    let first_odd_len = first_len.div_ceil(2);
    let second_odd_len = second_len.div_ceil(2);
    let first_even_len = first_len / 2;
    let second_even_len = second_len / 2;

    odd_even_merge_by_raw(
        payloads_ptr,
        first_start,
        first_stride * 2,
        first_odd_len,
        second_start,
        second_stride * 2,
        second_odd_len,
        greater,
    );
    odd_even_merge_by_raw(
        payloads_ptr,
        first_start + first_stride,
        first_stride * 2,
        first_even_len,
        second_start + second_stride,
        second_stride * 2,
        second_even_len,
        greater,
    );

    let odd_len = first_odd_len + second_odd_len;
    let even_len = first_even_len + second_even_len;
    for i in 0..even_len.min(odd_len.saturating_sub(1)) {
        let even_wire = concat_wire(
            first_start + first_stride,
            first_stride * 2,
            first_even_len,
            second_start + second_stride,
            second_stride * 2,
            i,
        );
        let next_odd_wire = concat_wire(
            first_start,
            first_stride * 2,
            first_odd_len,
            second_start,
            second_stride * 2,
            i + 1,
        );
        minmax_by_raw(payloads_ptr, even_wire, next_odd_wire, greater);
    }
}

/// Merges two adjacent ascending sorted runs with a fixed comparison network.
///
/// `split` is the public length of the first run. The sequence of comparisons
/// and swaps depends only on `payloads.len()` and `split`, never on record data.
/// Both lengths must be powers of two, and the first must be at least as long
/// as the second. These are exactly the run shapes used by tree-aware readout.
pub fn merge_sorted_by<P: Cmov, F: Fn(&P, &P) -> u8>(
    payloads: &mut [P],
    split: usize,
    greater: F,
) {
    assert!(split <= payloads.len());
    let second_len = payloads.len() - split;
    if split == 0 || second_len == 0 {
        return;
    }
    assert!(split.is_power_of_two());
    assert!(second_len.is_power_of_two());
    assert!(split >= second_len);

    unsafe {
        odd_even_merge_by_raw(
            payloads.as_mut_ptr(),
            0,
            1,
            split,
            split,
            1,
            second_len,
            &greater,
        );
    }
}

#[inline(always)]
unsafe fn minmax_with_payload_raw<P: Cmov>(
    keys_ptr: *mut u64,
    payloads_ptr: *mut P,
    i: usize,
    j: usize,
) {
    let key_a_ptr = keys_ptr.add(i);
    let key_b_ptr = keys_ptr.add(j);
    let pay_a_ptr = payloads_ptr.add(i);
    let pay_b_ptr = payloads_ptr.add(j);

    let should_swap = *key_a_ptr > *key_b_ptr;

    cswap_fast_ptr(key_a_ptr, key_b_ptr, should_swap);
    cswap_fast_ptr(pay_a_ptr, pay_b_ptr, should_swap);
}

#[inline(always)]
unsafe fn cascade_with_payload_raw<P: Cmov>(
    keys_ptr: *mut u64,
    payloads_ptr: *mut P,
    j: usize,
    p: usize,
    q: usize,
) {
    let mut a_key = *keys_ptr.add(j + p);
    let mut a_pay = *payloads_ptr.add(j + p);

    let mut r = q;
    while r > p {
        let b_key_ptr = keys_ptr.add(j + r);
        let b_pay_ptr = payloads_ptr.add(j + r);

        let should_swap = a_key > *b_key_ptr;

        cswap_fast_ptr(&mut a_key, b_key_ptr, should_swap);
        cswap_fast_ptr(&mut a_pay, b_pay_ptr, should_swap);

        r >>= 1;
    }

    *keys_ptr.add(j + p) = a_key;
    *payloads_ptr.add(j + p) = a_pay;
}

pub fn sort_with_payload<P: Cmov>(keys: &mut [u64], payloads: &mut [P]) {
    let n = keys.len();
    if n < 2 {
        return;
    }
    assert_eq!(keys.len(), payloads.len());

    let keys_ptr = keys.as_mut_ptr();
    let payloads_ptr = payloads.as_mut_ptr();

    let mut top: usize = 1;
    while top < n - top {
        top += top;
    }

    let mut p = top;
    while p >= 1 {
        let mut i: usize = 0;
        while i + 2 * p <= n {
            let mut k: usize = 0;
            while k < p {
                unsafe {
                    minmax_with_payload_raw(keys_ptr, payloads_ptr, i + k, i + k + p);
                }
                k += 1;
            }
            i += 2 * p;
        }

        let mut j = i;
        while j + p < n {
            unsafe {
                minmax_with_payload_raw(keys_ptr, payloads_ptr, j, j + p);
            }
            j += 1;
        }

        i = 0;
        j = 0;
        let mut q = top;
        while q > p {
            if j != i {
                loop {
                    if j + q == n {
                        q >>= 1;
                        continue;
                    }
                    unsafe {
                        cascade_with_payload_raw(keys_ptr, payloads_ptr, j, p, q);
                    }
                    j += 1;
                    if j == i + p {
                        i += 2 * p;
                        break;
                    }
                }
                if q <= p {
                    break;
                }
            }

            while i + p + q <= n {
                let mut k: usize = 0;
                while k < p {
                    unsafe {
                        cascade_with_payload_raw(keys_ptr, payloads_ptr, i + k, p, q);
                    }
                    k += 1;
                }
                i += 2 * p;
            }

            j = i;
            while j + q < n {
                unsafe {
                    cascade_with_payload_raw(keys_ptr, payloads_ptr, j, p, q);
                }
                j += 1;
            }

            q >>= 1;
        }

        p >>= 1;
    }
}

#[cfg(test)]
mod tests {
    use super::merge_sorted_by;

    #[test]
    fn odd_even_merge_handles_power_of_two_runs_with_unequal_lengths() {
        for left_exp in 0..7 {
            for right_exp in 0..=left_exp {
                let left_len = 1usize << left_exp;
                let right_len = 1usize << right_exp;
                let mut values: Vec<u64> = (0..left_len as u64).map(|x| x * 3).collect();
                values.extend((0..right_len as u64).map(|x| x * 2 + 1));

                merge_sorted_by(&mut values, left_len, |a, b| u8::from(a > b));

                assert!(
                    values.windows(2).all(|pair| pair[0] <= pair[1]),
                    "failed for left={left_len}, right={right_len}: {values:?}"
                );
            }
        }
    }
}
