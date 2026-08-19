// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

pub mod binomial_solver;
pub mod compaction;
pub mod crypto;
pub mod ct;
pub mod djbsort;
pub mod reduction;

/// Copies up to `N` bytes from `src` into a fresh zero-padded `[u8; N]`.
pub fn copy_prefix<const N: usize>(src: &[u8]) -> [u8; N] {
    let mut out = [0u8; N];
    for (dst, &s) in out.iter_mut().zip(src) {
        *dst = s;
    }
    out
}
