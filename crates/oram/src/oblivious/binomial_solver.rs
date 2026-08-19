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
    (0..=k.min(n)).map(|i| binom_pmf(n, i, p)).sum::<f64>().min(1.0)
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
    (batch_size / shard_count..=batch_size)
        .find(|&q| binom_sf(batch_size, q, p) <= target)
        .unwrap_or(batch_size)
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
    let q = (per_shard_capacity..=total_n)
        .find(|&q| binom_sf(total_n, q, p) <= target)
        .unwrap_or(per_shard_capacity);
    q.saturating_sub(per_shard_capacity)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_p_equals_one() {
        println!("binom_pmf(16, 16, 1.0) = {}", binom_pmf(16, 16, 1.0));
        println!("binom_pmf(16, 15, 1.0) = {}", binom_pmf(16, 15, 1.0));
        let res = dp_det_threshold(64.0, 16.0, 16.0, 0.05, 1.0);
        println!("dp_det_threshold(64, 16, 16, 0.05, 1.0) = {res}");
        assert!(!res.is_nan(), "dp_det_threshold produced NaN for p=1.0!");
    }

    #[test]
    fn test_solver_performance_ranges() {
        let start = std::time::Instant::now();
        let res_med = dp_det_threshold(65536.0, 16384.0, 1024.0, 0.05, 1.0);
        println!("dp_det_threshold(65536) = {res_med} (took {:?})", start.elapsed());

        let start_quota = std::time::Instant::now();
        let q = suggested_per_shard_quota(65536, 16, 80);
        println!(
            "suggested_per_shard_quota(65536, 16, 80) = {q} (took {:?})",
            start_quota.elapsed()
        );
    }
}
