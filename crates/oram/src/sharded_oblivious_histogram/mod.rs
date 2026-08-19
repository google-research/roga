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

//! Sharded Oblivious Histogram implementation.

// --- Submodules ---
pub mod router;

// --- Imports ---

use aes::cipher::generic_array::GenericArray;
use aes::cipher::KeyInit;
use aes::Aes128;
use rand::rngs::StdRng;
use rand::{CryptoRng, Rng, RngExt, SeedableRng};
use rayon::prelude::*;

use crate::block::OramBlock;
use crate::metrics::ShardedOramMetrics;
use crate::oblivious::compaction::distribute_payload;
use crate::oblivious_histogram::routing::OramAddress;
use crate::oblivious_histogram::{AutoResizeConfig, ObliviousHistogram};
use crate::{ct, Address, StashSize};

use router::{build_distribute_marks_router, prepare_key, shard_index_for_tag};

type ExportedEntry<const K: usize = 16, V = u64> = (u64, [u8; K], V);

/// Trait for an individual ORAM shard processing batch chunks in parallel.
pub trait OramShard<const K: usize = 16, V = u64>: Send + Sync {
    /// Processes a chunk of buffered updates in constant time ($O(|chunk|)$).
    fn process_batch_chunk(&mut self, chunk: &[OramBlock<K, V>]);
}

use crate::OramValue;

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue> OramShard<K, V>
    for ObliviousHistogram<Z, K, A, S, V>
{
    fn process_batch_chunk(&mut self, chunk: &[OramBlock<K, V>]) {
        self.append_prepared_masked_batch(chunk.len(), |i| {
            let b = chunk[i];
            (b.tag, b.value, b.payload, b.tag.ct_is_real())
        });
    }
}

#[repr(align(64))]
struct RouterFrontend<const K: usize = 16, V = u64> {
    router_blocks: Vec<OramBlock<K, V>>,
    router_keys: Vec<u64>,
    router_marks: Vec<usize>,
    router_dist_marks: Vec<usize>,
    router_counts: Vec<u64>,
}

impl<const K: usize, V: OramValue> RouterFrontend<K, V> {
    fn new(sub_batch_size: usize, shard_count: usize, per_shard_sub_quota: usize) -> Self {
        let routed_len = shard_count * per_shard_sub_quota;
        let max_cap = routed_len.max(sub_batch_size);
        Self {
            router_blocks: vec![OramBlock::<K, V>::dummy(); max_cap],
            router_keys: vec![0u64; max_cap],
            router_marks: Vec::with_capacity(max_cap + 1),
            router_dist_marks: Vec::with_capacity(routed_len + 1),
            router_counts: vec![0u64; shard_count],
        }
    }

    fn process_sub_batch(
        &mut self,
        batch_slice: &[OramBlock<K, V>],
        shard_count: usize,
        per_shard_sub_quota: usize,
    ) {
        let n = batch_slice.len();
        self.router_blocks.resize(n, OramBlock::dummy());
        self.router_blocks.copy_from_slice(batch_slice);

        let reduced_len = crate::oblivious::compaction::sort_reduce_compact(
            &mut self.router_blocks,
            &mut self.router_keys,
            &mut self.router_marks,
            |block| {
                let tag = block.tag;
                let shard = shard_index_for_tag(tag, shard_count) as u64;
                let real = tag.ct_is_real();
                (shard << 58) | (((ct::ct_not(real)) as u64) << 57) | (tag & 0x01ff_ffff_ffff_ffff)
            },
        );

        self.router_counts.fill(0);
        for i in 0..reduced_len {
            let tag = self.router_blocks[i].tag;
            let shard = shard_index_for_tag(tag, shard_count);
            self.router_counts[shard] += 1;
        }

        let mut max_load = 0usize;
        for &c in &self.router_counts {
            if (c as usize) > max_load {
                max_load = c as usize;
            }
        }
        assert!(
            max_load <= per_shard_sub_quota,
            "sharded OSAM batch overflow: frontend shard load {max_load} exceeds per-frontend sub-quota {per_shard_sub_quota}"
        );

        let routed_len = shard_count * per_shard_sub_quota;
        self.router_blocks.resize(routed_len, OramBlock::dummy());
        build_distribute_marks_router(
            &mut self.router_dist_marks,
            &self.router_counts,
            per_shard_sub_quota,
            routed_len,
        );
        distribute_payload(&mut self.router_blocks, &self.router_dist_marks);
    }
}

/// Multi-core parallel batch router distributing updates to shard trees.
pub struct ShardedBatchRouter<S: OramShard<K, V>, const K: usize = 16, V = u64> {
    /// Array of physical ORAM shards.
    pub shards: Vec<S>,
    /// Master PRF instance for key routing.
    pub prf: Aes128,
    /// Number of physical shards.
    pub shard_count: usize,
    /// Target batch buffer capacity.
    pub batch_capacity: usize,
    /// Maximum allowed items per shard per flush quota.
    pub per_shard_quota: usize,
    /// Number of parallel frontend routing worker threads.
    pub frontend_count: usize,
    /// Pending buffered updates.
    pub pending: Vec<OramBlock<K, V>>,
    frontends: Vec<RouterFrontend<K, V>>,
    /// Accumulated runtime diagnostic metrics.
    pub metrics: ShardedOramMetrics,
}

impl<S: OramShard<K, V>, const K: usize, V: OramValue> ShardedBatchRouter<S, K, V> {
    /// Constructs a new batch router with 4 frontend threads ($O(M)$).
    pub fn new<R: Rng + CryptoRng>(
        shards: Vec<S>,
        batch_size: usize,
        per_shard_quota: usize,
        rng: &mut R,
    ) -> Self {
        Self::new_with_frontends(shards, batch_size, per_shard_quota, 4, rng)
    }

    /// Constructs a new batch router with `frontend_count` frontend threads ($O(M)$).
    pub fn new_with_frontends<R: Rng + CryptoRng>(
        shards: Vec<S>,
        batch_size: usize,
        per_shard_quota: usize,
        frontend_count: usize,
        rng: &mut R,
    ) -> Self {
        let shard_count = shards.len();
        let mut raw_key = [0u8; 16];
        rng.fill_bytes(&mut raw_key);
        let prf = Aes128::new(GenericArray::from_slice(&raw_key));

        let frontend_count = frontend_count.max(1);
        let sub_batch_size = batch_size / frontend_count;
        let per_shard_sub_quota = per_shard_quota / frontend_count;
        let frontends = (0..frontend_count)
            .map(|_| RouterFrontend::new(sub_batch_size, shard_count, per_shard_sub_quota))
            .collect();
        let pending = Vec::with_capacity(batch_size);

        Self {
            shards,
            prf,
            shard_count,
            batch_capacity: batch_size,
            per_shard_quota,
            frontend_count,
            pending,
            frontends,
            metrics: ShardedOramMetrics::default(),
        }
    }

    /// Appends blocks to the batch buffer, flushing when capacity is reached ($O(1)$ amortized).
    pub fn increment_blocks(&mut self, mut blocks: Vec<OramBlock<K, V>>) {
        self.pending.append(&mut blocks);
        while self.pending.len() >= self.batch_capacity {
            self.flush();
        }
    }

    /// Flushes all pending buffered updates through parallel sorting and shard dispatch ($O(B \log B / F)$).
    pub fn flush(&mut self) {
        if self.pending.is_empty() {
            return;
        }
        let _total = timing_scope!(&mut self.metrics.total_flush_time);
        #[cfg(feature = "profile")]
        {
            self.metrics.flush_count += 1;
        }

        let n = self.batch_capacity;
        let f_count = self.frontends.len();
        let sub_n = n / f_count;
        let sub_quota = self.per_shard_quota / f_count;
        let shard_count = self.shard_count;

        if self.pending.len() < n {
            self.pending.resize(n, OramBlock::dummy());
        }

        {
            let _t = timing_scope!(&mut self.metrics.compact);
            self.frontends
                .par_iter_mut()
                .zip(self.pending[..n].par_chunks(sub_n))
                .with_min_len(1)
                .for_each(|(frontend, sub_batch)| {
                    frontend.process_sub_batch(sub_batch, shard_count, sub_quota);
                });
        }

        {
            let _t = timing_scope!(&mut self.metrics.parallel_shard_process_time);
            let frontends = &self.frontends;

            self.shards.par_iter_mut().enumerate().with_min_len(1).for_each(|(s, shard)| {
                for f in frontends {
                    let chunk = &f.router_blocks[s * sub_quota..(s + 1) * sub_quota];
                    shard.process_batch_chunk(chunk);
                }
            });
        }

        if self.pending.len() == n {
            self.pending.clear();
        } else {
            self.pending.drain(..n);
        }
    }
}

/// Multi-core sharded batch layer over independent PRF-routed Path OSAM trees.
///
/// Invariants:
/// - Batches of size $B$ are obliviously sorted and partitioned across $M$ shards in parallel.
/// - Read operations flush pending updates and perform dummy reads across all other shards.
pub struct ShardedObliviousHistogram<
    const Z: usize = 16,
    const K: usize = 16,
    const A: usize = 20,
    const S: usize = 64,
    V: OramValue = u64,
> {
    /// Batch routing layer managing shard trees.
    pub router: ShardedBatchRouter<ObliviousHistogram<Z, K, A, S, V>, K, V>,
    /// Global resize epoch.
    pub epoch: u8,
    capacity: u64,
    size_in_bytes: u64,
    auto_resize: bool,
    dummy_read_rng: StdRng,
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue>
    ShardedObliviousHistogram<Z, K, A, S, V>
{
    /// Constructs a new sharded OSAM keyed histogram with 4 frontend threads ($O(M \cdot N/M)$).
    pub fn new<R: Rng + CryptoRng>(
        shard_count: usize,
        total_block_capacity: Address,
        batch_size: usize,
        per_shard_quota: usize,
        rng: &mut R,
    ) -> Self {
        Self::new_with_frontends(
            shard_count,
            total_block_capacity,
            batch_size,
            per_shard_quota,
            4,
            rng,
        )
    }

    /// Constructs a new sharded OSAM keyed histogram with `frontend_count` frontend threads ($O(M \cdot N/M)$).
    pub fn new_with_frontends<R: Rng + CryptoRng>(
        shard_count: usize,
        total_block_capacity: Address,
        batch_size: usize,
        per_shard_quota: usize,
        frontend_count: usize,
        rng: &mut R,
    ) -> Self {
        let shard_capacity = total_block_capacity / (shard_count as Address);
        let trees: Vec<_> = (0..shard_count)
            .map(|_| ObliviousHistogram::<Z, K, A, S, V>::new(shard_capacity, rng))
            .collect();

        let dummy_read_rng = StdRng::seed_from_u64(rng.random());
        let size_in_bytes = trees.iter().map(|t| t.size_in_bytes()).sum();
        let router = ShardedBatchRouter::new_with_frontends(
            trees,
            batch_size,
            per_shard_quota,
            frontend_count,
            rng,
        );

        Self {
            router,
            epoch: 0u8,
            capacity: total_block_capacity,
            size_in_bytes,
            auto_resize: false,
            dummy_read_rng,
        }
    }

    /// Computes the minimal shard quota ensuring overflow probability $\le 2^{-\text{security\_bits}}$ ($O(\log B)$).
    pub fn suggested_per_shard_quota(
        batch_size: usize,
        shard_count: usize,
        security_bits: usize,
    ) -> usize {
        Self::suggested_per_shard_quota_with_frontends(batch_size, shard_count, security_bits, 4)
    }

    /// Computes the minimal per-frontend shard quota ensuring bounded overflow probability ($O(\log(B/F))$).
    pub fn suggested_per_shard_quota_with_frontends(
        batch_size: usize,
        shard_count: usize,
        security_bits: usize,
        frontend_count: usize,
    ) -> usize {
        let f = frontend_count.max(1);
        f * router::suggested_per_shard_quota(batch_size / f, shard_count, security_bits)
    }

    /// Returns the number of physical ORAM shards ($O(1)$).
    pub fn shard_count(&self) -> usize {
        self.router.shard_count
    }

    /// Returns the total physical size of all ORAM shards in bytes ($O(1)$).
    pub fn size_in_bytes(&self) -> u64 {
        self.size_in_bytes
    }

    /// Returns the total current stash occupancy across all shards ($O(M)$).
    pub fn stash_occupancy(&self) -> u64 {
        self.router.shards.iter().map(|t| t.stash_occupancy()).sum()
    }

    /// Enables deferred auto-resizing across all shards with inter-shard coordination slack ($O(M)$).
    pub fn enable_auto_resize(&mut self, cfg: AutoResizeConfig) {
        self.auto_resize = true;
        let shard_count = self.router.shard_count;
        let per_shard_target = cfg.t_capacity / (shard_count as u64);
        let slack = crate::oblivious::binomial_solver::shard_coordination_slack(
            per_shard_target as usize,
            shard_count,
            40,
        ) as u64;
        let deflated_target = per_shard_target.saturating_sub(slack).max(1);

        for (i, tree) in self.router.shards.iter_mut().enumerate() {
            let mut shard_cfg = cfg;
            shard_cfg.t_capacity = deflated_target;
            shard_cfg.seed = cfg.seed.wrapping_add(i as u64);
            tree.enable_deferred_auto_resize(shard_cfg);
        }
    }

    /// Returns the accumulated timing metrics for the sharded router ($O(1)$).
    pub fn metrics(&self) -> ShardedOramMetrics {
        self.router.metrics
    }

    /// Resets the accumulated timing metrics to zero across all shards ($O(M)$).
    pub fn reset_metrics(&mut self) {
        self.router.metrics = ShardedOramMetrics::default();
        for tree in &mut self.router.shards {
            tree.reset_metrics();
        }
    }

    /// Returns the peak overflow stash occupancy observed across all shards ($O(M)$).
    pub fn peak_overflow(&self) -> StashSize {
        self.router.shards.iter().map(|t| t.peak_overflow()).max().unwrap_or(0)
    }

    /// Returns the physical block capacity of a single shard ($O(1)$).
    pub fn shard_capacity(&self) -> Address {
        self.capacity / (self.router.shard_count as Address)
    }

    /// Returns the total physical block capacity across all shards ($O(1)$).
    pub fn total_capacity(&self) -> Address {
        self.capacity
    }

    /// Doubles the physical capacity of all ORAM shards in parallel ($O(N/M)$ parallel).
    pub fn grow(&mut self) {
        self.epoch = self.epoch.saturating_add(1);
        self.router.shards.par_iter_mut().with_min_len(1).for_each(|tree| tree.grow());
        self.capacity *= 2;
        self.size_in_bytes *= 2;
    }

    /// Reads the total value under `key` by querying target shard and executing dummy reads on all others ($O(M \cdot (ZL + S\log S))$).
    ///
    /// Automatically flushes pending writes before reading.
    pub fn read_total(&mut self, key: &[u8]) -> V {
        self.flush();
        let (target_shard, prepared_key) = prepare_key(&self.router.prf, key, self.shard_count());
        let payload = crate::oblivious::copy_prefix::<K>(key);
        let mut result = V::default();

        for (shard, tree) in self.router.shards.iter_mut().enumerate() {
            if shard == target_shard {
                result = tree.read_prepared_total(prepared_key, &payload);
            } else {
                tree.read_dummy(&mut self.dummy_read_rng);
            }
        }

        result
    }

    /// Extracts all raw entries across all shards in parallel, merging duplicate keys ($O(N \log N)$).
    pub fn export_entries(&mut self) -> Vec<(u64, [u8; K], V)> {
        self.flush();
        let shard_results: Vec<Vec<ExportedEntry<K, V>>> =
            self.router.shards.par_iter().map(|tree| tree.export_entries()).collect();

        shard_results.into_iter().flatten().collect()
    }

    /// Appends a key-value update to the batch buffer, flushing automatically when full ($O(1)$ amortized).
    pub fn append(&mut self, key: &[u8], value: V) {
        let (_, prepared_key) = prepare_key(&self.router.prf, key, self.shard_count());
        let payload = crate::oblivious::copy_prefix::<K>(key);
        self.router.pending.push(OramBlock::real(prepared_key, self.epoch, value, payload));
        if self.router.pending.len() >= self.router.batch_capacity {
            self.flush();
        }
    }

    /// Flushes buffered updates to physical ORAM shards and checks deferred auto-resize signals ($O(B \log B / F)$).
    pub fn flush(&mut self) {
        self.router.flush();

        // Deferred auto-resize check
        Self::handle_deferred_resize(
            self.auto_resize,
            &mut self.router.shards,
            &mut self.capacity,
            &mut self.size_in_bytes,
        );
    }

    fn handle_deferred_resize(
        auto_resize: bool,
        trees: &mut [ObliviousHistogram<Z, K, A, S, V>],
        capacity: &mut u64,
        size_in_bytes: &mut u64,
    ) {
        if !auto_resize {
            return;
        }
        let grow_signals: Vec<Option<u64>> = trees
            .par_iter_mut()
            .with_min_len(1)
            .map(|tree| tree.check_deferred_auto_resize_signal())
            .collect();

        if let Some(target) = grow_signals[0] {
            trees
                .par_iter_mut()
                .with_min_len(1)
                .for_each(|tree| tree.apply_deferred_auto_resize_grow(target));
            *capacity *= 2;
            *size_in_bytes *= 2;
        }
    }
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue> Drop
    for ShardedObliviousHistogram<Z, K, A, S, V>
{
    fn drop(&mut self) {
        if !self.router.pending.is_empty() {
            if std::thread::panicking() {
                eprintln!("WARNING: ShardedObliviousHistogram dropped with {} pending updates! Call flush() before dropping to avoid losing data.", self.router.pending.len());
            } else {
                panic!("ShardedObliviousHistogram dropped with {} pending updates! Call flush() before dropping to avoid losing data.", self.router.pending.len());
            }
        }
    }
}
