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

#[cfg(feature = "h2o2ram-baseline")]
use crate::bench_support::backends::h2o2ram::H2O2RamBenchWrapper;

pub struct SetupH2O2Ram;

impl Experiment for SetupH2O2Ram {
    fn name(&self) -> &'static str {
        "setup_h2o2ram"
    }

    fn description(&self) -> &'static str {
        "Precomputes all optimal H2O2RAM hash plans for powers-of-two capacities (2^12 to 2^24) and stores them in config files."
    }

    fn run(&self, _reporter: &mut BenchmarkReporter) {
        #[cfg(feature = "h2o2ram-baseline")]
        {
            println!("Ensuring H2O2RAM_SETUP_PLANS is set for profiling...");
            std::env::set_var("H2O2RAM_SETUP_PLANS", "1");

            let capacities: Vec<u64> = (12..=24).map(|pow| 1 << pow).collect();
            for &capacity in &capacities {
                println!(
                    "Profiling & precomputing H2O2RAM hash plans for capacity: {} (2^{})",
                    capacity,
                    capacity.ilog2()
                );
                let _wrapper = H2O2RamBenchWrapper::new("H2O2RAM setup", capacity, 1);
                println!("  -> Plan successfully recorded for capacity {}.", capacity);
            }
            println!(
                "All required H2O2RAM plans have been precomputed and stored in config files!"
            );
        }
        #[cfg(not(feature = "h2o2ram-baseline"))]
        {
            println!("Feature 'h2o2ram-baseline' not enabled. Skipping setup.");
        }
    }
}
