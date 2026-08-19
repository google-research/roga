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

pub struct DdosTelemetry;

impl Experiment for DdosTelemetry {
    fn name(&self) -> &'static str {
        "ddos_telemetry"
    }

    fn description(&self) -> &'static str {
        "Case Study: Streaming Network DDoS & Telemetry Analysis. Evaluates throughput, batch deduplication speedup under attack bursts, and DP auto-resizing memory efficiency."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let ops = 500_000usize;
        let domains = 131_072usize;
        let core_budgets = [1usize, 2, 4, 8, 16, 32, 64];
        let seed = 42u64;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let capacity = capacity_for_domains(domains as u64);

        println!("\n--- Part 1: Streaming Ingestion Scaling on Network Traffic (Zipf s=1.2) ---");
        for &cores in &core_budgets {
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
                DistributionKind::Zipf,
                1.2,
                100,
                None,
                "Sharded OSAM (Network Traffic Zipf s=1.2)",
            );
            reporter.record(trace);
        }

        println!("\n--- Part 2: Burst & Attack Simulation (Zipf s=2.5 - Heavy DDoS Flood) ---");
        for &cores in &[1usize, 4, 16, 64] {
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
                DistributionKind::Zipf,
                2.5, // Heavy skew simulating DDoS burst targeting top victim IPs
                100,
                None,
                "Sharded OSAM (DDoS Attack Burst Zipf s=2.5)",
            );
            reporter.record(trace);
        }
    }
}
