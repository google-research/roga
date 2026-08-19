// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Core operation pipeline for `ObliviousHistogram`: insert, read, and eviction.

use rand::{Rng, RngExt};

use super::routing::OramAddress;
use super::tree::{leaf_count, TreeIndex};
use super::ObliviousHistogram;

use crate::OramValue;

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue>
    ObliviousHistogram<Z, K, A, S, V>
{
    /// Appends `val` to the aggregate value under `key` ($O(1)$ amortized, $O(Z \cdot L + S \log S)$ on eviction).
    ///
    /// The update is buffered in the stash and flushed to the tree periodically every `A` inserts.
    pub fn append(&mut self, key: &[u8], val: V) {
        let prepared_key = {
            let _t = timing_scope!(&mut self.metrics.key_hash);
            crate::oblivious::crypto::prf_tag(&self.prf, key)
        };
        let payload = crate::oblivious::copy_prefix::<K>(key);
        self.add_prepared_inner(prepared_key, val, payload);
    }

    /// Explicitly flushes any pending buffered operations ($O(1)$ no-op for single tree).
    pub fn flush(&mut self) {}

    pub(crate) fn append_prepared_masked_batch<G>(&mut self, count: usize, mut update_at: G)
    where
        G: FnMut(usize) -> (u64, V, [u8; K], u8),
    {
        let mut offset = 0usize;

        while offset < count {
            let evict_period = A as u64;
            let interval_offset = self.append_ctr % evict_period;
            let until_evict =
                if interval_offset == 0 { evict_period } else { evict_period - interval_offset }
                    as usize;
            let chunk_len = (count - offset).min(until_evict);

            {
                let epoch = self.epoch;
                let _t = timing_scope!(&mut self.metrics.insert);
                self.stash.insert_batch(chunk_len, |i| {
                    let (tag, val, payload, cond) = update_at(offset + i);
                    (tag, epoch, val, payload, cond)
                });
            }

            self.append_ctr += chunk_len as u64;
            offset += chunk_len;
            if self.append_ctr.is_multiple_of(evict_period) {
                self.evict_after_insert();
            }
        }
    }

    fn add_prepared_inner(&mut self, key: u64, val: V, payload: [u8; K]) {
        {
            let _t = timing_scope!(&mut self.metrics.insert);
            self.stash.insert(key, self.epoch, val, &payload);
        }
        self.finish_append_after_insert();
    }

    fn finish_append_after_insert(&mut self) {
        self.append_ctr += 1;

        if !self.append_ctr.is_multiple_of(A as u64) {
            return;
        }

        self.evict_after_insert();
    }

    /// Reads and removes the aggregated value at `key` via path access ($O(Z \cdot L + S \log S)$).
    ///
    /// Returns `V::default()` if `key` is absent.
    pub fn read_total(&mut self, key: &[u8]) -> V {
        let tag = crate::oblivious::crypto::prf_tag(&self.prf, key);
        let payload = crate::oblivious::copy_prefix::<K>(key);
        self.read_prepared_total(tag, &payload)
    }

    pub(crate) fn read_prepared_total(&mut self, tag: u64, payload: &[u8; K]) -> V {
        let leaf = tag.to_leaf(self.height);
        self.stash.read_from_path(&mut self.physical_memory, leaf);
        self.stash.merge_accumulate_for_path(leaf);
        let (result, _payload) = self.stash.read_and_remove(payload);
        self.stash.write_to_path(&mut self.physical_memory, leaf);
        self.update_peak_overflow();
        result
    }

    /// Performs an oblivious dummy read/write path access without removing a logical key ($O(Z \cdot L + S \log S)$).
    pub fn read_dummy<R: Rng>(&mut self, rng: &mut R) {
        let leaf = leaf_count(self.height) | rng.random_range(0..leaf_count(self.height));
        self.stash.read_from_path(&mut self.physical_memory, leaf);
        self.stash.merge_accumulate_for_path(leaf);
        self.stash.write_to_path(&mut self.physical_memory, leaf);
        self.update_peak_overflow();
    }

    fn update_peak_overflow(&mut self) {
        let cur = self.stash.occupancy();
        if cur > self.metrics.peak_overflow {
            self.metrics.peak_overflow = cur;
        }
    }

    pub(super) fn next_evict_leaf(&mut self) -> TreeIndex {
        let num_leaves = leaf_count(self.height);
        let raw = self.evict_ctr % num_leaves;
        self.evict_ctr += 1;
        let rev = raw.reverse_bits() >> (64 - self.height);
        leaf_count(self.height) | rev
    }

    pub(super) fn evict_after_insert(&mut self) {
        let evict_leaf = self.next_evict_leaf();

        {
            let _t1 = timing_scope!(&mut self.metrics.read_path);
            self.stash.read_from_path(&mut self.physical_memory, evict_leaf);
            if self.evict_ctr <= self.sweep_end {
                self.stash.read_sibling_bucket(&self.physical_memory, evict_leaf ^ 1);
            }
        }

        let merge_metrics = self.stash.merge_accumulate_for_path(evict_leaf);
        self.metrics += merge_metrics;

        self.accumulate_resize_accounting(evict_leaf);

        let write_to_path_metrics = self.stash.write_to_path(&mut self.physical_memory, evict_leaf);
        self.metrics += write_to_path_metrics;
        self.update_peak_overflow();

        self.check_and_maybe_resize_post_eviction();
    }
}
