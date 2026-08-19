// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Timing and performance metrics definitions.

use std::time::Duration;

/// Detailed diagnostic and performance metrics for single-tree ORAM operations.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct OramMetrics {
    /// Time spent hashing user keys to internal block addresses.
    pub key_hash: Duration,
    /// Time spent inserting blocks into the stash overflow buffer.
    pub insert: Duration,
    /// Time spent reading eviction paths into the stash.
    pub read_path: Duration,
    /// Time spent folding same-address blocks in the stash.
    pub merge_accumulate: Duration,
    /// Stash merge subphase: sorting blocks.
    pub merge_detail_sort: Duration,
    /// Stash merge subphase: reducing equal-address runs.
    pub merge_detail_reduce: Duration,
    /// Time spent updating auto-resize counters before writeback.
    pub resize_accounting: Duration,
    /// Time spent writing the stash back to the eviction path.
    pub write_to_path: Duration,
    /// Writeback subphase: scratch allocation and setup.
    pub write_to_path_setup: Duration,
    /// Writeback subphase: assigning real blocks to writable levels.
    pub write_to_path_assign_real: Duration,
    /// Writeback subphase: compacting survivors.
    pub write_to_path_compact: Duration,
    /// Writeback subphase: copying path back to memory buckets.
    pub write_to_path_copy_back: Duration,
    /// Time spent checking and performing auto-resize.
    pub resize_check: Duration,
    /// Peak overflow stash occupancy observed across operations.
    pub peak_overflow: u64,
}

impl std::ops::AddAssign for OramMetrics {
    fn add_assign(&mut self, rhs: Self) {
        self.key_hash += rhs.key_hash;
        self.insert += rhs.insert;
        self.read_path += rhs.read_path;
        self.merge_accumulate += rhs.merge_accumulate;
        self.merge_detail_sort += rhs.merge_detail_sort;
        self.merge_detail_reduce += rhs.merge_detail_reduce;
        self.resize_accounting += rhs.resize_accounting;
        self.write_to_path += rhs.write_to_path;
        self.write_to_path_setup += rhs.write_to_path_setup;
        self.write_to_path_assign_real += rhs.write_to_path_assign_real;
        self.write_to_path_compact += rhs.write_to_path_compact;
        self.write_to_path_copy_back += rhs.write_to_path_copy_back;
        self.resize_check += rhs.resize_check;
        self.peak_overflow = self.peak_overflow.max(rhs.peak_overflow);
    }
}

/// Detailed diagnostic and performance metrics for multi-core sharded ORAM operations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ShardedOramMetrics {
    /// Total wall clock time spent during flush.
    pub total_flush_time: Duration,
    /// Time spent preparing keys and records for routing.
    pub prepare_work: Duration,
    /// Time spent sorting records by shard and key.
    pub sort: Duration,
    /// Time spent reducing duplicate keys.
    pub reduce: Duration,
    /// Time spent compacting survivor records.
    pub compact: Duration,
    /// Time spent calculating loads per shard.
    pub load_count: Duration,
    /// Time spent distributing records to shard subarrays.
    pub distribute: Duration,
    /// Parallel shard process wall clock duration.
    pub parallel_shard_process_time: Duration,
    /// Number of flushes performed.
    pub flush_count: u64,
}
