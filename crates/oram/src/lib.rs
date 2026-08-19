// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory of this source tree.
// You may select, at your option, one of the above-listed licenses.

//! Resizable Oblivious Histogram (OSAM) with PRF-routed Path ORAM design.
//!
//! Provides single-tree ([`ObliviousHistogram`]) and multi-core sharded
//! ([`ShardedObliviousHistogram`]) data-oblivious histograms with constant-time
//! memory operations, differential privacy auto-resizing, and configurable key/value schemas.

#![warn(clippy::doc_markdown, rustdoc::all)]
#![allow(clippy::cargo)]

// --- Macros & Timings (Must be defined first for submodules to use them) ---
/// Timing helper for profiling. Compiles to a no-op unless the `profile` feature is enabled.
///
/// When `profile` is disabled, expands to `()` for zero runtime overhead.
#[cfg(feature = "profile")]
#[macro_export]
macro_rules! timing_scope {
    ($accumulator:expr) => {{
        let __start = std::time::Instant::now();
        $crate::TimingGuard { start: __start, accumulator: $accumulator }
    }};
}

#[cfg(not(feature = "profile"))]
#[macro_export]
macro_rules! timing_scope {
    ($accumulator:expr) => {
        ()
    };
}

/// RAII guard that accumulates elapsed execution duration upon drop.
#[cfg(feature = "profile")]
pub struct TimingGuard<'a> {
    /// Wall-clock start timestamp.
    pub start: std::time::Instant,
    /// Destination duration accumulator.
    pub accumulator: &'a mut std::time::Duration,
}

#[cfg(feature = "profile")]
impl Drop for TimingGuard<'_> {
    fn drop(&mut self) {
        *self.accumulator += self.start.elapsed();
    }
}

// --- Submodules ---
pub mod block;
pub mod metrics;
pub mod oblivious_histogram;
pub mod sharded_oblivious_histogram;

#[doc(hidden)]
pub mod oblivious;

#[doc(hidden)]
pub use crate::oblivious::ct;
pub use crate::oblivious::djbsort;
pub use crate::oblivious_histogram::routing::OramAddress;

// --- Type Aliases & Configuration Structs ---
/// Logical block or tree capacity type.
pub type Address = u64;
/// Stash occupancy count type.
pub type StashSize = u64;

/// Trait bound for types stored and aggregated as values in the oblivious histogram.
///
/// Invariants: must support constant-time conditional moves (`Cmov`), default/zero initialization,
/// and associative addition (`Add`).
pub trait OramValue:
    cmov::Cmov + Default + Copy + Send + Sync + std::fmt::Debug + std::ops::Add<Output = Self> + 'static
{
}
impl<
        V: cmov::Cmov
            + Default
            + Copy
            + Send
            + Sync
            + std::fmt::Debug
            + std::ops::Add<Output = V>
            + 'static,
    > OramValue for V
{
}

/// Configures the logical block data schema (key length `KEY_LEN` in bytes, value type `V`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockConfig<const KEY_LEN: usize = 16, V: OramValue = u64>(
    pub std::marker::PhantomData<V>,
);

/// Trait defining the schema parameters for oblivious blocks.
pub trait BlockParams: Copy + Send + Sync + 'static {
    /// Key length in bytes.
    const KEY_LEN: usize;
    /// Aggregated value type.
    type Value: OramValue;
}

impl<const KEY_LEN: usize, V: OramValue> BlockParams for BlockConfig<KEY_LEN, V> {
    const KEY_LEN: usize = KEY_LEN;
    type Value = V;
}

/// Configures physical ORAM tree parameters:
/// - `Z`: Bucket capacity (blocks per tree node, default 16).
/// - `A`: Eviction rate (insert operations between path evictions, default 20).
/// - `S`: Stash overflow capacity bound (default 64).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OramConfig<const Z: usize = 16, const A: usize = 20, const S: usize = 64>;

/// Trait defining the physical parameters of an ORAM binary tree.
pub trait OramParams: Copy + Send + Sync + 'static {
    /// Bucket capacity (blocks per tree node).
    const Z: usize;
    /// Eviction rate (inserts between path evictions).
    const A: usize;
    /// Stash overflow capacity bound.
    const S: usize;
}

impl<const Z: usize, const A: usize, const S: usize> OramParams for OramConfig<Z, A, S> {
    const Z: usize = Z;
    const A: usize = A;
    const S: usize = S;
}

pub use crate::block::{FlowRecord, OramBlock};
pub use crate::metrics::{OramMetrics, ShardedOramMetrics};
pub use crate::oblivious::copy_prefix;
pub use crate::oblivious_histogram::{AutoResizeConfig, ObliviousHistogram};
pub use crate::sharded_oblivious_histogram::router::prepare_key;
pub use crate::sharded_oblivious_histogram::{
    OramShard, ShardedBatchRouter, ShardedObliviousHistogram,
};

#[doc(hidden)]
pub mod testing {
    pub use crate::block::OramBlock;
    pub use crate::oblivious_histogram::routing::{address_to_leaf, OramAddress, OramAddressMut};
    pub use crate::oblivious_histogram::stash::ObliviousStash;
    pub use crate::oblivious_histogram::tree::{Bucket, TreeIndex};
}
