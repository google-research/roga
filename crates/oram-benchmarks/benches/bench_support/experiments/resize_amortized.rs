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
use crate::{A, OVERFLOW, Z};

pub struct ResizeAmortized;

impl Experiment for ResizeAmortized {
    fn name(&self) -> &'static str {
        "resize_amortized"
    }

    fn description(&self) -> &'static str {
        "Head-to-Head: Asynchronous Runway Resizing OSAM vs Fixed OSAM (10M Zipf s=1.3 workload)."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let domains = 16_777_216usize; // 16M open domain universe
        let ops = 10_000_000usize; // 10M requests
        let distribution = DistributionKind::Zipf;
        let powerlaw_s = 1.3f64; // Zipf s=1.3 heavy-tailed skew
        let seed = 7u64;
        let verify_sample = 1_024usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let start_capacity = 1u64 << 16; // 65,536
        let max_capacity = 1u64 << 25; // 33,554,432

        println!("\n======================================================================================================");
        println!("Head-to-Head: Asynchronous Runway Resizing OSAM vs Fixed OSAM (10M Zipf s=1.3)");
        println!("======================================================================================================");

        // 1. Asynchronous Runway Resizing OSAM
        let trace_async = run_subprocess_bench(
            "oram-resizing",
            domains,
            start_capacity,
            z,
            seed,
            1,
            overflow,
            evict_interval,
            ops,
            distribution,
            powerlaw_s,
            verify_sample,
            None,
            "OSAM Asynchronous Resizing (1 core)",
        );
        reporter.record(trace_async);

        // 2. Fixed OSAM (Pre-allocated worst-case capacity 2^25)
        let trace_fixed = run_subprocess_bench(
            "oram-fixed",
            domains,
            max_capacity,
            z,
            seed,
            1,
            overflow,
            evict_interval,
            ops,
            distribution,
            powerlaw_s,
            verify_sample,
            None,
            "OSAM Fixed (cap=2^25, 1 core)",
        );
        reporter.record(trace_fixed);
    }
}
