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

use crate::bench_support::interface::{OramBenchBackend, OramOp};
use oram::{ObliviousHistogram, ShardedObliviousHistogram};

/// Holds either a single-tree or multi-threaded sharded oblivious histogram instance.
pub enum BenchHistogram<const Z: usize, const K: usize, const A: usize = 20, const S: usize = 64> {
    /// Single-tree oblivious histogram instance.
    Single(ObliviousHistogram<Z, K, A, S>),
    /// Sharded oblivious histogram with background worker threads and routing frontends.
    Sharded(ShardedObliviousHistogram<Z, K, A, S>),
}

/// Adapter implementing `OramBenchBackend` for single-tree and sharded oblivious histograms.
pub struct OramBenchWrapper<const Z: usize, const K: usize, const A: usize = 20, const S: usize = 64> {
    pub name: String,
    pub hist: BenchHistogram<Z, K, A, S>,
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize> OramBenchWrapper<Z, K, A, S> {
    /// Wraps a single-tree oblivious histogram backend.
    pub fn new_single(name: impl Into<String>, hist: ObliviousHistogram<Z, K, A, S>) -> Self {
        Self { name: name.into(), hist: BenchHistogram::Single(hist) }
    }

    /// Wraps a sharded oblivious histogram backend.
    pub fn new_sharded(name: impl Into<String>, hist: ShardedObliviousHistogram<Z, K, A, S>) -> Self {
        Self { name: name.into(), hist: BenchHistogram::Sharded(hist) }
    }
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize> OramBenchBackend
    for OramBenchWrapper<Z, K, A, S>
{
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        match &mut self.hist {
            BenchHistogram::Single(h) => {
                for op in ops {
                    let mut key = [0u8; K];
                    if op.key.len() != K {
                        panic!("Key length mismatch: expected {}, got {}", K, op.key.len());
                    }
                    key.copy_from_slice(&op.key);
                    h.append(&key, 1);
                }
            }
            BenchHistogram::Sharded(h) => {
                for op in ops {
                    let mut key = [0u8; K];
                    if op.key.len() != K {
                        panic!("Key length mismatch: expected {}, got {}", K, op.key.len());
                    }
                    key.copy_from_slice(&op.key);
                    h.append(&key, 1);
                }
                h.flush();
            }
        }
    }

    fn read_total(&mut self, key: &[u8], _idx: usize) -> u64 {
        let mut k = [0u8; K];
        k.copy_from_slice(key);
        match &mut self.hist {
            BenchHistogram::Single(h) => h.read_total(&k),
            BenchHistogram::Sharded(h) => h.read_total(&k),
        }
    }

    fn peak_stash(&self) -> u64 {
        match &self.hist {
            BenchHistogram::Single(h) => h.peak_overflow(),
            BenchHistogram::Sharded(h) => h.peak_overflow(),
        }
    }

    fn capacity(&self) -> u64 {
        match &self.hist {
            BenchHistogram::Single(h) => h.capacity(),
            BenchHistogram::Sharded(h) => h.total_capacity(),
        }
    }
}

impl<const Z: usize, const K: usize, const A: usize, const S: usize> Drop
    for OramBenchWrapper<Z, K, A, S>
{
    fn drop(&mut self) {
        #[cfg(feature = "profile")]
        match &self.hist {
            BenchHistogram::Single(h) => {
                let m = h.metrics();
                eprintln!("\n=== Profiling Breakdown: {} ===", self.name);
                eprintln!("  key_hash              : {:?}", m.key_hash);
                eprintln!("  insert                : {:?}", m.insert);
                eprintln!("  read_path             : {:?}", m.read_path);
                eprintln!("  merge_accumulate      : {:?}", m.merge_accumulate);
                eprintln!("    └─ merge_sort       : {:?}", m.merge_detail_sort);
                eprintln!("    └─ merge_reduce     : {:?}", m.merge_detail_reduce);
                eprintln!("  write_to_path         : {:?}", m.write_to_path);
                eprintln!("    └─ setup            : {:?}", m.write_to_path_setup);
                eprintln!("    └─ assign_real   : {:?}", m.write_to_path_assign_real);
                eprintln!("    └─ compact          : {:?}", m.write_to_path_compact);
                eprintln!("    └─ copy_back        : {:?}", m.write_to_path_copy_back);
            }
            BenchHistogram::Sharded(h) => {
                let sm = h.metrics();
                eprintln!("\n=== Profiling Breakdown: {} ===", self.name);
                eprintln!("  Total Flushes         : {}", sm.flush_count);
                eprintln!("  Total Flush Wall Time : {:?}", sm.total_flush_time);
                eprintln!("  1. prepare_work       : {:?}", sm.prepare_work);
                eprintln!("  2. sort_reduce_compact: {:?}", sm.compact);
                eprintln!("  3. load_count         : {:?}", sm.load_count);
                eprintln!("  4. distribute         : {:?}", sm.distribute);
                eprintln!("  5. parallel_shards    : {:?}", sm.parallel_shard_process_time);

                let mut agg = oram::OramMetrics::default();
                for s in &h.router.shards {
                    agg += s.metrics();
                }
                eprintln!("  [Per-Shard Aggregated Eviction Metrics (Sum across shards)]");
                eprintln!("     insert             : {:?}", agg.insert);
                eprintln!("     read_path          : {:?}", agg.read_path);
                eprintln!("     merge_accumulate   : {:?}", agg.merge_accumulate);
                eprintln!("       └─ sort          : {:?}", agg.merge_detail_sort);
                eprintln!("       └─ reduce        : {:?}", agg.merge_detail_reduce);
                eprintln!("     write_to_path      : {:?}", agg.write_to_path);
                eprintln!("       └─ setup         : {:?}", agg.write_to_path_setup);
                eprintln!("       └─ assign_real   : {:?}", agg.write_to_path_assign_real);
                eprintln!("       └─ compact       : {:?}", agg.write_to_path_compact);
                eprintln!("       └─ copy_back     : {:?}", agg.write_to_path_copy_back);
            }
        }
    }
}
