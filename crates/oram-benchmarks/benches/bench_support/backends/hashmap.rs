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

use crate::bench_support::interface::{OramBenchBackend, OramOp};
use std::collections::HashMap;

/// Un-oblivious `HashMap` baseline representing the empirical upper bound on throughput (zero oblivious overhead).
pub struct HashMapBenchWrapper {
    pub name: String,
    pub map: HashMap<Vec<u8>, u64>,
}

impl HashMapBenchWrapper {
    /// Constructs a new `HashMapBenchWrapper` with the specified display name.
    pub fn new(name: impl Into<String>) -> Self {
        Self { name: name.into(), map: HashMap::new() }
    }
}

impl OramBenchBackend for HashMapBenchWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        for op in ops {
            *self.map.entry(op.key.clone()).or_insert(0) += 1;
        }
    }

    fn read_total(&mut self, key: &[u8], _idx: usize) -> u64 {
        self.map.get(key).copied().unwrap_or(0)
    }
}
