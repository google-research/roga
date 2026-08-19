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

/// Expands supported bucket capacities (`Z` in 4, 8, 16, 32, 64) at compile time to satisfy const generics.
macro_rules! match_z {
    ($z:expr, $mac:ident) => {
        match $z {
            4 => $mac!(4),
            8 => $mac!(8),
            16 => $mac!(16),
            32 => $mac!(32),
            64 => $mac!(64),
            _ => return Err(format!("Unsupported Z={}", $z)),
        }
    };
    ($z:expr, $mac:ident, $($args:tt)*) => {
        match $z {
            4 => $mac!(4, $($args)*),
            8 => $mac!(8, $($args)*),
            16 => $mac!(16, $($args)*),
            32 => $mac!(32, $($args)*),
            64 => $mac!(64, $($args)*),
            _ => return Err(format!("Unsupported Z={}", $z)),
        }
    };
}

macro_rules! match_a {
    ($a:expr, $mac:ident, $($args:tt)*) => {
        match $a {
            2 => $mac!(2, $($args)*),
            3 => $mac!(3, $($args)*),
            4 => $mac!(4, $($args)*),
            5 => $mac!(5, $($args)*),
            6 => $mac!(6, $($args)*),
            7 => $mac!(7, $($args)*),
            8 => $mac!(8, $($args)*),
            9 => $mac!(9, $($args)*),
            10 => $mac!(10, $($args)*),
            11 => $mac!(11, $($args)*),
            12 => $mac!(12, $($args)*),
            13 => $mac!(13, $($args)*),
            14 => $mac!(14, $($args)*),
            15 => $mac!(15, $($args)*),
            16 => $mac!(16, $($args)*),
            18 => $mac!(18, $($args)*),
            20 => $mac!(20, $($args)*),
            22 => $mac!(22, $($args)*),
            24 => $mac!(24, $($args)*),
            26 => $mac!(26, $($args)*),
            28 => $mac!(28, $($args)*),
            30 => $mac!(30, $($args)*),
            32 => $mac!(32, $($args)*),
            36 => $mac!(36, $($args)*),
            40 => $mac!(40, $($args)*),
            44 => $mac!(44, $($args)*),
            48 => $mac!(48, $($args)*),
            52 => $mac!(52, $($args)*),
            56 => $mac!(56, $($args)*),
            60 => $mac!(60, $($args)*),
            64 => $mac!(64, $($args)*),
            72 => $mac!(72, $($args)*),
            80 => $mac!(80, $($args)*),
            _ => return Err(format!("Unsupported A={}", $a)),
        }
    };
}

macro_rules! match_s {
    ($s:expr, $mac:ident, $($args:tt)*) => {
        match $s {
            64 => $mac!(64, $($args)*),
            256 => $mac!(256, $($args)*),
            _ => return Err(format!("Unsupported S={}", $s)),
        }
    };
}

macro_rules! match_z_a_s {
    ($z:expr, $a:expr, $s:expr, $mac:ident) => {{
        macro_rules! dispatch_s {
            ($s_val:expr, $z_val:expr, $a_val:expr) => {
                $mac!($z_val, $a_val, $s_val)
            };
        }
        macro_rules! dispatch_a {
            ($a_val:expr, $z_val:expr) => {
                match_s!($s, dispatch_s, $z_val, $a_val)
            };
        }
        macro_rules! dispatch_z {
            ($z_val:expr) => {
                match_a!($a, dispatch_a, $z_val)
            };
        }
        match_z!($z, dispatch_z)
    }};
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

            Ok(match_z_a_s!(z, a_val, s_val, make_fixed))
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

            Ok(match_z_a_s!(z, a_val, s_val, make_resizing))
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

            Ok(match_z_a_s!(z, a_val, s_val, make_sharded))
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

            Ok(match_z_a_s!(z, a_val, s_val, make_sharded_resizing))
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
