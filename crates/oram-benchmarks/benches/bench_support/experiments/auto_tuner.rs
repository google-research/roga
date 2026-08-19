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

use crate::bench_support::experiment::Experiment;
use crate::bench_support::reporter::BenchmarkReporter;
use crate::bench_support::{run_subprocess_bench, DistributionKind};
use crate::{capacity_for_domains, A, OVERFLOW, Z};

pub struct AutoTuner;

impl Experiment for AutoTuner {
    fn name(&self) -> &'static str {
        "auto_tuner"
    }

    fn description(&self) -> &'static str {
        "Automated Grid-Search Auto-Tuner: Sweeps over Shard Counts (S), Frontend Counts (F), and Batch Sizes (B) to find the global optimal Sharded OSAM configuration."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let domains = 65536usize;
        let distribution = DistributionKind::Uniform;
        let seed = 42u64;
        let verify_sample = 100usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let capacity = capacity_for_domains(domains as u64);

        let shard_counts = [16usize, 32, 64];
        let batch_sizes = [16384usize, 32768, 65536];

        println!("\n=== Starting Automated Sharded OSAM Grid Search Auto-Tuning ===");
        println!("Fixed Ring ORAM Parameters: Z={}, A={}", z, evict_interval);
        println!("Testing Shard Counts: {:?}", shard_counts);
        println!("Testing Batch Sizes : {:?}", batch_sizes);
        println!("---------------------------------------------------------------");

        for &shard_count in &shard_counts {
            for &batch_size in &batch_sizes {
                let flushes = 10;
                let ops = batch_size * flushes;
                let label = format!("Sharded OSAM Auto-Tune (S={shard_count}, B={batch_size})");

                let trace = run_subprocess_bench(
                    "sharded-oram",
                    domains,
                    capacity,
                    z,
                    seed,
                    shard_count,
                    overflow,
                    evict_interval,
                    ops,
                    distribution,
                    1.0,
                    verify_sample,
                    Some(batch_size),
                    &label,
                );
                reporter.record(trace);
            }
        }
    }
}
