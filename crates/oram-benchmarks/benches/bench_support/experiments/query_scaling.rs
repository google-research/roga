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

pub struct QueryScaling;

impl Experiment for QueryScaling {
    fn name(&self) -> &'static str {
        "query_scaling"
    }

    fn description(&self) -> &'static str {
        "Fixed domain universe, variable number of updates. Compares HashMap, fixed-size OSAM, resizing OSAM, and fixed-size ORAM."
    }

    fn run(&self, reporter: &mut BenchmarkReporter) {
        let domain_count = 4_000_000usize;
        let query_counts = [
            1_000, 2_000, 3_000, 5_000, 7_500, 10_000, 15_000, 20_000, 30_000, 50_000, 75_000,
            100_000, 150_000, 250_000, 500_000, 750_000, 1_000_000, 1_500_000, 2_000_000,
        ];
        let distributions = [DistributionKind::Uniform, DistributionKind::Zipf];
        let powerlaw_s = 1.3f64;
        let seed = 17u64;
        let verify_sample = 1_024usize;
        let overflow = OVERFLOW;
        let evict_interval = A as u64;
        let z = Z;

        let fixed_worst_case_capacity = capacity_for_domains(domain_count as u64);
        let start_capacity = 1u64 << 16; // 2^16 = 65536

        for &dist in &distributions {
            let oracle_capacity = match dist {
                DistributionKind::Uniform => 1u64 << 21,
                DistributionKind::Zipf => 1u64 << 17,
            };

            let backends = [
                ("hashmap", fixed_worst_case_capacity, "HashMap"),
                ("oram-fixed", fixed_worst_case_capacity, "OSAM fixed"),
                ("oram-fixed-oracle", oracle_capacity, "OSAM oracle fixed"),
                ("oram-resizing", start_capacity, "OSAM resizing"),
            ];

            for &query_count in &query_counts {
                for &(backend_name, cap, label) in &backends {
                    let actual_backend = if backend_name == "oram-fixed-oracle" {
                        "oram-fixed"
                    } else {
                        backend_name
                    };
                    let effective_domains = domain_count;
                    let repeats = if query_count <= 100_000 { 7 } else { 5 };

                    let mut total_us = 0.0;
                    let mut total_elapsed = std::time::Duration::from_secs(0);
                    let mut max_stash = 0u64;
                    let mut max_rss = 0u64;
                    let mut final_cap = 0u64;
                    let mut correctness = String::new();
                    let mut last_trace = None;

                    for r in 0..repeats {
                        let rep_seed = seed.wrapping_add((r as u64) * 101);
                        let trace = run_subprocess_bench(
                            actual_backend,
                            effective_domains,
                            cap,
                            z,
                            rep_seed,
                            1,
                            overflow,
                            evict_interval,
                            query_count,
                            dist,
                            powerlaw_s,
                            verify_sample,
                            None,
                            label,
                        );
                        total_us += trace.us_per_op;
                        total_elapsed += trace.elapsed;
                        max_stash = max_stash.max(trace.peak_stash_items);
                        max_rss = max_rss.max(trace.tree_bytes);
                        final_cap = trace.final_capacity;
                        correctness = trace.correctness_result.clone();
                        last_trace = Some(trace);
                    }

                    if let Some(mut avg_trace) = last_trace {
                        avg_trace.us_per_op = total_us / (repeats as f64);
                        avg_trace.elapsed = total_elapsed / (repeats as u32);
                        avg_trace.peak_stash_items = max_stash;
                        avg_trace.tree_bytes = max_rss;
                        avg_trace.final_capacity = final_cap;
                        avg_trace.correctness_result = correctness;
                        reporter.record(avg_trace);
                    }
                }
            }
        }
    }
}
