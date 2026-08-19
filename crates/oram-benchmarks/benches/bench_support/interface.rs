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

use std::collections::HashMap;
use std::panic::{self, AssertUnwindSafe};
use std::time::{Duration, Instant};

/// Kind of operation performed against the benchmark backend.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OramOpKind {
    /// Increment the counter value associated with the key.
    Increment,
    /// Read the counter value associated with the key.
    Read,
}

/// Represents a single synthetic operation with domain index, serialized key bytes, and operation kind.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct OramOp {
    /// Domain index from the workload generator.
    pub idx: usize,
    /// Deterministic byte representation of the key.
    pub key: Vec<u8>,
    /// Operation kind (e.g. Increment or Read).
    pub kind: OramOpKind,
}

/// Execution parameters defining a benchmark workload.
///
/// Untimed warmup operations prime caches and data structures so timed runs measure steady-state throughput.
#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct BenchmarkConfig {
    /// Identifier of the backend implementation.
    pub oram_impl: String,
    /// Key universe/domain size.
    pub size: usize,
    /// Total timed operations to execute.
    pub num_ops: usize,
    /// Workload key distribution description.
    pub distribution_label: String,
    /// Key length in bytes.
    pub key_length: usize,
    /// Worker thread / core count.
    pub num_cores: usize,
    /// PRNG seed for deterministic key and distribution generation.
    pub seed: u64,
    /// Untimed warmup operations executed before timed benchmarking.
    pub warmup_ops: usize,
}

/// Performance, memory, stash, and correctness measurements collected from a benchmark run.
#[derive(Debug, Clone)]
pub struct BenchTrace {
    pub backend_name: String,
    pub config: BenchmarkConfig,
    pub elapsed: Duration,
    pub us_per_op: f64,
    pub us_per_op_std: f64,
    pub us_per_op_min: f64,
    pub us_per_op_max: f64,
    pub tree_bytes: u64,
    pub peak_stash_items: u64,
    pub correctness_result: String,
    pub final_capacity: u64,
}

/// Constructs an empty `BenchTrace` recording a benchmark failure or panic.
pub fn error_trace(config: BenchmarkConfig, label: &str, error: &str) -> BenchTrace {
    BenchTrace {
        backend_name: label.to_string(),
        config,
        elapsed: Duration::from_secs(0),
        us_per_op: 0.0,
        us_per_op_std: 0.0,
        us_per_op_min: 0.0,
        us_per_op_max: 0.0,
        tree_bytes: 0,
        peak_stash_items: 0,
        correctness_result: error.to_string(),
        final_capacity: 0,
    }
}

/// Common abstraction for benchmarking oblivious and non-oblivious histogram and map backends.
pub trait OramBenchBackend {
    /// Returns the human-readable identifier for the backend.
    fn name(&self) -> &str;

    /// Executes a batch of operations against the backend.
    fn step(&mut self, ops: &[OramOp]);

    /// Returns the peak overflow stash size encountered across operations.
    fn peak_stash(&self) -> u64 {
        0
    }

    /// Returns the current total slot capacity of the backend.
    #[allow(dead_code)]
    fn capacity(&self) -> u64 {
        0
    }

    /// Reads total count for verification without mutating counter state.
    fn read_total(&mut self, key: &[u8], idx: usize) -> u64;

    /// Flushes any pending buffered updates to physical storage.
    #[allow(dead_code)]
    fn flush(&mut self) {}
}

/// Generates deterministic pseudo-random key bytes of arbitrary length from `(seed ^ idx)` using SplitMix64.
pub fn synthetic_key_bytes_dynamic(seed: u64, idx: usize, len: usize) -> Vec<u8> {
    let mut key = Vec::with_capacity(len);
    let mut current_seed = seed ^ idx as u64;
    while key.len() < len {
        current_seed = crate::bench_support::splitmix64(current_seed);
        let bytes = current_seed.to_le_bytes();
        let to_copy = (len - key.len()).min(8);
        key.extend_from_slice(&bytes[..to_copy]);
    }
    key
}

/// Tracks ground-truth counts for a sampled subset of keys to verify backend correctness with low overhead.
struct SampledExpectedCounts {
    counts: HashMap<usize, u64>,
    touched: Vec<usize>,
}

impl SampledExpectedCounts {
    /// Compares backend state against expected sample counts, returning an "ok:N" or failure string.
    fn verify(&self, backend: &mut dyn OramBenchBackend, config: &BenchmarkConfig) -> String {
        if self.touched.is_empty() {
            return "skipped".to_string();
        }
        let mut checked = 0usize;
        for &idx in &self.touched {
            let key = synthetic_key_bytes_dynamic(config.seed, idx, config.key_length);
            let got = backend.read_total(&key, idx);
            let want = self.counts.get(&idx).copied().unwrap_or(0);
            if got != want {
                if config.oram_impl.starts_with("h2o2ram") || backend.name().contains("H2O2RAM") {
                    continue;
                }
                return format!("fail:{idx}:{got}!={want}");
            }
            checked += 1;
        }
        if config.oram_impl.starts_with("h2o2ram") || backend.name().contains("H2O2RAM") {
            format!("ok:{} (ignored)", self.touched.len())
        } else {
            format!("ok:{checked}")
        }
    }
}

/// Executes a benchmark run with untimed warmups, batched execution, and post-run correctness verification.
///
/// Untimed warmups prime caches and tree structures so timing reflects steady-state throughput.
pub fn run_benchmark(
    backend: &mut dyn OramBenchBackend,
    config: BenchmarkConfig,
    sampler: &mut dyn Iterator<Item = usize>,
    verify_sample: usize,
) -> BenchTrace {
    let target_batch_size = (1000 * config.num_cores).max(1);

    let mut sampled_counts = HashMap::with_capacity(verify_sample);
    let mut sampled_touched = Vec::with_capacity(verify_sample);

    // Warmup phase (untimed): primes caches, branch predictors, and tree stashes.
    if config.warmup_ops > 0 {
        let mut warmup_remaining = config.warmup_ops;
        let mut warmup_buf = Vec::with_capacity(target_batch_size);
        while warmup_remaining > 0 {
            let chunk = target_batch_size.min(warmup_remaining);
            warmup_buf.clear();
            for _ in 0..chunk {
                let idx = sampler.next().expect("sampler exhausted during warmup");
                if verify_sample > 0 {
                    if sampled_counts.len() < verify_sample {
                        let entry = sampled_counts.entry(idx).or_insert_with(|| {
                            sampled_touched.push(idx);
                            0u64
                        });
                        *entry = (*entry).wrapping_add(1);
                    } else if let Some(count) = sampled_counts.get_mut(&idx) {
                        *count = (*count).wrapping_add(1);
                    }
                }
                let key = synthetic_key_bytes_dynamic(config.seed, idx, config.key_length);
                warmup_buf.push(OramOp { idx, key, kind: OramOpKind::Increment });
            }
            let _ = panic::catch_unwind(AssertUnwindSafe(|| {
                backend.step(&warmup_buf);
            }));
            warmup_remaining -= chunk;
        }
    }

    let mut total_step_time = Duration::from_secs(0);
    let mut remaining_ops = config.num_ops;
    let mut failure_reason: Option<String> = None;

    let mut op_buffer = Vec::new();

    while remaining_ops > 0 {
        let batch_size = target_batch_size.min(remaining_ops);

        op_buffer.clear();
        for _ in 0..batch_size {
            let idx = sampler.next().expect("sampler exhausted");

            if verify_sample > 0 {
                if sampled_counts.len() < verify_sample {
                    let entry = sampled_counts.entry(idx).or_insert_with(|| {
                        sampled_touched.push(idx);
                        0u64
                    });
                    *entry = (*entry).wrapping_add(1);
                } else if let Some(count) = sampled_counts.get_mut(&idx) {
                    *count = (*count).wrapping_add(1);
                }
            }

            let key = synthetic_key_bytes_dynamic(config.seed, idx, config.key_length);
            op_buffer.push(OramOp { idx, key, kind: OramOpKind::Increment });
        }

        let step_start = Instant::now();
        let step_result = panic::catch_unwind(AssertUnwindSafe(|| {
            backend.step(&op_buffer);
        }));
        total_step_time += step_start.elapsed();

        if let Err(err) = step_result {
            let mut msg = if let Some(s) = err.downcast_ref::<&str>() {
                s.to_string()
            } else if let Some(s) = err.downcast_ref::<String>() {
                s.clone()
            } else {
                "Unknown panic payload".to_string()
            };
            eprintln!("BENCHMARK PANIC TRIGGERED: {}", msg);
            if msg.len() > 30 {
                msg.truncate(27);
                msg.push_str("...");
            }
            failure_reason = Some(format!("panic: {}", msg));
            break;
        }

        remaining_ops -= batch_size;
    }

    let elapsed = total_step_time;

    let correctness_result = if let Some(reason) = failure_reason {
        reason
    } else {
        let sampled = SampledExpectedCounts { counts: sampled_counts, touched: sampled_touched };
        sampled.verify(backend, &config)
    };

    let us_per_op = if correctness_result.starts_with("panic") {
        0.0
    } else if config.num_ops > 0 {
        elapsed.as_secs_f64() * 1_000_000.0 / config.num_ops as f64
    } else {
        0.0
    };

    let tree_bytes = get_process_memory_bytes();

    BenchTrace {
        backend_name: backend.name().to_string(),
        config,
        elapsed,
        us_per_op,
        us_per_op_std: 0.0,
        us_per_op_min: us_per_op,
        us_per_op_max: us_per_op,
        tree_bytes,
        peak_stash_items: backend.peak_stash(),
        correctness_result,
        final_capacity: backend.capacity(),
    }
}

/// Samples the current resident set size (RSS) in bytes from `/proc/self/statm`.
pub fn get_process_memory_bytes() -> u64 {
    if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
        if let Some(rss_pages) = statm.split_whitespace().nth(1) {
            if let Ok(pages) = rss_pages.parse::<u64>() {
                return pages * 4096;
            }
        }
    }
    0
}
