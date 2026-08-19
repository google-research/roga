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

pub struct ResizeVsFixed;

impl Experiment for ResizeVsFixed {
    fn name(&self) -> &'static str {
        "resize_vs_fixed"
    }

    fn description(&self) -> &'static str {
        "Compares performance of resizing OSAM vs fixed capacity OSAM and ORAM baseline."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let domains = 4_000_000usize;
        let ops = 500_000usize;
        let distributions = [DistributionKind::Uniform, DistributionKind::Zipf];
        let powerlaw_s = 1.3f64;
        let seed = 7u64;
        let verify_sample = 1_024usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        // Worst-case fixed capacity based on domain universe (4M -> 2^23)
        let fixed_worst_case_capacity = capacity_for_domains(domains as u64);
        let start_capacity = 1u64 << 16; // 2^16 = 65536

        for &dist in &distributions {
            // Realized distinct keys at 2M ops: ~1.6M for uniform (-> 2^21), ~70k for Zipf s=1.3 (-> 2^17)
            let oracle_capacity = match dist {
                DistributionKind::Uniform => 1u64 << 21,
                DistributionKind::Zipf => 1u64 << 17,
            };

            let backends = [
                ("hashmap", fixed_worst_case_capacity, "HashMap"),
                ("oram-fixed", fixed_worst_case_capacity, "OSAM fixed (worst-case)"),
                ("oram-fixed", oracle_capacity, "OSAM fixed (oracle)"),
                ("oram-resizing", start_capacity, "OSAM resizing"),
            ];

            for &(backend_name, cap, label) in &backends {
                let effective_domains = domains;

                let trace = run_subprocess_bench(
                    backend_name,
                    effective_domains,
                    cap,
                    z,
                    seed,
                    1,
                    overflow,
                    evict_interval,
                    ops,
                    dist,
                    powerlaw_s,
                    verify_sample,
                    None, // batch_size
                    label,
                );
                reporter.record(trace);
            }
        }
    }
}
