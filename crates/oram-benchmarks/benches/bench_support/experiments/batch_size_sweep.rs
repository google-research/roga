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

pub struct BatchSizeSweep;

impl Experiment for BatchSizeSweep {
    fn name(&self) -> &'static str {
        "batch_size_sweep"
    }

    fn description(&self) -> &'static str {
        "Sweeps across multiple batch sizes for sharded OSAM at fixed core counts (16 and 64 cores)."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let domains = 65536usize;
        let core_budgets = [16usize, 64];
        let batch_sizes = [4096usize, 8192, 12288, 16384, 24576, 32768, 49152, 65536];
        let distribution = DistributionKind::Uniform;
        let seed = 42u64;
        let verify_sample = 100usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let capacity = capacity_for_domains(domains as u64);

        for &cores in &core_budgets {
            for &batch_size in &batch_sizes {
                let ops = batch_size * 10;
                let label = format!("Sharded OSAM (batch={batch_size}, cores={cores})");

                let trace = run_subprocess_bench(
                    "sharded-oram",
                    domains,
                    capacity,
                    z,
                    seed,
                    cores,
                    overflow,
                    evict_interval,
                    ops,
                    distribution,
                    1.0, // zipf_s (not used for Uniform, but passed)
                    verify_sample,
                    Some(batch_size),
                    &label,
                );
                reporter.record(trace);
            }
        }
    }
}
