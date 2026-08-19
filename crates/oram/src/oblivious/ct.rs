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

// Constant-time scalar and SIMD helpers for sorting networks and ORAM path operations.

use cmov::{Cmov as ScalarCmov, CmovEq};

// Constant-Time Comparisons & Logic

/// Constant-time equality as `0` or `1`.
#[inline]
pub fn ct_eq<T: CmovEq>(a: T, b: T) -> u8 {
    let mut out = 0u8;
    a.cmoveq(&b, 1, &mut out);
    out
}

/// Constant-time byte array equality as `0` or `1`.
#[inline]
pub fn ct_eq_bytes<const N: usize>(a: &[u8; N], b: &[u8; N]) -> u8 {
    if N == 16 {
        let a_val = u128::from_ne_bytes(a[..16].try_into().unwrap());
        let b_val = u128::from_ne_bytes(b[..16].try_into().unwrap());
        ct_eq(a_val, b_val)
    } else if N == 8 {
        let a_val = u64::from_ne_bytes(a[..8].try_into().unwrap());
        let b_val = u64::from_ne_bytes(b[..8].try_into().unwrap());
        ct_eq(a_val, b_val)
    } else {
        let mut acc = 0u8;
        for (&x, &y) in a.iter().zip(b.iter()) {
            acc |= x ^ y;
        }
        let acc32 = acc as u32;
        ((acc32.wrapping_sub(1) & !acc32) >> 31) as u8
    }
}

/// Constant-time unsigned less-than as `0` or `1`.
#[inline]
pub fn ct_lt(a: u64, b: u64) -> u8 {
    let diff = a.wrapping_sub(b);
    let borrow = (!a & b) | (!(a ^ b) & diff);
    ((borrow >> 63) & 1) as u8
}

/// Constant-time logical NOT for a value that is either 0 or 1.
#[inline]
pub fn ct_not(cond: u8) -> u8 {
    debug_assert!(cond == 0 || cond == 1);
    cond ^ 1
}

/// Constant-time minimum of two u64 values.
#[inline]
pub fn ct_min(a: u64, b: u64) -> u64 {
    use cmov::Cmov as _;
    let mut out = a;
    out.cmovnz(&b, ct_lt(b, a));
    out
}

/// Constant-time unsigned less-than for u8 as `0` or `1`.
#[inline]
pub fn ct_lt_u8(a: u8, b: u8) -> u8 {
    let diff = a.wrapping_sub(b);
    let borrow = (!a & b) | (!(a ^ b) & diff);
    ((borrow >> 7) & 1) as u8
}

/// Constant-time minimum of two u8 values.
#[inline]
pub fn ct_min_u8(a: u8, b: u8) -> u8 {
    use cmov::Cmov as _;
    let mut out = a;
    out.cmovnz(&b, ct_lt_u8(b, a));
    out
}

/// Constant-time unsigned less-than for u32 as `0` or `1`.
#[inline]
pub fn ct_lt_u32(a: u32, b: u32) -> u8 {
    let diff = a.wrapping_sub(b);
    let borrow = (!a & b) | (!(a ^ b) & diff);
    ((borrow >> 31) & 1) as u8
}

/// Constant-time minimum of two u32 values.
#[inline]
pub fn ct_min_u32(a: u32, b: u32) -> u32 {
    use cmov::Cmov as _;
    let mut out = a;
    out.cmovnz(&b, ct_lt_u32(b, a));
    out
}

/// Constant-time maximum of two u32 values.
#[inline]
pub fn ct_max_u32(a: u32, b: u32) -> u32 {
    use cmov::Cmov as _;
    let mut out = a;
    out.cmovnz(&b, ct_lt_u32(a, b));
    out
}

// Conditional Movement Traits & Scalar Swaps

/// A trait for conditionally moving values with constant memory trace.
pub trait Cmov: Copy {
    /// Conditionally move `other` into `self` based on `choice`.
    fn cmov(&mut self, other: &Self, choice: bool);

    /// Conditionally exchange `other` and `self` based on `choice`.
    #[inline]
    fn cxchg(&mut self, other: &mut Self, choice: bool) {
        let tmp = *self;
        self.cmov(other, choice);
        other.cmov(&tmp, choice);
    }
}

macro_rules! impl_cmov_primitive {
    ($($t:ty),*) => {
        $(
            impl Cmov for $t {
                #[inline]
                fn cmov(&mut self, other: &Self, choice: bool) {
                    ScalarCmov::cmovnz(self, other, choice as u8);
                }
            }
        )*
    };
}

impl_cmov_primitive!(u8, u16, u32, u64, u128, usize, i8, i16, i32, i64, i128, isize);

impl<T: Cmov, const N: usize> Cmov for [T; N] {
    #[inline]
    fn cmov(&mut self, other: &Self, choice: bool) {
        for i in 0..N {
            self[i].cmov(&other[i], choice);
        }
    }
}

/// Conditional swap: swap `a` and `b` if `cond != 0`.
#[inline]
pub fn ct_swap<T: Cmov>(a: &mut T, b: &mut T, cond: u8) {
    let choice = cond != 0;
    let tmp = *a;
    a.cmov(b, choice);
    b.cmov(&tmp, choice);
}

// High-Performance SIMD Slice Swapping

/// AVX2 / SSE / 64-bit chunked conditional swap for arbitrary types T.
#[inline(always)]
pub unsafe fn cswap_fast_ptr<T: Cmov>(ptr_i: *mut T, ptr_j: *mut T, choice: bool) {
    let size = std::mem::size_of::<T>();
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{__m128i, _mm_blendv_epi8, _mm_set1_epi8};
        use core::arch::x86_64::{__m256i, _mm256_blendv_epi8, _mm256_set1_epi8};

        if size >= 32 && size % 32 == 0 {
            let mask = _mm256_set1_epi8(-(choice as i8));
            let (pi, pj) = (ptr_i as *mut __m256i, ptr_j as *mut __m256i);
            for i in 0..size / 32 {
                let (va, vb) = (pi.add(i).read_unaligned(), pj.add(i).read_unaligned());
                pi.add(i).write_unaligned(_mm256_blendv_epi8(va, vb, mask));
                pj.add(i).write_unaligned(_mm256_blendv_epi8(vb, va, mask));
            }
            return;
        } else if size >= 16 && size % 16 == 0 {
            let mask = _mm_set1_epi8(-(choice as i8));
            let (pi, pj) = (ptr_i as *mut __m128i, ptr_j as *mut __m128i);
            for i in 0..size / 16 {
                let (va, vb) = (pi.add(i).read_unaligned(), pj.add(i).read_unaligned());
                pi.add(i).write_unaligned(_mm_blendv_epi8(va, vb, mask));
                pj.add(i).write_unaligned(_mm_blendv_epi8(vb, va, mask));
            }
            return;
        } else if size >= 8 && size % 8 == 0 {
            let (pi, pj) = (ptr_i as *mut u64, ptr_j as *mut u64);
            for i in 0..size / 8 {
                let (mut vi, mut vj) = (pi.add(i).read_unaligned(), pj.add(i).read_unaligned());
                let (tmp_i, tmp_j) = (vi, vj);
                vi.cmov(&tmp_j, choice);
                vj.cmov(&tmp_i, choice);
                pi.add(i).write_unaligned(vi);
                pj.add(i).write_unaligned(vj);
            }
            return;
        }
    }
    (*ptr_i).cxchg(&mut *ptr_j, choice);
}

/// A fast conditional exchange for element slices, using specialized AVX2 swap if possible.
#[inline(always)]
pub fn cswap_fast<T: Cmov>(arr: &mut [T], i: usize, j: usize, choice: bool) {
    debug_assert!(i < arr.len() && j < arr.len());
    unsafe {
        let ptr = arr.as_mut_ptr();
        cswap_fast_ptr(ptr.add(i), ptr.add(j), choice);
    }
}
