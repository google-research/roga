// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Oblivious stash of blocks.

use super::routing::OramAddress;
use super::tree::{CompleteBinaryTreeIndex, TreeIndex};
use crate::oblivious::compaction::{compact_marks, compact_payload, distribute_payload};
use crate::oblivious::ct::Cmov as CtCmov;
use crate::OramBlock;
use crate::{ct, OramMetrics, OramValue, StashSize};
use cmov::Cmov;

/// Oblivious stash buffer holding both path buckets ($(L+1)Z$ slots) and overflow storage ($S$ slots).
///
/// Invariant: Total length is always `path_size + S`, and memory operations are constant-time.
#[derive(Debug, Clone)]
pub struct ObliviousStash<const Z: usize = 16, const K: usize = 16, const S: usize = 64, V = u64> {
    /// Contiguous buffer of path and overflow blocks.
    pub blocks: Vec<OramBlock<K, V>>,
    pub(crate) path_size: usize,
    pub(crate) sort_keys: Vec<u64>,
    pub(crate) insert_scratch: Vec<OramBlock<K, V>>,
    pub(crate) compact_marks: Vec<usize>,
    pub(crate) distribute_marks: Vec<usize>,
}

impl<const Z: usize, const K: usize, const S: usize, V: OramValue> ObliviousStash<Z, K, S, V> {
    /// Allocates an oblivious stash for a tree path buffer of `path_size` plus $S$ overflow slots ($O(L \cdot Z + S)$).
    pub fn new(path_size: StashSize) -> Self {
        let path_size = path_size as usize;
        let len = path_size + S;
        Self {
            blocks: vec![OramBlock::<K, V>::dummy(); len],
            path_size,
            sort_keys: vec![0u64; len],
            insert_scratch: Vec::new(),
            compact_marks: Vec::with_capacity(len + 1),
            distribute_marks: Vec::with_capacity(len + 1),
        }
    }

    #[inline]
    fn identity_eq(block: &OramBlock<K, V>, target: &[u8; K]) -> u8 {
        crate::oblivious::ct::ct_eq_bytes(&block.payload, target)
    }

    /// Computes the deepest common ancestor level along the eviction path to `target_leaf` in constant time ($O(1)$).
    pub fn deepest_shared_level(
        target_leaf: TreeIndex,
        block_tag: u64,
        height: u64,
        block_is_dummy: u8,
    ) -> TreeIndex {
        let arbitrary_leaf: TreeIndex = 1 << height;
        let mut block_leaf = block_tag.to_leaf(height);
        block_leaf.cmovnz(&arbitrary_leaf, block_is_dummy);

        let differing_bits = target_leaf ^ block_leaf;
        let shared_prefix_bits = differing_bits.leading_zeros() as u64;
        let raw_level = (height as i64) + (shared_prefix_bits as i64) - 63;
        let mask = raw_level >> 63;
        let level = (raw_level as u64) & !(mask as u64);
        level.saturating_sub(1)
    }

    /// Obliviously writes blocks from the stash back to the eviction path towards `position` ($O(Z \cdot L + S)$).
    #[allow(unused_mut)]
    pub fn write_to_path(
        &mut self,
        physical_memory: &mut [OramBlock<K, V>],
        position: TreeIndex,
    ) -> OramMetrics {
        let mut metrics = OramMetrics::default();

        let height = position.ct_depth();
        let mut level_counts = [0u64; 32];
        let mut overflow_count = 0usize;

        let z_u64 = Z as u64;
        let n_levels = (height + 1) as usize;

        {
            let _t2 = timing_scope!(&mut metrics.write_to_path_assign_real);
            for block in &self.blocks {
                let block_is_real = block.tag.ct_is_real();
                let block_is_dummy = ct::ct_not(block_is_real);
                let block_level =
                    Self::deepest_shared_level(position, block.tag, height, block_is_dummy);

                let mut assigned: u8 = 0;
                for level in (0..n_levels).rev() {
                    let level_u64 = level as u64;
                    let legal_level = ct::ct_lt(level_u64, block_level + 1);
                    let bucket_not_full = ct::ct_lt(level_counts[level], z_u64);

                    let should_assign =
                        legal_level & bucket_not_full & ct::ct_not(assigned) & block_is_real;
                    level_counts[level] += should_assign as u64;
                    assigned |= should_assign;
                }

                let should_overflow = block_is_real & ct::ct_not(assigned);
                overflow_count += should_overflow as usize;
            }
        }

        let overflow_capacity = self.blocks.len() - self.path_size;
        if overflow_count >= overflow_capacity {
            panic!(
                "stash overflow after write_to_path: {overflow_count} overflow blocks need capacity below {overflow_capacity} to preserve the sentinel slot. blocks.len={}, path_size={}",
                self.blocks.len(), self.path_size
            );
        }

        {
            let _t3 = timing_scope!(&mut metrics.write_to_path_compact);
            compact_marks(&self.blocks, &mut self.compact_marks);

            build_leaf_to_root_layout_marks::<Z>(
                &mut self.distribute_marks,
                &level_counts[..n_levels],
                overflow_count,
                self.path_size,
                self.blocks.len(),
            );

            compact_payload(&mut self.blocks, &self.compact_marks);
            distribute_payload(&mut self.blocks, &self.distribute_marks);
        }

        {
            let _t4 = timing_scope!(&mut metrics.write_to_path_copy_back);
            for depth in 0..=height {
                let dst_idx = position.ct_node_on_path(depth, height) as usize;
                let src_start = (height - depth) as usize * Z;
                let dst_start = dst_idx * Z;
                physical_memory[dst_start..dst_start + Z]
                    .copy_from_slice(&self.blocks[src_start..src_start + Z]);
            }
            self.blocks[..self.path_size].fill(OramBlock::dummy());
        }
        #[cfg(feature = "profile")]
        {
            metrics.write_to_path = metrics.write_to_path_setup
                + metrics.write_to_path_assign_real
                + metrics.write_to_path_compact
                + metrics.write_to_path_copy_back;
        }
        metrics
    }

    /// Scans the stash, extracts the value matching `payload`, and marks the block dummy in constant time ($O(M)$).
    pub fn read_and_remove(&mut self, payload: &[u8; K]) -> (V, [u8; K]) {
        let mut result_value = V::default();
        let mut result_payload = [0u8; K];
        for block in &mut self.blocks {
            let is_target = Self::identity_eq(block, payload) & block.tag.ct_is_real();
            let value = block.value;
            result_value.cmovnz(&value, is_target);
            result_payload.cmov(&block.payload, is_target != 0);
            block.conditional_dummy(is_target);
        }
        (result_value, result_payload)
    }

    /// Obliviously inserts a new block into the first available overflow slot ($O(S)$).
    pub fn insert(&mut self, tag: u64, epoch: u8, value: V, payload: &[u8; K]) {
        let mut placed: u8 = 0;
        for block in &mut self.blocks[self.path_size..] {
            let is_dummy = ct::ct_not(block.tag.ct_is_real());
            let put = is_dummy & ct::ct_not(placed);
            block.assign_if(tag, epoch, value, payload, put);
            placed |= put;
        }
        if placed == 0 {
            panic!("stash overflow on buffered insert: no free overflow slot");
        }
    }

    /// Inserts a fixed window of prepared updates by compacting the existing overflow buffer ($O(S + B)$).
    pub(crate) fn insert_batch<G>(&mut self, count: usize, mut update_at: G)
    where
        G: FnMut(usize) -> (u64, u8, V, [u8; K], u8),
    {
        let overflow_len = self.blocks.len() - self.path_size;
        let work_len = overflow_len + count;
        self.insert_scratch.resize(work_len, OramBlock::<K, V>::dummy());
        self.insert_scratch[..overflow_len].copy_from_slice(&self.blocks[self.path_size..]);
        self.insert_scratch[overflow_len..work_len].fill(OramBlock::dummy());

        for i in 0..count {
            let (tag, epoch, value, payload, is_real_update) = update_at(i);
            let is_real_update = is_real_update & 1;
            let block = &mut self.insert_scratch[overflow_len + i];
            block.assign_if(tag, epoch, value, &payload, is_real_update);
        }

        self.compact_marks.resize(work_len + 1, 0);
        self.compact_marks[0] = 0;
        for i in 0..work_len {
            self.compact_marks[i + 1] =
                self.compact_marks[i] + self.insert_scratch[i].tag.ct_is_real() as usize;
        }
        let real_len = self.compact_marks[work_len];
        if real_len > overflow_len {
            panic!(
                "stash overflow on buffered batch insert: {real_len} overflow blocks need capacity at most {overflow_len}"
            );
        }

        compact_payload(&mut self.insert_scratch, &self.compact_marks);
        for i in 0..work_len {
            let keep_real = ct::ct_lt(i as u64, real_len as u64);
            self.insert_scratch[i].conditional_dummy(ct::ct_not(keep_real));
        }
        self.blocks[self.path_size..].copy_from_slice(&self.insert_scratch[..overflow_len]);
    }

    /// Extends the stash path buffer by $Z$ slots when the tree height grows ($O(S)$).
    pub fn grow_extend_path_buffer(&mut self) {
        self.blocks.reserve(Z);
        self.blocks.splice(self.path_size..self.path_size, [OramBlock::<K, V>::dummy(); Z]);
        self.path_size += Z;
        let new_len = self.blocks.len();
        self.sort_keys.resize(new_len, 0u64);
        self.compact_marks.reserve(Z);
        self.distribute_marks.reserve(Z);
    }

    /// Computes the number of real blocks currently occupying the overflow buffer ($O(S)$).
    pub fn occupancy(&self) -> StashSize {
        self.blocks[self.path_size..].iter().map(|block| block.tag.ct_is_real() as StashSize).sum()
    }

    /// Counts real blocks matching `target_leaf` at `height` and `target_epoch` in constant time ($O(M)$).
    pub fn count_matching_blocks(&self, height: u64, target_leaf: TreeIndex, target_epoch: u8) -> u64 {
        let mut result = 0u64;
        for block in &self.blocks {
            let is_real = block.tag.ct_is_real();
            let leaf_match = ct::ct_eq(block.tag.to_leaf(height), target_leaf);
            let epoch_match = ct::ct_eq(block.epoch as u64, target_epoch as u64);
            let matches = is_real & leaf_match & epoch_match;
            result += matches as u64;
        }
        result
    }

    /// Counts real blocks satisfying predicate `pred` ($O(M)$).
    pub fn count_blocks_with<F: Fn(u64, u8) -> bool>(&self, pred: F) -> u64 {
        let mut result = 0u64;
        for block in &self.blocks {
            result += (block.tag.ct_is_real() != 0 && pred(block.tag, block.epoch)) as u64;
        }
        result
    }

    /// Reads all buckets along the path to `position` from physical memory into the path buffer ($O(L \cdot Z)$).
    pub fn read_from_path(&mut self, physical_memory: &[OramBlock<K, V>], position: TreeIndex) {
        let height = position.ct_depth();

        for depth in (0..(self.path_size / Z)).rev() {
            let bucket_index = position.ct_node_on_path(depth as u64, height);
            let dst_start = depth * Z;
            let src_start = (bucket_index as usize) * Z;
            self.blocks[dst_start..dst_start + Z]
                .copy_from_slice(&physical_memory[src_start..src_start + Z]);
        }
    }

    /// Reads a sibling bucket into scratch memory during lazy migration sweep ($O(Z)$).
    pub fn read_sibling_bucket(
        &mut self,
        physical_memory: &[OramBlock<K, V>],
        sibling_leaf: TreeIndex,
    ) {
        let sibling_start = (sibling_leaf as usize) * Z;
        if self.insert_scratch.len() < Z {
            self.insert_scratch.resize(Z, OramBlock::dummy());
        }
        self.insert_scratch[..Z]
            .copy_from_slice(&physical_memory[sibling_start..sibling_start + Z]);
    }

    /// Obliviously sorts and deduplicates stash blocks along the path to `position` ($O(M \log^2 M)$).
    pub fn merge_accumulate_for_path(&mut self, position: TreeIndex) -> OramMetrics {
        let height = position.ct_depth();
        crate::oblivious::compaction::sort_reduce(&mut self.blocks, &mut self.sort_keys, |block| {
            block.tag.path_rank_key(position, height)
        })
    }
}

pub(crate) fn build_leaf_to_root_layout_marks<const Z: usize>(
    marks: &mut Vec<usize>,
    level_counts: &[u64],
    overflow_count: usize,
    path_size: usize,
    len: usize,
) {
    marks.resize(len + 1, 0);
    marks[0] = 0;

    let mut prefix_sum = 0usize;
    let mut idx = 1;
    for &count in level_counts.iter().rev() {
        let count = count as usize;
        for slot in 0..Z {
            prefix_sum += crate::ct::ct_lt(slot as u64, count as u64) as usize;
            marks[idx] = prefix_sum;
            idx += 1;
        }
    }

    debug_assert_eq!(idx, path_size + 1);
    let overflow_capacity = len - path_size;
    for slot in 0..overflow_capacity {
        prefix_sum += crate::ct::ct_lt(slot as u64, overflow_count as u64) as usize;
        marks[idx] = prefix_sum;
        idx += 1;
    }
    debug_assert_eq!(idx, len + 1);
}
