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

//! Routing functions and key layout helpers for sharded ORAM.

use crate::block::OramBlock;
use crate::oblivious::crypto::prf_tag;
use crate::oblivious_histogram::routing::OramAddress;
use crate::{ct, OramValue};
use aes::Aes128;

pub(crate) use crate::oblivious::binomial_solver::suggested_per_shard_quota;

/// Extracts the destination physical ORAM shard index for a given routing tag.
#[inline]
pub(crate) fn shard_index_for_tag(tag: u64, shard_count: usize) -> usize {
    let shard_raw = tag >> 32;
    (shard_raw as usize) & (shard_count - 1)
}

/// Prepares the logical key: hashes it with the PRF to obtain the routing tag,
/// and computes the destination physical ORAM shard.
pub fn prepare_key(prf: &Aes128, key: &[u8], shard_count: usize) -> (usize, u64) {
    let tag = prf_tag(prf, key);
    let shard = shard_index_for_tag(tag, shard_count);
    (shard, tag)
}

/// Counts the active reduced records assigned to each shard with a fixed memory trace.
///
/// The full public-length block slice is scanned, and every shard counter is touched for
/// every block. `reduced_len` may therefore remain secret without affecting loop bounds or
/// the sequence of counter addresses accessed.
#[doc(hidden)]
#[inline(always)]
pub fn count_shard_loads_oblivious<const K: usize, V: OramValue>(
    counts: &mut [u64],
    blocks: &[OramBlock<K, V>],
    reduced_len: usize,
) {
    counts.fill(0);
    for (i, block) in blocks.iter().enumerate() {
        let tag = block.tag;
        let active = ct::ct_lt(i as u64, reduced_len as u64) & tag.ct_is_real();
        let shard = shard_index_for_tag(tag, counts.len());
        for (candidate, count) in counts.iter_mut().enumerate() {
            let should_increment = active & ct::ct_eq(shard as u64, candidate as u64);
            *count = count.wrapping_add(should_increment as u64);
        }
    }
}

/// Generates marks for oblivious distribution back to their original slots.
pub(crate) fn build_distribute_marks_router(
    marks: &mut Vec<usize>,
    counts: &[u64],
    per_shard_quota: usize,
    len: usize,
) {
    if marks.len() != len + 1 {
        marks.resize(len + 1, 0);
    }
    marks[0] = 0;

    let mut prefix_sum = 0usize;
    let mut idx = 1;
    for &count in counts {
        for slot in 0..per_shard_quota {
            prefix_sum += ct::ct_lt(slot as u64, count) as usize;
            marks[idx] = prefix_sum;
            idx += 1;
        }
    }
    debug_assert_eq!(idx, len + 1);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_for_shard(shard: usize) -> OramBlock<16> {
        OramBlock::real(((shard as u64) << 32) | 1, 0, 1, [0u8; 16])
    }

    #[test]
    fn oblivious_counts_scan_only_the_active_prefix() {
        let blocks = [
            block_for_shard(2),
            block_for_shard(0),
            block_for_shard(2),
            block_for_shard(1),
            block_for_shard(3),
        ];
        let mut counts = [99u64; 4];

        count_shard_loads_oblivious(&mut counts, &blocks, 3);

        assert_eq!(counts, [1, 0, 2, 0]);
    }

    #[test]
    fn distribute_marks_use_fixed_per_shard_slots() {
        let mut marks = Vec::new();

        build_distribute_marks_router(&mut marks, &[2, 1], 3, 6);

        assert_eq!(marks, [0, 1, 2, 2, 3, 3, 3]);
    }
}
