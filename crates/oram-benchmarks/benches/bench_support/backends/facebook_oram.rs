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

#![cfg(feature = "facebook-baseline")]

use crate::bench_support::interface::{OramBenchBackend, OramOp};

/// Adapter wrapping Facebook's PathORAM baseline implementation.
pub struct FacebookOramBenchWrapper {
    pub name: String,
    pub oram: facebook_oram::PathOram<u32, 4, 8>,
    pub rng: rand::rngs::StdRng,
}

impl FacebookOramBenchWrapper {
    /// Initializes a Facebook PathORAM instance with the given capacity and random seed.
    pub fn new(name: impl Into<String>, capacity: u64, seed: u64) -> Self {
        use rand::SeedableRng;
        let mut rng = rand::rngs::StdRng::seed_from_u64(seed ^ capacity ^ 0x0A11_0A11);
        let oram =
            facebook_oram::PathOram::<u32, 4, 8>::new_with_parameters(capacity, &mut rng, 40, 1)
                .expect("failed to create PathOram");
        Self { name: name.into(), oram, rng }
    }
}

impl OramBenchBackend for FacebookOramBenchWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        use facebook_oram::Oram;
        for op in ops {
            let _ = self.oram.access(op.idx as u64, |v| v.wrapping_add(1), &mut self.rng);
        }
    }

    fn read_total(&mut self, _key: &[u8], idx: usize) -> u64 {
        use facebook_oram::Oram;
        self.oram.access(idx as u64, |v| *v, &mut self.rng).unwrap_or(0) as u64
    }
}
