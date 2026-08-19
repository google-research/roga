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

//! Minimal statistical and Binomial tail solvers for ORAM sizing.

pub fn ln_gamma(x: f64) -> f64 {
    let (mut x, mut adj) = (x, 0.0);
    while x < 8.0 {
        adj += x.ln();
        x += 1.0;
    }
    (x - 0.5) * x.ln() - x + 0.9189385332046727 + 1.0 / (12.0 * x) - 1.0 / (360.0 * x.powi(3)) - adj
}

pub fn binom_pmf(n: usize, k: usize, p: f64) -> f64 {
    if k > n {
        return 0.0;
    }
    if p >= 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    if p <= 0.0 {
        return if k == 0 { 1.0 } else { 0.0 };
    }
    let (n_f, k_f) = (n as f64, k as f64);
    let ln_comb = ln_gamma(n_f + 1.0) - ln_gamma(k_f + 1.0) - ln_gamma(n_f - k_f + 1.0);
    (ln_comb + k_f * p.ln() + (n_f - k_f) * (1.0 - p).ln()).exp()
}

pub fn binom_cdf(n: usize, k: usize, p: f64) -> f64 {
    let k = k.min(n);
    if p >= 1.0 {
        return if k == n { 1.0 } else { 0.0 };
    }
    if p <= 0.0 {
        return 1.0;
    }
    let n_f = n as f64;
    let ln_gamma_n_plus_1 = ln_gamma(n_f + 1.0);
    let ln_p = p.ln();
    let ln_1_minus_p = (1.0 - p).ln();

    (0..=k)
        .map(|i| {
            let i_f = i as f64;
            let ln_comb = ln_gamma_n_plus_1 - ln_gamma(i_f + 1.0) - ln_gamma(n_f - i_f + 1.0);
            (ln_comb + i_f * ln_p + (n_f - i_f) * ln_1_minus_p).exp()
        })
        .sum::<f64>()
        .min(1.0)
}

pub fn binom_sf(n: usize, k: usize, p: f64) -> f64 {
    if k == 0 {
        1.0
    } else if k > n {
        0.0
    } else {
        1.0 - binom_cdf(n, k - 1, p)
    }
}

pub fn suggested_per_shard_quota(
    batch_size: usize,
    shard_count: usize,
    security_bits: usize,
) -> usize {
    let p = 1.0 / (shard_count as f64);
    let target = 2.0f64.powi(-(security_bits as i32)) * p;
    let mut low = batch_size / shard_count;
    let mut high = batch_size;
    while low < high {
        let mid = low + (high - low) / 2;
        if binom_sf(batch_size, mid, p) <= target {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low
}

pub fn binom_ppf(n: usize, p: f64, alpha: f64) -> usize {
    let start = ((n as f64 * p - 3.0 * (n as f64 * p * (1.0 - p)).sqrt()) as usize).min(n);
    let mut cdf = binom_cdf(n, start, p);
    let mut k = start;
    while k < n && cdf < alpha {
        k += 1;
        cdf += binom_pmf(n, k, p);
    }
    k.saturating_sub(1)
}

pub fn dp_det_threshold(t: f64, l: f64, k: f64, alpha: f64, r: f64) -> f64 {
    let exact_margin = t - (l / k) * (binom_ppf(t as usize, k / l, alpha) as f64);
    t - 2.0 * r * k - exact_margin
}

/// Computes the inter-shard slack Delta_m such that with probability 1 - 2^-lambda,
/// no shard exceeds the coordinator's target by more than Delta_m.
pub fn shard_coordination_slack(
    per_shard_capacity: usize,
    shard_count: usize,
    security_bits: usize,
) -> usize {
    if shard_count <= 1 {
        return 0;
    }
    let m = shard_count as f64;
    let p = 1.0 / m;
    let total_n = per_shard_capacity * shard_count;
    let target = 2.0f64.powi(-(security_bits as i32)) * p;
    let mut low = per_shard_capacity;
    let mut high = total_n;
    while low < high {
        let mid = low + (high - low) / 2;
        if binom_sf(total_n, mid, p) <= target {
            high = mid;
        } else {
            low = mid + 1;
        }
    }
    low.saturating_sub(per_shard_capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_p_equals_one() {
        assert_eq!(binom_pmf(16, 16, 1.0), 1.0);
        assert_eq!(binom_pmf(16, 15, 1.0), 0.0);
        let res = dp_det_threshold(64.0, 16.0, 16.0, 0.05, 1.0);
        assert!(!res.is_nan(), "dp_det_threshold produced NaN for p=1.0!");
    }
}
