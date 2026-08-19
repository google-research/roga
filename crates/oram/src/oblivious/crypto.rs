// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory of this source tree.
// You may select, at your option, one of the above-listed licenses.

//! Cryptographic primitives for oblivious algorithms.

use aes::cipher::generic_array::GenericArray;
use aes::cipher::BlockEncrypt;
use aes::Aes128;

/// Derives a `K`-byte routing tag from `key` using a keyed PRF (AES-based CBC-MAC).
///
/// The input key is processed in 16-byte blocks. If the key length is not a multiple
/// of 16, the last block is zero-padded. Empty keys are processed as a single zero block.
pub fn prf_tag(prf: &Aes128, key: &[u8]) -> u64 {
    let mut state = GenericArray::clone_from_slice(&[0u8; 16]);
    let (chunks, remainder) = key.as_chunks::<16>();

    for chunk in chunks {
        for j in 0..16 {
            state[j] ^= chunk[j];
        }
        prf.encrypt_block(&mut state);
    }

    if !remainder.is_empty() || key.is_empty() {
        let mut last_block = [0u8; 16];
        last_block[..remainder.len()].copy_from_slice(remainder);
        for j in 0..16 {
            state[j] ^= last_block[j];
        }
        prf.encrypt_block(&mut state);
    }

    let mut buf = [0u8; 8];
    buf.copy_from_slice(&state[..8]);
    u64::from_le_bytes(buf)
}
