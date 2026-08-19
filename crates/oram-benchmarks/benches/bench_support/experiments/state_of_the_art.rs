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

pub struct StateOfTheArt;

impl Experiment for StateOfTheArt {
    fn name(&self) -> &'static str {
        "state_of_the_art"
    }

    fn description(&self) -> &'static str {
        "Runs comparison across multiple backends (HashMap, OSAM, Sharded OSAM, obliviouslabs/ParOMap, H2O2RAM) at various core counts."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let ops = 1_000_000usize;
        let domain_budgets = [262_144usize, 4_194_304usize];
        let core_budgets = [1usize, 4, 16, 32, 64];
        let distribution = DistributionKind::Uniform;
        let zipf_s = 1.0f64;
        let seed = 42u64;
        let verify_sample = 100usize;

        // Map backends with a flag indicating whether they scale with cores or run single-threaded (cores = 1)
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
            ("h2o2ram-oram-kv", true, 16, 20, 64, "H2O2RAM (key+value)"),
        ];

        for &domains in &domain_budgets {
            let capacity = capacity_for_domains(domains as u64);

            for &cores in &core_budgets {
                for &(backend_name, multi_core, cfg_z, cfg_a, cfg_overflow, label) in configs {
                    if !multi_core && cores != 1 {
                        // Single-threaded baseline backends only run at cores = 1
                        continue;
                    }

                    if backend_name == "sharded-oram" && cores < 2 {
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
                        None, // batch_size
                        label,
                    );
                    reporter.record(trace);
                }
            }
        }
    }
}
