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

mod bench_support;

/// Canonical benchmark configuration — the single source of truth for the ORAM
/// shape used across every benchmark.
pub const Z: usize = 64;
pub const A: usize = 20;
pub const OVERFLOW: u64 = 256;
pub const LOAD_PERCENT: u64 = 50;

/// Computes the expected distinct keys for a given capacity at standard 50% load factor.
pub const fn distinct_for_capacity(capacity: u64) -> u64 {
    capacity * LOAD_PERCENT / 100
}

/// Computes the minimum power-of-two capacity required to store `domains` distinct keys at 50% load.
pub fn capacity_for_domains(domains: u64) -> u64 {
    (domains * 100 / LOAD_PERCENT).next_power_of_two()
}

use crate::bench_support::experiments::{
    AutoTuner, BatchSizeSweep, DdosTelemetry, FixedScaling, QueryScaling, QuickSota,
    ResizeAmortized, ResizeVsFixed, SetupH2O2Ram, StateOfTheArt, ZaSweep,
};
use crate::bench_support::{
    run_subprocess_bench, BenchmarkReporter, DistributionKind, Experiment,
};

fn parse_capacity(s: &str) -> Result<u64, String> {
    let s = s.trim().to_lowercase().replace('_', "");
    if let Some(exp) = s.strip_prefix("2^") {
        let e: u32 = exp.parse().map_err(|e| format!("Invalid exponent: {e}"))?;
        return Ok(1u64 << e);
    }
    if let Some(exp) = s.strip_prefix("1<<") {
        let e: u32 = exp.parse().map_err(|e| format!("Invalid exponent: {e}"))?;
        return Ok(1u64 << e);
    }
    let s_clean = s.strip_suffix("ib").unwrap_or(&s);
    let s_clean = s_clean.strip_suffix('b').unwrap_or(s_clean);
    if let Some(num) = s_clean.strip_suffix('k') {
        let n: f64 = num.parse().map_err(|e| format!("Invalid number: {e}"))?;
        return Ok((n * 1024.0) as u64);
    }
    if let Some(num) = s_clean.strip_suffix('m') {
        let n: f64 = num.parse().map_err(|e| format!("Invalid number: {e}"))?;
        return Ok((n * 1024.0 * 1024.0) as u64);
    }
    if let Some(num) = s_clean.strip_suffix('g') {
        let n: f64 = num.parse().map_err(|e| format!("Invalid number: {e}"))?;
        return Ok((n * 1024.0 * 1024.0 * 1024.0) as u64);
    }
    s.parse::<u64>().map_err(|e| format!("Invalid capacity '{s}': {e}"))
}

fn parse_ops(s: &str) -> Result<usize, String> {
    let s = s.trim().to_lowercase().replace('_', "");
    let s_clean = s.strip_suffix("ops").unwrap_or(&s);
    let s_clean = s_clean.strip_suffix("op").unwrap_or(s_clean);
    if let Some(num) = s_clean.strip_suffix('k') {
        let n: f64 = num.parse().map_err(|e| format!("Invalid number: {e}"))?;
        return Ok((n * 1000.0) as usize);
    }
    if let Some(num) = s_clean.strip_suffix('m') {
        let n: f64 = num.parse().map_err(|e| format!("Invalid number: {e}"))?;
        return Ok((n * 1_000_000.0) as usize);
    }
    s.parse::<usize>().map_err(|e| format!("Invalid ops count '{s}': {e}"))
}

fn print_usage(experiments: &[Box<dyn Experiment>]) {
    println!("ORAM Benchmark Suite CLI");
    println!("========================");
    println!("\nUsage:");
    println!("  1. Run predefined experiment(s):");
    println!("     oram_benchmarks <experiment_name | all>");
    println!("\n  2. Run custom benchmark with parameters:");
    println!("     oram_benchmarks [--backend <names>] [--capacity <N>] [--cores <C>] [--ops <M>] [options]");
    println!("\nOptions for custom benchmarks:");
    println!("  --backend <list>       Comma-separated backends (default: oram-fixed)");
    println!("                         Available: oram-fixed, h2o2ram-oram, obliviouslabs-oram,");
    println!("                                    sharded-oram, hashmap, oram-dynamic, oram-auto");
    println!("  --capacity <list>      Capacity (e.g., 2^24, 16777216, 1M, 2^18) (default: 2^18)");
    println!("  --cores <list>         Comma-separated core counts (e.g., 1,4,16,32,64) (default: 1)");
    println!("  --ops <count>          Operations per trial (e.g., 50k, 100000, 1M) (default: 50000)");
    println!("  --distribution <kind>  Workload distribution: uniform | zipf (default: uniform)");
    println!("  --zipf <s>             Zipf parameter s (default: 1.0)");
    println!("  --z <Z>                Bucket size Z (default: 16)");
    println!("  --a <A>                Eviction interval A (default: 20)");
    println!("  --overflow <S>         Stash overflow size S (default: 64)");
    println!("  --seed <seed>          RNG seed (default: 42)");
    println!("  --csv <path>           CSV output file path (default: target/custom_bench.csv)");
    println!("\nPredefined Experiments:");
    for exp in experiments {
        println!("  - {:<20} : {}", exp.name(), exp.description());
    }
}

fn run_custom(args: &[String]) {
    let mut backends = vec!["oram-fixed".to_string()];
    let mut capacities = vec![1u64 << 18];
    let mut cores_list = vec![1usize];
    let mut ops = 50_000usize;
    let mut distribution = DistributionKind::Uniform;
    let mut zipf_s = 1.0f64;
    let mut z = Z;
    let mut evict_interval = A as u64;
    let mut overflow = OVERFLOW;
    let mut seed = 42u64;
    let mut csv_path = "target/custom_bench.csv".to_string();

    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        let (key, inline_val) = if let Some((k, v)) = arg.split_once('=') {
            (k, Some(v))
        } else {
            (arg.as_str(), None)
        };

        let mut next_val = || -> Option<String> {
            if let Some(v) = inline_val {
                Some(v.to_string())
            } else if i + 1 < args.len() {
                i += 1;
                Some(args[i].clone())
            } else {
                None
            }
        };

        match key {
            "--backend" | "-b" => {
                if let Some(val) = next_val() {
                    backends = val
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            }
            "--capacity" | "-c" => {
                if let Some(val) = next_val() {
                    capacities = val
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| parse_capacity(s).expect("Invalid capacity"))
                        .collect();
                }
            }
            "--cores" | "-t" => {
                if let Some(val) = next_val() {
                    cores_list = val
                        .split(',')
                        .map(|s| s.trim())
                        .filter(|s| !s.is_empty())
                        .map(|s| s.parse::<usize>().expect("Invalid core count"))
                        .collect();
                }
            }
            "--ops" | "-n" => {
                if let Some(val) = next_val() {
                    ops = parse_ops(&val).expect("Invalid ops count");
                }
            }
            "--distribution" | "-d" => {
                if let Some(val) = next_val() {
                    match val.trim().to_lowercase().as_str() {
                        "uniform" => distribution = DistributionKind::Uniform,
                        "zipf" => distribution = DistributionKind::Zipf,
                        other => panic!("Unknown distribution '{}', must be uniform or zipf", other),
                    }
                }
            }
            "--zipf" | "-s" => {
                if let Some(val) = next_val() {
                    zipf_s = val.trim().parse::<f64>().expect("Invalid zipf s");
                }
            }
            "--z" => {
                if let Some(val) = next_val() {
                    z = val.trim().parse::<usize>().expect("Invalid Z");
                }
            }
            "--a" => {
                if let Some(val) = next_val() {
                    evict_interval = val.trim().parse::<u64>().expect("Invalid A");
                }
            }
            "--overflow" => {
                if let Some(val) = next_val() {
                    overflow = val.trim().parse::<u64>().expect("Invalid overflow size");
                }
            }
            "--seed" => {
                if let Some(val) = next_val() {
                    seed = val.trim().parse::<u64>().expect("Invalid seed");
                }
            }
            "--csv" => {
                if let Some(val) = next_val() {
                    csv_path = val.trim().to_string();
                }
            }
            "run" | "custom" => {}
            other if other.starts_with("--") || other.starts_with('-') => {
                eprintln!("Warning: unknown option '{}'", other);
            }
            _ => {}
        }
        i += 1;
    }

    println!("\n========================================================");
    println!("  Running Custom Benchmark");
    println!("  Backends   : {:?}", backends);
    println!("  Capacities : {:?}", capacities);
    println!("  Cores      : {:?}", cores_list);
    println!("  Ops/trial  : {}", ops);
    println!("  Workload   : {:?} (zipf_s = {})", distribution, zipf_s);
    println!("========================================================");

    let mut reporter = BenchmarkReporter::new();

    for &capacity in &capacities {
        let domain_count = usize::try_from(distinct_for_capacity(capacity)).unwrap_or(usize::MAX);
        for &cores in &cores_list {
            for backend_name in &backends {
                let label = format!("{backend_name} (N={capacity}, C={cores})");
                let trace = run_subprocess_bench(
                    backend_name,
                    domain_count,
                    capacity,
                    z,
                    seed,
                    cores,
                    overflow,
                    evict_interval,
                    ops,
                    distribution,
                    zipf_s,
                    100,
                    None,
                    &label,
                );
                reporter.record(trace);
            }
        }
    }

    if let Err(e) = reporter.write_csv(&csv_path) {
        eprintln!("Error writing CSV to {}: {}", csv_path, e);
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--worker" {
        if args.len() < 3 {
            eprintln!("Missing config string for --worker");
            std::process::exit(1);
        }
        std::panic::set_hook(Box::new(|info| {
            eprintln!("WORKER PANIC: {}", info);
        }));
        bench_support::run_worker(&args[2]);
        return;
    }

    let experiments: Vec<Box<dyn Experiment>> = vec![
        Box::new(QueryScaling),
        Box::new(FixedScaling),
        Box::new(ResizeVsFixed),
        Box::new(ResizeAmortized),
        Box::new(ZaSweep),
        Box::new(StateOfTheArt),
        Box::new(BatchSizeSweep),
        Box::new(QuickSota),
        Box::new(AutoTuner),
        Box::new(DdosTelemetry),
        Box::new(SetupH2O2Ram),
    ];

    if args.len() > 1 && (args[1] == "--help" || args[1] == "-h" || args[1] == "help") {
        print_usage(&experiments);
        return;
    }

    // Strip cargo-bench harness artifacts if present
    let filtered_args: Vec<String> = args
        .iter()
        .skip(1)
        .filter(|a| *a != "--bench" && *a != "oram_benchmarks" && *a != "--")
        .cloned()
        .collect();

    // Check if custom CLI flags are passed
    let has_custom_flags = filtered_args.iter().any(|a| {
        (a.starts_with('-') && a != "--help" && a != "-h" && a != "help")
            || a == "custom"
            || a == "run"
    });

    if has_custom_flags {
        run_custom(&filtered_args);
        return;
    }

    let target = filtered_args.first().map(|s| s.as_str()).unwrap_or("all");
    let normalized_target = target.replace('-', "_");

    let mut ran_any = false;
    for exp in &experiments {
        if normalized_target == "all" || normalized_target == exp.name() {
            println!("\n========================================================");
            println!("  Running Experiment: {}", exp.name());
            println!("  Description       : {}", exp.description());
            println!("========================================================");

            let mut reporter = BenchmarkReporter::new();
            exp.run(&mut reporter);

            let csv_path = format!("benchmark_results/{}.csv", exp.name());
            if let Err(e) = reporter.write_csv(&csv_path) {
                eprintln!("Error writing CSV to {}: {}", csv_path, e);
            }
            ran_any = true;
        }
    }

    if !ran_any {
        eprintln!("Unknown experiment or argument: {}", target);
        print_usage(&experiments);
        std::process::exit(1);
    }
}
