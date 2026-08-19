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

use crate::bench_support::backends::create_backend;
use crate::bench_support::interface::{error_trace, run_benchmark, BenchTrace, BenchmarkConfig};
use crate::bench_support::{make_sampler, DistributionKind};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Serializable configuration passed from the benchmark coordinator to a worker subprocess.
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkerInput {
    pub backend_name: String,
    pub domains: usize,
    pub capacity: u64,
    pub z: usize,
    pub seed: u64,
    pub cores: usize,
    pub overflow: u64,
    pub evict_interval: u64,
    pub ops: usize,
    pub warmup_ops: usize,
    pub distribution: DistributionKind,
    pub zipf_s: f64,
    pub verify_sample: usize,
    pub batch_size: Option<usize>,
}

/// Serializable performance metrics emitted by a worker subprocess on stdout (`RESULT|...`).
#[derive(Serialize, Deserialize, Debug)]
pub struct WorkerOutput {
    pub elapsed_ms: f64,
    pub us_per_op: f64,
    pub peak_stash_items: u64,
    pub peak_rss_bytes: u64,
    pub correctness_result: String,
    pub final_capacity: u64,
}

/// Reads peak resident set size from `VmHWM` (High Water Mark) in `/proc/self/status`.
///
/// VmHWM captures the true maximum physical memory occupied across the process lifetime.
fn get_peak_rss_bytes() -> u64 {
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if line.starts_with("VmHWM:") {
                let parts: Vec<&str> = line.split_whitespace().collect();
                if parts.len() >= 2 {
                    if let Ok(kb) = parts[1].parse::<u64>() {
                        return kb * 1024;
                    }
                }
            }
        }
    }
    0
}

#[cfg(target_os = "linux")]
mod affinity {
    extern "C" {
        fn sched_setaffinity(pid: i32, cpusetsize: usize, mask: *const u64) -> i32;
    }

    /// Binds the calling thread to a specific physical CPU core.
    pub fn pin_current_thread_to_core(core_index: usize) {
        let mut mask = [0u64; 16]; // 1024-bit cpu_set_t
        let word = (core_index / 64) % 16;
        let bit = core_index % 64;
        mask[word] = 1u64 << bit;
        unsafe {
            let _ = sched_setaffinity(0, std::mem::size_of_val(&mask), mask.as_ptr());
        }
    }
}

/// Worker entry point executed inside an isolated child process.
///
/// Configures Rayon thread pool with CPU core pinning, executes benchmark operations, measures peak RSS, and prints JSON results.
pub fn run_worker(config_json: &str) {
    let input: WorkerInput =
        serde_json::from_str(config_json).expect("Worker failed to deserialize input config");

    if input.cores > 1 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(input.cores)
            .start_handler(|thread_idx| {
                #[cfg(target_os = "linux")]
                affinity::pin_current_thread_to_core(thread_idx);
            })
            .build_global();
    } else {
        #[cfg(target_os = "linux")]
        affinity::pin_current_thread_to_core(0);
    }

    let mut backend = match create_backend(
        &input.backend_name,
        input.capacity,
        input.z,
        input.seed,
        input.cores,
        input.overflow,
        input.evict_interval,
        input.batch_size,
    ) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("Worker failed to create backend: {}", e);
            std::process::exit(1);
        }
    };

    let mut sampler = make_sampler(
        input.distribution,
        input.domains,
        input.seed ^ input.domains as u64,
        input.zipf_s,
    );

    let config = BenchmarkConfig {
        oram_impl: input.backend_name.clone(),
        size: input.domains,
        num_ops: input.ops,
        distribution_label: input.distribution.label(input.zipf_s),
        key_length: 16,
        num_cores: input.cores,
        seed: input.seed,
        warmup_ops: input.warmup_ops,
    };

    let trace = run_benchmark(&mut *backend, config, &mut sampler, input.verify_sample);

    let peak_rss = get_peak_rss_bytes();

    let output = WorkerOutput {
        elapsed_ms: trace.elapsed.as_secs_f64() * 1000.0,
        us_per_op: trace.us_per_op,
        peak_stash_items: trace.peak_stash_items,
        peak_rss_bytes: peak_rss,
        correctness_result: trace.correctness_result,
        final_capacity: trace.final_capacity,
    };

    println!("RESULT|{}", serde_json::to_string(&output).unwrap());
}

/// Executes benchmark trials in isolated child subprocesses and aggregates statistics across runs.
///
/// Worker subprocesses isolate allocator state and memory leaks between backends and trials.
#[allow(clippy::too_many_arguments)]
pub fn run_subprocess_bench(
    backend_name: &str,
    domains: usize,
    capacity: u64,
    z: usize,
    seed: u64,
    cores: usize,
    overflow: u64,
    evict_interval: u64,
    ops: usize,
    distribution: DistributionKind,
    zipf_s: f64,
    verify_sample: usize,
    batch_size: Option<usize>,
    label: &str,
) -> BenchTrace {
    let warmup_ops = if backend_name.contains("resizing") {
        0
    } else {
        10_000.min(ops / 10)
    };
    let num_trials = std::env::var("ORAM_BENCH_TRIALS")
        .ok()
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(10);
    let mut trial_outputs: Vec<WorkerOutput> = Vec::with_capacity(num_trials);

    let config = BenchmarkConfig {
        oram_impl: label.to_string(),
        size: domains,
        num_ops: ops,
        distribution_label: distribution.label(zipf_s),
        key_length: 16,
        num_cores: cores,
        seed,
        warmup_ops,
    };

    for trial in 0..num_trials {
        let trial_seed = seed.wrapping_add((trial as u64) * 1000);
        let input = WorkerInput {
            backend_name: backend_name.to_string(),
            domains,
            capacity,
            z,
            seed: trial_seed,
            cores,
            overflow,
            evict_interval,
            ops,
            warmup_ops,
            distribution,
            zipf_s,
            verify_sample,
            batch_size,
        };

        let config_json = serde_json::to_string(&input).expect("failed to serialize worker input");
        let mut cmd = std::process::Command::new("/usr/bin/taskset");
        let max_core = (cores.max(1) - 1).min(63);
        cmd.arg("-c").arg(format!("0-{}", max_core));
        cmd.arg(std::env::current_exe().unwrap());
        cmd.arg("--worker").arg(config_json);
        cmd.env("OMP_NUM_THREADS", "1");
        cmd.env("TBB_NUM_THREADS", "1");

        let output = cmd.output().expect("failed to execute worker");
        let stderr = String::from_utf8_lossy(&output.stderr);
        if !stderr.is_empty() {
            eprintln!("--- Worker Stderr ({}, trial {}) ---\n{}", label, trial + 1, stderr);
        }
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return error_trace(config, label, &format!("worker_failed: {}", stderr.trim()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut parsed_result = None;

        for line in stdout.lines() {
            if line.starts_with("RESULT|") {
                let json_str = &line["RESULT|".len()..];
                match serde_json::from_str::<WorkerOutput>(json_str) {
                    Ok(worker_out) => parsed_result = Some(worker_out),
                    Err(e) => eprintln!("FAILED TO PARSE WORKER JSON: {}\nRAW JSON: {}", e, json_str),
                }
            } else {
                println!("{}", line);
            }
        }

        if let Some(out) = parsed_result {
            if !out.correctness_result.starts_with("ok:") {
                return BenchTrace {
                    backend_name: label.to_string(),
                    config,
                    elapsed: Duration::from_secs_f64(out.elapsed_ms / 1000.0),
                    us_per_op: out.us_per_op,
                    us_per_op_std: 0.0,
                    us_per_op_min: out.us_per_op,
                    us_per_op_max: out.us_per_op,
                    tree_bytes: out.peak_rss_bytes,
                    peak_stash_items: out.peak_stash_items,
                    correctness_result: out.correctness_result,
                    final_capacity: out.final_capacity,
                };
            }
            trial_outputs.push(out);
        } else {
            return error_trace(config, label, "failed_to_parse_worker_output");
        }
    }

    if trial_outputs.is_empty() {
        return error_trace(config, label, "no_trials_completed");
    }

    // Statistical aggregation across trials
    let n = trial_outputs.len();
    let mut us_list: Vec<f64> = trial_outputs.iter().map(|o| o.us_per_op).collect();
    us_list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    let median_us = if n % 2 == 1 {
        us_list[n / 2]
    } else {
        (us_list[n / 2 - 1] + us_list[n / 2]) / 2.0
    };
    let min_us = *us_list.first().unwrap_or(&0.0);
    let max_us = *us_list.last().unwrap_or(&0.0);

    let mean_us: f64 = us_list.iter().sum::<f64>() / n as f64;
    let variance: f64 = us_list.iter().map(|&x| (x - mean_us).powi(2)).sum::<f64>() / n as f64;
    let stddev_us = variance.sqrt();

    let mut elapsed_list: Vec<f64> = trial_outputs.iter().map(|o| o.elapsed_ms).collect();
    elapsed_list.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let median_elapsed_ms = elapsed_list[n / 2];

    let max_stash = trial_outputs.iter().map(|o| o.peak_stash_items).max().unwrap_or(0);
    let max_rss = trial_outputs.iter().map(|o| o.peak_rss_bytes).max().unwrap_or(0);
    let final_cap = trial_outputs.last().map(|o| o.final_capacity).unwrap_or(0);
    let correctness = trial_outputs.last().map(|o| o.correctness_result.clone()).unwrap_or_default();

    BenchTrace {
        backend_name: label.to_string(),
        config,
        elapsed: Duration::from_secs_f64(median_elapsed_ms / 1000.0),
        us_per_op: median_us,
        us_per_op_std: stddev_us,
        us_per_op_min: min_us,
        us_per_op_max: max_us,
        tree_bytes: max_rss,
        peak_stash_items: max_stash,
        correctness_result: correctness,
        final_capacity: final_cap,
    }
}
