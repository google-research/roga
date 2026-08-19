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

use crate::bench_support::reporter::BenchmarkReporter;

/// Defines a reproducible benchmark experiment suite.
pub trait Experiment {
    /// Unique CLI name for the experiment (e.g. "query_scaling").
    fn name(&self) -> &'static str;
    /// Human-readable summary of the experiment's goal and parameters.
    fn description(&self) -> &'static str;
    /// Executes the experiment trials and records measurements to `reporter`.
    fn run(&self, reporter: &mut BenchmarkReporter);
}
