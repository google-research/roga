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
use crate::{distinct_for_capacity, OVERFLOW};
use std::collections::HashSet;

pub struct ZaSweep;

#[derive(Debug, Clone, Copy)]
struct ZaPair {
    z: usize,
    a: u64,
}

impl ZaSweep {
    fn parse_configs(raw: &str) -> Vec<ZaPair> {
        let mut configs = Vec::new();
        for group in raw.split(';') {
            if group.trim().is_empty() {
                continue;
            }
            let mut parts = group.split(':');
            let z_str = parts.next().expect("invalid group format");
            let z: usize = z_str.trim().parse().expect("invalid Z value");
            let a_list = parts.next().expect("missing eviction intervals list");
            for a_str in a_list.split(',') {
                let a: u64 = a_str.trim().parse().expect("invalid A value");
                configs.push(ZaPair { z, a });
            }
        }
        configs
    }
}

impl Experiment for ZaSweep {
    fn name(&self) -> &'static str {
        "z_a_sweep"
    }

    fn description(&self) -> &'static str {
        "Sweeps across multiple bucket sizes (Z) and eviction intervals (A)."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let capacity = 1u64 << 18; // 2^18
        let ops = 2_000_000usize;
        let configs_str = "4:2,3,4,5,6,7,8,9,10;8:4,5,6,7,8,9,10,11,12,13,14,15,16;16:8,10,12,14,16,18,20,22,24,26,28,30,32,36,40;32:16,20,24,28,32,36,40,44,48,52,56,60,64,72,80";
        let configs = Self::parse_configs(configs_str);
        let distributions = [DistributionKind::Uniform, DistributionKind::Zipf];
        let zipf_s = 1.0f64;
        let seed = 11u64;
        let verify_sample = 1_024usize;
        let overflow = OVERFLOW;

        let domain_count = usize::try_from(distinct_for_capacity(capacity)).unwrap_or(usize::MAX);

        for &dist in &distributions {
            let mut failed_zs = HashSet::new();
            for &config in &configs {
                if failed_zs.contains(&config.z) {
                    continue;
                }

                let label = format!("OSAM fixed (Z={}, A={})", config.z, config.a);

                let trace = run_subprocess_bench(
                    "oram-fixed",
                    domain_count,
                    capacity,
                    config.z,
                    seed,
                    1,
                    overflow,
                    config.a,
                    ops,
                    dist,
                    zipf_s,
                    verify_sample,
                    None,
                    &label,
                );

                if trace.correctness_result.contains("panic")
                    || trace.correctness_result.contains("failed")
                {
                    failed_zs.insert(config.z);
                }
                reporter.record(trace);
            }
        }
    }
}
