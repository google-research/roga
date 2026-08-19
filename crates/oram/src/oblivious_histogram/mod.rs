// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Oblivious Histogram: type definition, construction, accessors, and export.
//!
//! The operation pipeline (insert/read/evict) lives in [`ops`], and all
//! resize logic (decision + mechanism + DP math) lives in [`resize`].

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
use rand::{CryptoRng, Rng};

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
pub struct ObliviousHistogram<
    const Z: usize = 16,
    const K: usize = 16,
    const A: usize = 20,
    const S: usize = 64,
    V = u64,
> {
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

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: fmt::Debug> fmt::Debug
    for ObliviousHistogram<Z, K, A, S, V>
{
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

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue>
    ObliviousHistogram<Z, K, A, S, V>
{
    /// Constructs a keyed ORAM histogram with a fresh PRF key for the given capacity ($O(N)$).
    pub fn new<R: Rng + CryptoRng>(block_capacity: Address, rng: &mut R) -> Self {
        let mut raw_key = [0u8; 16];
        rng.fill_bytes(&mut raw_key);
        let prf = Aes128::new(GenericArray::from_slice(&raw_key));
        let height = tree::tree_height_for_capacity(block_capacity, Z);
        let path_size = (Z as u64) * (height + 1);
        let stash = ObliviousStash::<Z, K, S, V>::new(path_size);
        let physical_memory = vec![OramBlock::<K, V>::dummy(); (1usize << (height + 1)) * Z];

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

    /// For benchmarking: sets the lazy sweep migration window active state ($O(1)$).
    pub fn set_sweep_window_active(&mut self, active: bool) {
        if active {
            self.sweep_end = u64::MAX;
        } else {
            self.sweep_end = 0;
        }
    }

    /// Extracts and merges all non-dummy entries, returning raw `(tag, payload, value)` tuples ($O(N \log N)$).
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
