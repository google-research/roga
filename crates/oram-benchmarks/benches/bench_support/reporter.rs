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

use crate::bench_support::csv_field;
use crate::bench_support::interface::BenchTrace;
use std::fs::File;
use std::io::Write;
use std::path::Path;

/// Aggregates and reports benchmark execution traces for console output and CSV export.
pub struct BenchmarkReporter {
    /// Ordered list of recorded benchmark traces.
    pub traces: Vec<BenchTrace>,
}

impl Default for BenchmarkReporter {
    fn default() -> Self {
        Self::new()
    }
}

/// Formats a byte count into a human-readable MB or GB string representation.
fn format_bytes(bytes: u64) -> String {
    if bytes == 0 {
        return "N/A".to_string();
    }
    let mb = bytes as f64 / 1_048_576.0;
    if mb >= 1024.0 {
        format!("{:.1} GB", mb / 1024.0)
    } else {
        format!("{:.1} MB", mb)
    }
}

impl BenchmarkReporter {
    /// Initializes reporter and prints the aligned terminal table header.
    pub fn new() -> Self {
        println!(
            "{:<45} | {:<5} | {:>8} | {:>7} | {:>9} | {:>7} | {:>12} | {:>10} | {:<25}",
            "Backend",
            "Cores",
            "Size",
            "Ops",
            "Time (ms)",
            "us/op",
            "Memory",
            "Peak Stash",
            "Correctness"
        );
        println!("{}", "-".repeat(140));
        Self { traces: Vec::new() }
    }

    /// Formats, displays, and records an individual benchmark trace.
    pub fn record(&mut self, trace: BenchTrace) {
        let is_ok = trace.correctness_result.starts_with("ok:");
        let (time_str, us_op_str) = if is_ok {
            let us_str = if trace.us_per_op_std > 0.001 {
                format!("{:.3} ± {:.3}", trace.us_per_op, trace.us_per_op_std)
            } else {
                format!("{:.3}", trace.us_per_op)
            };
            (
                format!("{:.2}", trace.elapsed.as_secs_f64() * 1000.0),
                us_str,
            )
        } else {
            ("N/A".to_string(), "N/A".to_string())
        };
        println!(
            "{:<45} | {:<5} | {:>8} | {:>7} | {:>9} | {:>15} | {:>12} | {:>10} | {:<25}",
            trace.backend_name,
            trace.config.num_cores,
            trace.config.size,
            trace.config.num_ops,
            time_str,
            us_op_str,
            format_bytes(trace.tree_bytes),
            trace.peak_stash_items,
            trace.correctness_result
        );
        self.traces.push(trace);
    }

    /// Exports recorded benchmark traces to CSV, resolving relative paths against `BUILD_WORKING_DIRECTORY`.
    pub fn write_csv(&self, path: impl AsRef<Path>) -> std::io::Result<()> {
        let mut resolved_path = path.as_ref().to_path_buf();
        if resolved_path.is_relative() {
            if let Ok(wd) = std::env::var("BUILD_WORKING_DIRECTORY") {
                resolved_path = Path::new(&wd).join(&resolved_path);
            } else if let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") {
                let ws_root = Path::new(&manifest_dir).parent().and_then(|p| p.parent()).unwrap_or(Path::new(&manifest_dir));
                resolved_path = ws_root.join(&resolved_path);
            }
        }
        let path = &resolved_path;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(path)?;

        // Write header
        writeln!(
            file,
            "backend,cores,time_ms,us_per_op,us_per_op_std,us_per_op_min,us_per_op_max,tree_bytes,correctness,size,num_ops,distribution,seed,final_capacity,peak_stash"
        )?;

        // Write rows
        for t in &self.traces {
            writeln!(
                file,
                "{},{},{:.3},{:.6},{:.6},{:.6},{:.6},{},{},{},{},{},{},{},{}",
                csv_field(&t.backend_name),
                t.config.num_cores,
                t.elapsed.as_secs_f64() * 1000.0,
                t.us_per_op,
                t.us_per_op_std,
                t.us_per_op_min,
                t.us_per_op_max,
                t.tree_bytes,
                csv_field(&t.correctness_result),
                t.config.size,
                t.config.num_ops,
                csv_field(&t.config.distribution_label),
                t.config.seed,
                t.final_capacity,
                t.peak_stash_items
            )?;
        }

        println!("\nWrote benchmark results CSV to: {}", path.display());
        Ok(())
    }
}
