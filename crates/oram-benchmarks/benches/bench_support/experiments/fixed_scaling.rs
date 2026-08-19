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
use crate::{distinct_for_capacity, A, OVERFLOW, Z};

pub struct FixedScaling;

impl Experiment for FixedScaling {
    fn name(&self) -> &'static str {
        "fixed_scaling"
    }

    fn description(&self) -> &'static str {
        "Runs a fixed-size runtime scaling benchmark for HashMap, fixed-size OSAM, and fixed-size ORAM baselines."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let capacities = [
            1 << 16,
            1 << 18,
            1 << 20,
            1 << 22,
            1 << 24,
            1 << 26,
        ];
        let distributions = [DistributionKind::Uniform];
        let powerlaw_s = 1.0f64;
        let seed = 11u64;
        let verify_sample = 1_024usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let backends = [
            "hashmap",
            "oram-fixed",
            #[cfg(feature = "obliviouslabs-baseline")]
            "obliviouslabs-oram",
            #[cfg(feature = "h2o2ram-baseline")]
            "h2o2ram-oram",
        ];

        for &dist in &distributions {
            for &capacity_hint in &capacities {
                let capacity = (capacity_hint as u64).next_power_of_two();
                let domain_count =
                    usize::try_from(distinct_for_capacity(capacity)).unwrap_or(usize::MAX);
                // Run 100,000 operations for fast, responsive head-to-head comparison
                let ops = 100_000usize;

                for &backend_name in &backends {
                    if (backend_name == "obliviouslabs-oram" || backend_name == "h2o2ram-oram")
                        && capacity < 65536
                    {
                        continue;
                    }
                    // H2O2RAM config table is precomputed up to 2^26 (67108864)
                    if backend_name == "h2o2ram-oram" && capacity > 67108864 {
                        continue;
                    }

                    let label = match backend_name {
                        "oram-fixed" => "OSAM fixed".to_string(),
                        "obliviouslabs-oram" => "obliviouslabs/ParOMap".to_string(),
                        "h2o2ram-oram" => "H2O2RAM".to_string(),
                        _ => backend_name.to_string(),
                    };

                    let trace = run_subprocess_bench(
                        backend_name,
                        domain_count,
                        capacity,
                        z,
                        seed,
                        1,
                        overflow,
                        evict_interval,
                        ops,
                        dist,
                        powerlaw_s,
                        verify_sample,
                        None,
                        &label,
                    );
                    reporter.record(trace);
                }
            }
        }
    }
}
