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

pub mod interface;

pub mod backends;

pub mod experiment;
pub use experiment::Experiment;

pub mod reporter;
pub use reporter::BenchmarkReporter;

pub mod subprocess;
pub use subprocess::{run_subprocess_bench, run_worker};

pub mod experiments;

use rand::{rngs::StdRng, RngExt, SeedableRng};
use rand_distr::{Distribution, Zipf};

const UNIFORM_SEED: u64 = 0x00C0_FFEE;
const ZIPF_SEED: u64 = 0x0005_17E5_EED5;

use serde::{Deserialize, Serialize};

/// Workload key access distribution kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DistributionKind {
    /// Uniformly distributed access across all keys.
    Uniform,
    /// Power-law Zipf-distributed access skewing toward popular keys.
    Zipf,
}

impl DistributionKind {
    /// Returns a human-readable label representing the distribution and its skew parameter.
    pub fn label(self, s: f64) -> String {
        match self {
            Self::Uniform => "uniform".to_string(),
            Self::Zipf => format!("zipf_s={s}"),
        }
    }
}

/// 64-bit SplitMix pseudo-random generator function for deterministic key and seed mixing.
pub fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Escapes strings containing commas, quotes, or newlines for safe CSV serialization.
pub fn csv_field(value: &str) -> String {
    if value.contains([',', '"', '\n']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

/// Deterministic uniform pseudo-random index generator across the key domain.
pub struct UniformSampler {
    domain_count: usize,
    rng: StdRng,
}

impl UniformSampler {
    /// Creates a uniform index sampler seeded with `(seed ^ UNIFORM_SEED)`.
    pub fn new(domain_count: usize, seed: u64) -> Self {
        let rng = StdRng::seed_from_u64(seed ^ UNIFORM_SEED);
        Self { domain_count, rng }
    }
}

impl Iterator for UniformSampler {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        Some(self.rng.random_range(0..self.domain_count))
    }
}

/// Deterministic Zipf-distributed pseudo-random index generator modeling skewed power-law key access.
pub struct ZipfSampler {
    rng: StdRng,
    zipf: Zipf<f64>,
}

impl ZipfSampler {
    /// Creates a Zipf index sampler seeded with `(seed ^ ZIPF_SEED)`.
    pub fn new(domain_count: usize, zipf_s: f64, seed: u64) -> Self {
        let rng = StdRng::seed_from_u64(seed ^ ZIPF_SEED);
        let zipf = Zipf::new(domain_count as f64, zipf_s).expect("invalid Zipf parameters");
        Self { rng, zipf }
    }
}

impl Iterator for ZipfSampler {
    type Item = usize;
    fn next(&mut self) -> Option<Self::Item> {
        Some(self.zipf.sample(&mut self.rng) as usize - 1)
    }
}

/// Factory constructing an index iterator for the specified workload distribution.
pub fn make_sampler(
    distribution: DistributionKind,
    domain_count: usize,
    seed: u64,
    zipf_s: f64,
) -> Box<dyn Iterator<Item = usize>> {
    match distribution {
        DistributionKind::Uniform => Box::new(UniformSampler::new(domain_count, seed)),
        DistributionKind::Zipf => Box::new(ZipfSampler::new(domain_count, zipf_s, seed)),
    }
}
