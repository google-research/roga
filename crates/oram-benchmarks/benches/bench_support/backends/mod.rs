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

pub mod hashmap;
pub mod oram_backend;

#[cfg(feature = "facebook-baseline")]
pub mod facebook_oram;

#[cfg(feature = "h2o2ram-baseline")]
pub mod h2o2ram;

#[cfg(feature = "obliviouslabs-baseline")]
pub mod obliviouslabs_oram;

#[cfg(feature = "mc-oblivious-baseline")]
pub mod mc_oblivious;

use crate::bench_support::interface::OramBenchBackend;
use oram::{ObliviousHistogram, ShardedObliviousHistogram};
use rand::{rngs::StdRng, SeedableRng};

/// Expands supported (Z, A, S) configurations for `oram-fixed` to satisfy const generics
/// for `ZaSweep` and standard benchmarks without exploding the Cartesian product.
macro_rules! match_fixed_z_a_s {
    ($z:expr, $a:expr, $s:expr, $mac:ident) => {
        match ($z, $a, $s) {
            // Z = 4 (for ZaSweep)
            (4, 2, 256) => Ok($mac!(4, 2, 256)),
            (4, 3, 256) => Ok($mac!(4, 3, 256)),
            (4, 4, 256) => Ok($mac!(4, 4, 256)),
            (4, 5, 256) => Ok($mac!(4, 5, 256)),
            (4, 6, 256) => Ok($mac!(4, 6, 256)),
            (4, 7, 256) => Ok($mac!(4, 7, 256)),
            (4, 8, 256) => Ok($mac!(4, 8, 256)),
            (4, 9, 256) => Ok($mac!(4, 9, 256)),
            (4, 10, 256) => Ok($mac!(4, 10, 256)),

            // Z = 8 (for ZaSweep)
            (8, 4, 256) => Ok($mac!(8, 4, 256)),
            (8, 5, 256) => Ok($mac!(8, 5, 256)),
            (8, 6, 256) => Ok($mac!(8, 6, 256)),
            (8, 7, 256) => Ok($mac!(8, 7, 256)),
            (8, 8, 256) => Ok($mac!(8, 8, 256)),
            (8, 9, 256) => Ok($mac!(8, 9, 256)),
            (8, 10, 256) => Ok($mac!(8, 10, 256)),
            (8, 11, 256) => Ok($mac!(8, 11, 256)),
            (8, 12, 256) => Ok($mac!(8, 12, 256)),
            (8, 13, 256) => Ok($mac!(8, 13, 256)),
            (8, 14, 256) => Ok($mac!(8, 14, 256)),
            (8, 15, 256) => Ok($mac!(8, 15, 256)),
            (8, 16, 256) => Ok($mac!(8, 16, 256)),

            // Z = 16 (for ZaSweep + standard benchmarks)
            (16, 8, 256) => Ok($mac!(16, 8, 256)),
            (16, 10, 256) => Ok($mac!(16, 10, 256)),
            (16, 12, 256) => Ok($mac!(16, 12, 256)),
            (16, 14, 256) => Ok($mac!(16, 14, 256)),
            (16, 16, 256) => Ok($mac!(16, 16, 256)),
            (16, 18, 256) => Ok($mac!(16, 18, 256)),
            (16, 20, 256) => Ok($mac!(16, 20, 256)),
            (16, 20, 64) => Ok($mac!(16, 20, 64)),
            (16, 22, 256) => Ok($mac!(16, 22, 256)),
            (16, 24, 256) => Ok($mac!(16, 24, 256)),
            (16, 26, 256) => Ok($mac!(16, 26, 256)),
            (16, 28, 256) => Ok($mac!(16, 28, 256)),
            (16, 30, 256) => Ok($mac!(16, 30, 256)),
            (16, 32, 256) => Ok($mac!(16, 32, 256)),
            (16, 36, 256) => Ok($mac!(16, 36, 256)),
            (16, 40, 256) => Ok($mac!(16, 40, 256)),

            // Z = 32 (for ZaSweep)
            (32, 16, 256) => Ok($mac!(32, 16, 256)),
            (32, 20, 256) => Ok($mac!(32, 20, 256)),
            (32, 24, 256) => Ok($mac!(32, 24, 256)),
            (32, 28, 256) => Ok($mac!(32, 28, 256)),
            (32, 32, 256) => Ok($mac!(32, 32, 256)),
            (32, 36, 256) => Ok($mac!(32, 36, 256)),
            (32, 40, 256) => Ok($mac!(32, 40, 256)),
            (32, 44, 256) => Ok($mac!(32, 44, 256)),
            (32, 48, 256) => Ok($mac!(32, 48, 256)),
            (32, 52, 256) => Ok($mac!(32, 52, 256)),
            (32, 56, 256) => Ok($mac!(32, 56, 256)),
            (32, 60, 256) => Ok($mac!(32, 60, 256)),
            (32, 64, 256) => Ok($mac!(32, 64, 256)),
            (32, 72, 256) => Ok($mac!(32, 72, 256)),
            (32, 80, 256) => Ok($mac!(32, 80, 256)),

            // Z = 64 (Canonical & SOTA)
            (64, 20, 256) => Ok($mac!(64, 20, 256)),
            (64, 20, 64) => Ok($mac!(64, 20, 64)),
            (64, 16, 64) => Ok($mac!(64, 16, 64)),
            (64, 16, 256) => Ok($mac!(64, 16, 256)),

            _ => Err(format!(
                "Unsupported (Z={}, A={}, S={}) for oram-fixed. See ZaSweep and canonical benchmarks for supported configurations.",
                $z, $a, $s
            )),
        }
    };
}

/// Instantiates a concrete `OramBenchBackend` trait object based on configuration string and benchmark parameters.
///
/// Enforces minimum capacity (>= 65536) for tree-based ORAMs to avoid degenerate linear sweep fallbacks.
pub fn create_backend(
    name: &str,
    capacity: u64,
    z: usize,
    seed: u64,
    cores: usize,
    overflow: u64,
    evict_interval: u64,
    batch_size: Option<usize>,
) -> Result<Box<dyn OramBenchBackend>, String> {
    if name != "hashmap" && !name.contains("h2o2ram") {
        assert!(
            capacity >= 65536,
            "ORAM capacity must be at least 65536 to avoid linear sweep fallback (got {}) for backend {}",
            capacity,
            name
        );
    }
    let a_val = evict_interval as usize;
    let s_val = overflow as usize;

    match name {
        "hashmap" => Ok(Box::new(hashmap::HashMapBenchWrapper::new("HashMap"))),
        "oram-fixed" => {
            let mut rng = StdRng::seed_from_u64(seed ^ capacity ^ 0x05A0_5001);

            macro_rules! make_fixed {
                ($z_val:expr, $a_val:expr, $s_val:expr) => {{
                    let h = ObliviousHistogram::<$z_val, 16, $a_val, $s_val>::new(capacity, &mut rng);
                    Box::new(oram_backend::OramBenchWrapper::new_single(
                        format!("ORAM fixed (Z={}, A={}, S={})", $z_val, $a_val, $s_val),
                        h,
                    )) as Box<dyn OramBenchBackend>
                }};
            }

            match_fixed_z_a_s!(z, a_val, s_val, make_fixed)
        }
        "oram-resizing" => {
            let mut rng = StdRng::seed_from_u64(seed ^ 0x0A00_5000);

            macro_rules! make_resizing {
                ($z_val:expr, $a_val:expr, $s_val:expr) => {{
                    let h = ObliviousHistogram::<$z_val, 16, $a_val, $s_val>::new(capacity, &mut rng);
                    let h = configure_resizing_osam(h, seed);
                    Box::new(oram_backend::OramBenchWrapper::new_single(
                        format!("ORAM resizing (Z={}, A={}, S={})", $z_val, $a_val, $s_val),
                        h,
                    )) as Box<dyn OramBenchBackend>
                }};
            }

            match (z, a_val, s_val) {
                (64, 20, 256) => Ok(make_resizing!(64, 20, 256)),
                (16, 20, 64) => Ok(make_resizing!(16, 20, 64)),
                (64, 16, 64) => Ok(make_resizing!(64, 16, 64)),
                (64, 20, 64) => Ok(make_resizing!(64, 20, 64)),
                _ => Err(format!(
                    "Unsupported (Z={}, A={}, S={}) for oram-resizing; only canonical configurations are compiled.",
                    z, a_val, s_val
                )),
            }
        }
        #[cfg(feature = "facebook-baseline")]
        "facebook-oram" => {
            Ok(Box::new(facebook_oram::FacebookOramBenchWrapper::new("Stock ORAM", capacity, seed)))
        }
        "sharded-oram" => {
            let shard_count = cores.max(1);
            let frontend_count = cores.max(1);
            let batch_size = batch_size.unwrap_or_else(|| (shard_count * 4096).max(4096));
            let mut rng = StdRng::seed_from_u64(seed);

            macro_rules! make_sharded {
                ($z_val:expr, $a_val:expr, $s_val:expr) => {{
                    let per_shard_quota =
                        ShardedObliviousHistogram::<$z_val, 16, $a_val, $s_val>::suggested_per_shard_quota_with_frontends(
                            batch_size,
                            shard_count,
                            80,
                            frontend_count,
                        );
                    let hist = ShardedObliviousHistogram::<$z_val, 16, $a_val, $s_val>::new_with_frontends(
                        shard_count,
                        capacity,
                        batch_size,
                        per_shard_quota,
                        frontend_count,
                        &mut rng,
                    );
                    Box::new(oram_backend::OramBenchWrapper::new_sharded(
                        format!(
                            "Sharded ORAM (Z={}, A={}, S={}, Shards={}, B={})",
                            $z_val, $a_val, $s_val, shard_count, batch_size
                        ),
                        hist,
                    )) as Box<dyn OramBenchBackend>
                }};
            }

            match (z, a_val, s_val) {
                (64, 20, 256) => Ok(make_sharded!(64, 20, 256)),
                (16, 20, 64) => Ok(make_sharded!(16, 20, 64)),
                (64, 16, 64) => Ok(make_sharded!(64, 16, 64)),
                (64, 20, 64) => Ok(make_sharded!(64, 20, 64)),
                _ => Err(format!(
                    "Unsupported (Z={}, A={}, S={}) for sharded-oram; only canonical configurations are compiled.",
                    z, a_val, s_val
                )),
            }
        }
        "sharded-oram-resizing" => {
            let shard_count = (cores * 2).min(64).max(4);
            let batch_size = batch_size.unwrap_or_else(|| (shard_count * 512).max(4096));
            let mut rng = StdRng::seed_from_u64(seed);

            macro_rules! make_sharded_resizing {
                ($z_val:expr, $a_val:expr, $s_val:expr) => {{
                    let per_shard_quota =
                        ShardedObliviousHistogram::<$z_val, 16, $a_val, $s_val>::suggested_per_shard_quota(
                            batch_size,
                            shard_count,
                            80,
                        );
                    let mut hist = ShardedObliviousHistogram::<$z_val, 16, $a_val, $s_val>::new(
                        shard_count,
                        capacity,
                        batch_size,
                        per_shard_quota,
                        &mut rng,
                    );
                    let start_cap = hist.total_capacity();
                    let scale = ($z_val as u64 / 4).max(1);
                    let t_capacity = (start_cap * crate::LOAD_PERCENT / 100) / scale;
                    hist.enable_auto_resize(oram::AutoResizeConfig {
                        t_capacity,
                        eps: 1.0,
                        delta: 1e-6,
                        alpha: 0.05,
                        r: if $a_val == 20 && $z_val == 64 { 2 } else { 1 },
                        seed: seed ^ 0x0A00_5000_EAEA,
                    });
                    Box::new(oram_backend::OramBenchWrapper::new_sharded(
                        format!(
                            "Sharded Resizing OSAM (Z={}, A={}, S={}, B={})",
                            $z_val, $a_val, $s_val, batch_size
                        ),
                        hist,
                    )) as Box<dyn OramBenchBackend>
                }};
            }

            match (z, a_val, s_val) {
                (64, 20, 256) => Ok(make_sharded_resizing!(64, 20, 256)),
                (16, 20, 64) => Ok(make_sharded_resizing!(16, 20, 64)),
                (64, 16, 64) => Ok(make_sharded_resizing!(64, 16, 64)),
                (64, 20, 64) => Ok(make_sharded_resizing!(64, 20, 64)),
                _ => Err(format!(
                    "Unsupported (Z={}, A={}, S={}) for sharded-oram-resizing; only canonical configurations are compiled.",
                    z, a_val, s_val
                )),
            }
        }
        #[cfg(feature = "h2o2ram-baseline")]
        "h2o2ram-oram" => {
            Ok(Box::new(h2o2ram::H2O2RamBenchWrapper::new("H2O2RAM", capacity, cores as u32)))
        }
        #[cfg(feature = "h2o2ram-baseline")]
        "h2o2ram-oram-kv" => Ok(Box::new(h2o2ram::H2O2RamBenchWrapper::new_kv(
            "H2O2RAM (key+value)",
            capacity,
            cores as u32,
        ))),
        #[cfg(feature = "obliviouslabs-baseline")]
        "obliviouslabs-oram" => Ok(Box::new(obliviouslabs_oram::ParOMapBenchWrapper::new(
            "obliviouslabs/ParOMap",
            capacity as u32,
            cores as u32,
        ))),
        #[cfg(feature = "mc-oblivious-baseline")]
        "mc-oblivious-map" => Ok(Box::new(mc_oblivious::McObliviousBenchWrapper::new(
            "mc-oblivious/ObliviousMap",
            capacity,
            seed,
        ))),
        _ => Err(format!("Unknown backend: {}", name)),
    }
}

/// Configures auto-resizing OSAM parameters scaled by bucket capacity `Z`.
fn configure_resizing_osam<const Z: usize, const A: usize, const S: usize>(
    mut h: ObliviousHistogram<Z, 16, A, S>,
    seed: u64,
) -> ObliviousHistogram<Z, 16, A, S> {
    let start_cap = h.capacity();
    let scale = (Z as u64 / 4).max(1);
    let t_capacity = (start_cap * crate::LOAD_PERCENT / 100) / scale;

    let cfg = oram::AutoResizeConfig {
        t_capacity,
        eps: 1.0,
        delta: 1e-6,
        alpha: 0.05,
        r: if A == 20 && Z == 64 { 2 } else { 1 },
        seed: seed ^ 0x0A00_5000_EAEA,
    };
    h.enable_auto_resize(cfg);
    h
}
