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
use crate::capacity_for_domains;

pub struct QuickSota;

impl Experiment for QuickSota {
    fn name(&self) -> &'static str {
        "quick_sota"
    }

    fn description(&self) -> &'static str {
        "Quick debugging benchmark: HashMap (1 core), Path OSAM (1 core), and Sharded OSAM (16 and 32 cores)."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let ops = 100_000usize;
        let domains = 262_144usize;
        let core_budgets = [1usize, 4, 16, 32, 64];
        let distribution = DistributionKind::Uniform;
        let zipf_s = 1.0f64;
        let seed = 42u64;
        let verify_sample = 100usize;

        let capacity = capacity_for_domains(domains as u64);

        let configs: &[(&str, bool, usize, u64, u64, &str)] = &[
            ("hashmap", false, 16, 20, 64, "Raw HashMap (Non-Oblivious)"),
            ("oram-fixed", false, 16, 20, 64, "Path OSAM (Z=16, A=20)"),
            ("oram-fixed", false, 64, 16, 64, "Path OSAM (Z=64, A=16, Safe)"),
            ("sharded-oram", true, 16, 20, 64, "Sharded OSAM (Z=16, A=20)"),
            ("sharded-oram", true, 64, 16, 64, "Sharded OSAM (Z=64, A=16, Safe)"),
            ("sharded-oram", true, 64, 20, 256, "Sharded OSAM (Z=64, A=20, r=2, Safe)"),
            #[cfg(feature = "obliviouslabs-baseline")]
            ("obliviouslabs-oram", true, 16, 20, 64, "obliviouslabs/ParOMap"),
            #[cfg(feature = "h2o2ram-baseline")]
            ("h2o2ram-oram", true, 16, 20, 64, "H2O2RAM"),
        ];

        for &cores in &core_budgets {
            for &(backend_name, multi_core, cfg_z, cfg_a, cfg_overflow, label) in configs {
                if !multi_core && cores != 1 {
                    continue;
                }
                if multi_core
                    && cores == 1
                    && backend_name != "h2o2ram-oram"
                    && backend_name != "obliviouslabs-oram"
                {
                    continue;
                }

                let trace = run_subprocess_bench(
                    backend_name,
                    domains,
                    capacity,
                    cfg_z,
                    seed,
                    cores,
                    cfg_overflow,
                    cfg_a,
                    ops,
                    distribution,
                    zipf_s,
                    verify_sample,
                    None,
                    label,
                );
                reporter.record(trace);
            }
        }
    }
}
