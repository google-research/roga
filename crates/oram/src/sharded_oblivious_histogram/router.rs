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

use crate::oblivious::crypto::prf_tag;
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

/// Generates marks for oblivious distribution back to their original slots.
pub(crate) fn build_distribute_marks_router(
    marks: &mut Vec<usize>,
    counts: &[u64],
    per_shard_quota: usize,
    len: usize,
) {
    marks.resize(len + 1, 0);
    marks[0] = 0;

    let mut prefix_sum = 0usize;
    let mut idx = 1;
    for &count in counts {
        let count = count as usize;
        for slot in 0..per_shard_quota {
            prefix_sum += usize::from(slot < count);
            marks[idx] = prefix_sum;
            idx += 1;
        }
    }
    debug_assert_eq!(idx, len + 1);
}
