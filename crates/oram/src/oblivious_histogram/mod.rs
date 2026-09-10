// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Oblivious Histogram: type definition, construction, accessors, and export.
//!
//! The operation pipeline (insert/read/evict) lives in `ops`, and all
//! resize logic (decision + mechanism + DP math) lives in `resize`.

// --- Submodules ---
pub(crate) mod ops;
pub(crate) mod resize;
pub(crate) mod routing;
pub(crate) mod stash;
pub(crate) mod tree;

// --- Imports ---
use std::fmt;

use aes::cipher::generic_array::GenericArray;
use aes::cipher::KeyInit;
use aes::Aes128;
use cmov::Cmov;
use rand::{CryptoRng, Rng};
use rayon::prelude::*;

use crate::metrics::OramMetrics;
use crate::{Address, OramValue, StashSize};

use crate::OramBlock;
pub use resize::AutoResizeConfig;
use resize::AutoResizeState;
use routing::OramAddress;
use stash::ObliviousStash;

/// Data-oblivious keyed histogram backed by a PRF-routed Path ORAM binary tree.
///
/// Parameters:
/// - `Z`: Bucket capacity (blocks per tree node, default 16).
/// - `K`: Key byte length (default 16).
/// - `A`: Eviction interval (inserts between path evictions, default 20).
/// - `S`: Stash overflow capacity bound (default 64).
/// - `V`: Aggregated payload value type (default `u64`).
///
/// Invariants:
/// - Deterministic path eviction occurs every `A` operations using bit-reversed leaf sequence.
/// - Memory access trace is data-independent (constant-time).
#[repr(align(64))]
#[derive(Clone)]
pub struct ObliviousHistogram<const Z: usize = 16, const K: usize = 16, const A: usize = 20, const S: usize = 64, V = u64> {
    pub(crate) physical_memory: Vec<OramBlock<K, V>>,
    pub(crate) stash: ObliviousStash<Z, K, S, V>,
    pub(crate) height: u64,
    pub(crate) epoch: u8,
    pub(crate) prf: Aes128,
    pub(crate) append_ctr: u64,
    pub(crate) evict_ctr: u64,
    pub(crate) sweep_end: u64,
    pub(crate) auto_resize: Option<AutoResizeState>,
    pub(crate) filler_ctr: u64,
    /// Accumulated runtime diagnostic metrics.
    pub metrics: OramMetrics,
}

/// Indicates that a statistically-sized subtree inbox was too small.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SubtreeReadoutOverflow {
    pub max_load: usize,
    pub inbox_len: usize,
    pub subtree_count: usize,
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: fmt::Debug> fmt::Debug for ObliviousHistogram<Z, K, A, S, V> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ObliviousHistogram")
            .field("physical_memory", &self.physical_memory)
            .field("stash", &self.stash)
            .field("height", &self.height)
            .field("epoch", &self.epoch)
            .field("append_ctr", &self.append_ctr)
            .field("evict_ctr", &self.evict_ctr)
            .field("sweep_end", &self.sweep_end)
            .field("auto_resize", &self.auto_resize)
            .field("filler_ctr", &self.filler_ctr)
            .finish_non_exhaustive()
    }
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue> ObliviousHistogram<Z, K, A, S, V> {
    /// Constructs a keyed ORAM histogram with a fresh PRF key for the given capacity ($O(N)$).
    pub fn new<R: Rng + CryptoRng>(block_capacity: Address, rng: &mut R) -> Self {
        let mut raw_key = [0u8; 16];
        rng.fill_bytes(&mut raw_key);
        let prf = Aes128::new(GenericArray::from_slice(&raw_key));
        let height = tree::tree_height_for_capacity(block_capacity, Z);
        let path_size = (Z as u64) * (height + 1);
        let stash = ObliviousStash::<Z, K, S, V>::new(path_size);
        let tree_len = (1usize << (height + 1)) * Z;
        // Keep the public overflow-stash length as spare capacity so consuming
        // final readout can append it without reallocating the entire tree.
        let mut physical_memory = Vec::with_capacity(tree_len + S);
        physical_memory.resize(tree_len, OramBlock::<K, V>::dummy());

        Self {
            physical_memory,
            stash,
            height,
            epoch: 0u8,
            prf,
            append_ctr: 0,
            evict_ctr: 0,
            sweep_end: 0,
            auto_resize: None,
            filler_ctr: 0,
            metrics: OramMetrics::default(),
        }
    }

    /// Returns the accumulated timing metrics ($O(1)$).
    pub fn metrics(&self) -> OramMetrics {
        self.metrics
    }

    /// Resets the accumulated timing metrics to zero ($O(1)$).
    pub fn reset_metrics(&mut self) {
        self.metrics = OramMetrics::default();
    }

    /// Returns the peak overflow stash occupancy observed across operations ($O(1)$).
    pub fn peak_overflow(&self) -> StashSize {
        self.metrics.peak_overflow
    }

    /// Returns the total physical size of the binary tree buckets in bytes ($O(1)$).
    pub fn size_in_bytes(&self) -> u64 {
        self.physical_memory.len() as u64 * (std::mem::size_of::<OramBlock<K, V>>() as u64)
    }

    /// Returns the height of the ORAM binary tree ($O(1)$).
    pub fn height(&self) -> u64 {
        self.height
    }

    /// Returns the current physical block capacity ($O(1)$).
    pub fn capacity(&self) -> Address {
        (1 << self.height) * (Z as Address)
    }

    /// Returns the current number of real blocks in the overflow stash ($O(S)$).
    pub fn stash_occupancy(&self) -> StashSize {
        self.stash.occupancy()
    }

    /// Returns whether the lazy post-resize activation sweep is currently active.
    pub fn sweep_active(&self) -> bool {
        self.evict_ctr < self.sweep_end
    }

    /// For benchmarking: sets the lazy sweep migration window active state ($O(1)$).
    pub fn set_sweep_window_active(&mut self, active: bool) {
        if active {
            self.sweep_end = u64::MAX;
        } else {
            self.sweep_end = 0;
        }
    }

    /// Returns the public, fixed number of slots emitted by [`Self::readout`] ($O(1)$).
    pub fn readout_len(&self) -> usize {
        self.physical_memory.len() + (self.stash.blocks.len() - self.stash.path_size)
    }

    /// Performs the scheduled bulk readout described in the paper ($O(N \log^2 N)$).
    ///
    /// The method scans every tree and overflow-stash slot, sorts blocks with a
    /// fixed sorting network, merges equal-key runs, and obliviously compacts all
    /// surviving records to the front. It always returns [`Self::readout_len`]
    /// blocks, with dummy padding (`tag == 0`) after the real prefix.
    ///
    /// This is the data-structure layer of the protocol. A CVM integration must
    /// encrypt the complete returned array, including its dummy suffix, before
    /// it leaves trusted execution. Filtering dummies inside the CVM would reveal
    /// the number of distinct keys.
    pub fn readout(&self) -> Vec<OramBlock<K, V>> {
        let overflow = &self.stash.blocks[self.stash.path_size..];
        let mut blocks = Vec::with_capacity(self.physical_memory.len() + overflow.len());
        blocks.extend_from_slice(&self.physical_memory);
        blocks.extend_from_slice(overflow);
        oblivious_readout(&mut blocks);
        blocks
    }

    /// Performs an exact tree-aware bulk readout in $O(N \log N)$ work.
    ///
    /// The path invariant implies that every depth is already partitioned by a
    /// public routing prefix and contains at most one copy of any logical key.
    /// This method sorts only individual buckets, turns every depth into one
    /// sorted run, and obliviously merges those runs with the overflow stash.
    /// Equal-key runs are then reduced exactly using the full logical key.
    ///
    /// The returned array has the same public fixed length as [`Self::readout`],
    /// but dummy slots may be interspersed because final compaction is unnecessary
    /// when the encrypted recipient discards all dummies rather than only a suffix.
    pub fn readout_tree_aware(&self) -> Vec<OramBlock<K, V>> {
        let mut blocks = vec![OramBlock::dummy(); self.readout_len()];
        self.readout_tree_aware_into(&mut blocks);
        blocks
    }

    /// Performs tree-aware readout independently in fixed lower subtrees.
    ///
    /// Blocks above the public cut, together with the fixed overflow stash, are
    /// obliviously routed into fixed-size per-subtree inboxes. Copies of one key
    /// cannot cross subtree boundaries because they share a routing path. The
    /// returned length is public but may be slightly larger than [`Self::readout_len`]
    /// because every subtree receives the same statistically-sized inbox.
    pub fn readout_subtree_partitioned(
        &self,
        local_height: u64,
        security_bits: usize,
    ) -> Result<Vec<OramBlock<K, V>>, SubtreeReadoutOverflow> {
        if local_height >= self.height {
            return Ok(self.readout_tree_aware());
        }

        let cut_depth = self.height - local_height;
        let subtree_count = 1usize << cut_depth;
        let upper_tree_len = subtree_count * Z;
        let overflow = &self.stash.blocks[self.stash.path_size..];
        let upper_input_len = upper_tree_len + overflow.len();
        let suggested_quota = crate::oblivious::binomial_solver::suggested_per_shard_quota(
            upper_input_len,
            subtree_count,
            security_bits,
        );
        let inbox_len = suggested_quota.max(1).next_power_of_two();

        // Globally process only the geometrically small upper part of the tree.
        let mut upper = Vec::with_capacity(upper_input_len);
        upper.extend_from_slice(&self.physical_memory[..upper_tree_len]);
        upper.extend_from_slice(overflow);
        crate::oblivious::djbsort::sort_by(&mut upper, tree_readout_gt::<K, V>);
        crate::oblivious::reduction::reduce_equal_runs(&mut upper);
        let mut compact_marks = Vec::with_capacity(upper.len() + 1);
        crate::oblivious::compaction::compact_marks(&upper, &mut compact_marks);
        let real_upper_len = compact_marks[upper.len()];
        crate::oblivious::compaction::compact_payload(&mut upper, &compact_marks);

        // Count every destination with a fixed trace. The upper records are
        // PRF-routed balls and subtree prefixes are the public bins.
        let mut counts = vec![0u64; subtree_count];
        for (i, block) in upper.iter().enumerate() {
            let active = crate::ct::ct_lt(i as u64, real_upper_len as u64) & block.tag.ct_is_real();
            let destination =
                (block.tag.routing_bits() >> (routing::MAX_TREE_HEIGHT - cut_depth)) as usize;
            for (candidate, count) in counts.iter_mut().enumerate() {
                let matches = crate::ct::ct_eq(destination as u64, candidate as u64);
                *count = count.wrapping_add((active & matches) as u64);
            }
        }
        let mut max_load = 0u64;
        for count in &counts {
            max_load.cmovnz(count, crate::ct::ct_lt(max_load, *count));
        }
        let max_load = max_load as usize;
        if max_load > inbox_len {
            return Err(SubtreeReadoutOverflow { max_load, inbox_len, subtree_count });
        }

        let routed_len = subtree_count * inbox_len;
        upper.resize(routed_len, OramBlock::dummy());
        let mut distribute_marks = Vec::with_capacity(routed_len + 1);
        crate::sharded_oblivious_histogram::router::build_distribute_marks_router(
            &mut distribute_marks,
            &counts,
            inbox_len,
            routed_len,
        );
        crate::oblivious::compaction::distribute_payload(&mut upper, &distribute_marks);

        let local_tree_len = (1usize << (local_height + 1)) * Z;
        let chunk_len = local_tree_len + inbox_len;
        let mut output = vec![OramBlock::dummy(); subtree_count * chunk_len];
        output
            .par_chunks_mut(chunk_len)
            .enumerate()
            .for_each(|(subtree, chunk)| {
                // Node zero of every local tree remains the dummy bucket. The
                // real nodes are copied from their public breadth-first ranges.
                for local_depth in 0..=local_height {
                    let nodes_at_depth = 1usize << local_depth;
                    let global_depth = cut_depth + local_depth;
                    let global_first_node =
                        (1usize << global_depth) + subtree * nodes_at_depth;
                    let source_start = global_first_node * Z;
                    let source_end = source_start + nodes_at_depth * Z;
                    let destination_start = nodes_at_depth * Z;
                    let destination_end = destination_start + nodes_at_depth * Z;
                    chunk[destination_start..destination_end]
                        .copy_from_slice(&self.physical_memory[source_start..source_end]);
                }

                chunk[local_tree_len..]
                    .copy_from_slice(&upper[subtree * inbox_len..(subtree + 1) * inbox_len]);
                oblivious_tree_readout::<Z, K, V>(chunk, inbox_len, local_height);
            });
        Ok(output)
    }

    /// Consumes the histogram and reuses its tree allocation for tree-aware
    /// readout. This is the memory-scalable final-readout API: it avoids keeping
    /// both the live tree and a second full-size output array.
    pub fn into_readout_tree_aware(mut self) -> Vec<OramBlock<K, V>> {
        let overflow_len = self.stash.blocks.len() - self.stash.path_size;
        let mut blocks = std::mem::take(&mut self.physical_memory);
        // Constructors and resize preserve this spare capacity. Cloned Vecs do
        // not necessarily preserve capacity, so retain a public-size fallback.
        if blocks.capacity() < blocks.len() + overflow_len {
            blocks.reserve_exact(overflow_len);
        }
        blocks.extend_from_slice(&self.stash.blocks[self.stash.path_size..]);
        let height = self.height;
        drop(self);

        oblivious_tree_readout::<Z, K, V>(&mut blocks, overflow_len, height);
        blocks
    }

    pub(crate) fn readout_tree_aware_into(&self, blocks: &mut [OramBlock<K, V>]) {
        assert_eq!(blocks.len(), self.readout_len());
        let tree_len = self.physical_memory.len();
        blocks[..tree_len].copy_from_slice(&self.physical_memory);
        blocks[tree_len..].copy_from_slice(&self.stash.blocks[self.stash.path_size..]);
        oblivious_tree_readout::<Z, K, V>(
            blocks,
            self.stash.blocks.len() - self.stash.path_size,
            self.height,
        );
    }

    /// Diagnostic, non-oblivious export of only the real records.
    ///
    /// This compatibility helper leaks the distinct-key count and uses the
    /// standard library's data-dependent sort. Security-sensitive callers must
    /// use [`Self::readout`] and encrypt its complete fixed-length result.
    pub fn export_entries(&self) -> Vec<(u64, [u8; K], V)> {
        let mut blocks = Vec::new();
        // Collect from stash:
        for block in &self.stash.blocks {
            if block.tag.ct_is_real() != 0 {
                blocks.push(*block);
            }
        }
        // Collect from physical memory:
        for block in &self.physical_memory {
            if block.tag.ct_is_real() != 0 {
                blocks.push(*block);
            }
        }

        if blocks.is_empty() {
            return Vec::new();
        }

        // Sort by payload (non-oblivious standard sort)
        blocks.sort_by_key(|a| a.payload);

        // Merge duplicates
        let mut result = Vec::new();
        let mut current_tag = blocks[0].tag;
        let mut current_payload = blocks[0].payload;
        let mut current_val = blocks[0].value;

        for block in blocks.into_iter().skip(1) {
            if block.payload == current_payload {
                current_val = current_val + block.value;
            } else {
                result.push((current_tag, current_payload, current_val));
                current_tag = block.tag;
                current_payload = block.payload;
                current_val = block.value;
            }
        }
        result.push((current_tag, current_payload, current_val));

        result
    }
}

fn payload_gt<const K: usize, V: OramValue>(a: &OramBlock<K, V>, b: &OramBlock<K, V>) -> u8 {
    let a_real = a.tag.ct_is_real();
    let b_real = b.tag.ct_is_real();
    let a_dummy_after_real = crate::ct::ct_not(a_real) & b_real;
    let both_real = a_real & b_real;

    let mut greater = 0u8;
    let mut less = 0u8;
    for i in 0..K {
        let undecided = crate::ct::ct_not(greater | less);
        greater |= undecided & crate::ct::ct_lt_u8(b.payload[i], a.payload[i]);
        less |= undecided & crate::ct::ct_lt_u8(a.payload[i], b.payload[i]);
    }

    a_dummy_after_real | (both_real & greater)
}

#[inline]
fn tree_readout_gt<const K: usize, V: OramValue>(a: &OramBlock<K, V>, b: &OramBlock<K, V>) -> u8 {
    let a_real = a.tag.ct_is_real();
    let b_real = b.tag.ct_is_real();
    let a_dummy_after_real = crate::ct::ct_not(a_real) & b_real;
    let both_real = a_real & b_real;

    // Routing bits come first: physical bucket order fixes a prefix of this
    // value. The remaining PRF bits and full key make equal logical keys
    // adjacent even when distinct keys collide on the stored tag.
    let a_order = (a.tag.routing_bits() << 32) | (a.tag >> 32);
    let b_order = (b.tag.routing_bits() << 32) | (b.tag >> 32);
    let mut greater = crate::ct::ct_lt(b_order, a_order);
    let mut less = crate::ct::ct_lt(a_order, b_order);

    for i in 0..K {
        let undecided = crate::ct::ct_not(greater | less);
        greater |= undecided & crate::ct::ct_lt_u8(b.payload[i], a.payload[i]);
        less |= undecided & crate::ct::ct_lt_u8(a.payload[i], b.payload[i]);
    }

    a_dummy_after_real | (both_real & greater)
}

fn oblivious_tree_readout<const Z: usize, const K: usize, V: OramValue>(
    blocks: &mut [OramBlock<K, V>],
    overflow_len: usize,
    height: u64,
) {
    debug_assert!(Z.is_power_of_two());
    let tree_len = (1usize << (height + 1)) * Z;
    debug_assert_eq!(blocks.len(), overflow_len + tree_len);

    // Batcher's efficient unequal-run network below requires power-of-two run
    // lengths. All evaluated configurations satisfy this (Z and S are powers
    // of two); retain the general reference path for unusual public parameters.
    if !overflow_len.is_power_of_two() || overflow_len > tree_len {
        oblivious_readout(blocks);
        return;
    }

    let greater = tree_readout_gt::<K, V>;

    // The unused physical bucket at node index zero is the initial dummy run.
    // Each following depth has exactly the same length as all earlier tree
    // storage combined, producing the power-of-two merge schedule.

    let mut marks = Vec::new();
    for depth in 0..=height {
        let first_node = 1usize << depth;
        let end_node = 1usize << (depth + 1);
        let level_start = first_node * Z;
        let level_end = end_node * Z;
        let level = &mut blocks[level_start..level_end];

        // A node's path prefix is public from its physical position. Sorting
        // inside each bucket supplies the remaining order within that prefix.
        for bucket in level.chunks_exact_mut(Z) {
            crate::oblivious::djbsort::sort_by(bucket, greater);
        }

        // Remove dummy separators between buckets. Stable compaction preserves
        // the prefix/key order established above and retains a fixed run length.
        crate::oblivious::compaction::compact_marks(level, &mut marks);
        crate::oblivious::compaction::compact_payload(level, &marks);

        // Everything before this level is one ascending run; the current level
        // is another. Batcher's fixed bitonic merge accepts unequal run lengths.
        let split = level_start;
        crate::oblivious::djbsort::merge_sorted_by(&mut blocks[..level_end], split, greater);
    }

    // The fixed overflow stash is a final, much smaller sorted run. Batcher's
    // (m,n) odd-even network costs O(tree_len * log(overflow_len)) here rather
    // than padding the stash to a second full tree.
    crate::oblivious::djbsort::sort_by(&mut blocks[tree_len..], greater);
    crate::oblivious::djbsort::merge_sorted_by(blocks, tree_len, greater);

    crate::oblivious::reduction::reduce_equal_runs(blocks);
}

pub(crate) fn oblivious_readout<const K: usize, V: OramValue>(blocks: &mut [OramBlock<K, V>]) {
    crate::oblivious::djbsort::sort_by(blocks, payload_gt::<K, V>);
    crate::oblivious::reduction::reduce_equal_runs(blocks);

    let mut marks = Vec::with_capacity(blocks.len() + 1);
    crate::oblivious::compaction::compact_marks(blocks, &mut marks);
    crate::oblivious::compaction::compact_payload(blocks, &marks);
}

#[cfg(test)]
mod readout_tests {
    use super::oblivious_tree_readout;
    use crate::oblivious_histogram::routing::OramAddressMut;
    use crate::OramBlock;

    #[test]
    fn tree_readout_merges_a_key_across_nonadjacent_depths_and_stash() {
        const Z: usize = 2;
        const HEIGHT: u64 = 3;
        const TREE_LEN: usize = (1 << (HEIGHT + 1)) * Z;
        const OVERFLOW_LEN: usize = 4;

        let key = 7u128.to_be_bytes();
        let mut tag = 0x1234_5678_9abc_def0;
        tag.force_leaf(13, HEIGHT);

        let other_key = 11u128.to_be_bytes();
        let mut other_tag = 0x0fed_cba9_8765_4321;
        other_tag.force_leaf(8, HEIGHT);

        let mut blocks = vec![OramBlock::<16>::dummy(); TREE_LEN + OVERFLOW_LEN];
        blocks[Z] = OramBlock::real(tag, 0, 1, key); // root, depth 0
        blocks[6 * Z] = OramBlock::real(tag, 0, 2, key); // depth 2
        blocks[13 * Z] = OramBlock::real(tag, 0, 4, key); // leaf, depth 3
        blocks[4 * Z] = OramBlock::real(other_tag, 0, 10, other_key);
        blocks[TREE_LEN] = OramBlock::real(tag, 0, 8, key); // overflow stash

        oblivious_tree_readout::<Z, 16, u64>(&mut blocks, OVERFLOW_LEN, HEIGHT);

        let real: Vec<_> = blocks.iter().filter(|block| block.tag != 0).collect();
        assert_eq!(real.len(), 2);
        assert_eq!(real.iter().find(|block| block.payload == key).unwrap().value, 15);
        assert_eq!(real.iter().find(|block| block.payload == other_key).unwrap().value, 10);
    }
}
