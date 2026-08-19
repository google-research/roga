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
