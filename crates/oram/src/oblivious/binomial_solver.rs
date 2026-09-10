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
        return 1.0;
    }
    if k > n || p <= 0.0 {
        return 0.0;
    }
    if p >= 1.0 {
        return 1.0;
    }

    // Sum the upper tail directly. Computing it as `1 - cdf` loses every
    // probability below roughly 2^-53 to floating-point cancellation, which
    // is not sufficient for the 80-bit sharding parameters used by ROGA.
    let mut term = binom_pmf(n, k, p);
    let mut sum = 0.0;
    let mut correction = 0.0;
    for i in k..=n {
        // Kahan summation keeps the small tail terms from being discarded.
        let adjusted = term - correction;
        let next_sum = sum + adjusted;
        correction = (next_sum - sum) - adjusted;
        sum = next_sum;

        if i == n {
            break;
        }
        term *= ((n - i) as f64 / (i + 1) as f64) * (p / (1.0 - p));
        if term == 0.0 || (term < sum * f64::EPSILON && i >= (n as f64 * p) as usize) {
            break;
        }
    }
    sum.min(1.0)
}

/// Direct lower-tail sum `Pr[X <= k]` for a point below the binomial mode.
/// This avoids both cancellation and an O(n) traversal from zero.
fn binom_lower_tail(n: usize, k: usize, p: f64) -> f64 {
    if k >= n || p <= 0.0 {
        return 1.0;
    }
    if p >= 1.0 {
        return 0.0;
    }

    let mut term = binom_pmf(n, k, p);
    let mut sum = 0.0;
    let mut correction = 0.0;
    for i in (0..=k).rev() {
        let adjusted = term - correction;
        let next_sum = sum + adjusted;
        correction = (next_sum - sum) - adjusted;
        sum = next_sum;

        if i == 0 {
            break;
        }
        term *= (i as f64 / (n - i + 1) as f64) * ((1.0 - p) / p);
        if term == 0.0 || (term < sum * f64::EPSILON && i <= (n as f64 * p) as usize) {
            break;
        }
    }
    sum.min(1.0)
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
    if p >= 1.0 {
        return n;
    }
    if p <= 0.0 {
        return 0;
    }
    let start = ((n as f64 * p - 3.0 * (n as f64 * p * (1.0 - p)).sqrt()) as usize).min(n);
    let mut cdf = binom_cdf(n, start, p);
    let mut k = start;
    let n_f = n as f64;
    let ln_gamma_n_plus_1 = ln_gamma(n_f + 1.0);
    let ln_p = p.ln();
    let ln_1_minus_p = (1.0 - p).ln();
    while k < n && cdf < alpha {
        k += 1;
        let k_f = k as f64;
        let ln_comb = ln_gamma_n_plus_1 - ln_gamma(k_f + 1.0) - ln_gamma(n_f - k_f + 1.0);
        let pmf = (ln_comb + k_f * ln_p + (n_f - k_f) * ln_1_minus_p).exp();
        cdf += pmf;
    }
    k.saturating_sub(1)
}

pub fn dp_det_threshold(
    t: f64,
    l: f64,
    k: f64,
    alpha: f64,
    a: f64,
    attempts: f64,
) -> f64 {
    let exact_margin = t - (l / k) * (binom_ppf(t as usize, k / l, alpha) as f64);
    t - (1.0 + attempts) * a * k - exact_margin
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
    // Bound every shard on both sides of the common mean. If all shard loads
    // lie in [mean-lower, mean+upper], no shard can be more than
    // lower+upper ahead of the coordinator. Split 2^-lambda across the 2m
    // tail events.
    let target = 2.0f64.powi(-(security_bits as i32)) * p / 2.0;
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
    let upper_slack = low.saturating_sub(per_shard_capacity);

    let mut low_deviation = 0usize;
    let mut high_deviation = per_shard_capacity;
    while low_deviation < high_deviation {
        let mid = low_deviation + (high_deviation - low_deviation) / 2;
        let cutoff = per_shard_capacity.saturating_sub(mid);
        if binom_lower_tail(total_n, cutoff, p) <= target {
            high_deviation = mid;
        } else {
            low_deviation = mid + 1;
        }
    }

    upper_slack.saturating_add(low_deviation)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_boundary_p_equals_one() {
        assert_eq!(binom_pmf(16, 16, 1.0), 1.0);
        assert_eq!(binom_pmf(16, 15, 1.0), 0.0);
        let res = dp_det_threshold(64.0, 16.0, 16.0, 0.05, 1.0, 2.0);
        assert!(!res.is_nan(), "dp_det_threshold produced NaN for p=1.0!");
    }

    #[test]
    fn upper_tail_remains_nonzero_below_machine_epsilon() {
        let tail = binom_sf(1024, 150, 1.0 / 16.0);
        assert!(tail > 0.0);
        assert!(tail < f64::EPSILON);
    }

    #[test]
    fn quota_really_meets_the_requested_eighty_bit_bound() {
        let n = 1024;
        let m = 16;
        let quota = suggested_per_shard_quota(n, m, 80);
        let target = 2.0f64.powi(-80) / m as f64;

        assert!(binom_sf(n, quota, 1.0 / m as f64) <= target);
        assert!(binom_sf(n, quota - 1, 1.0 / m as f64) > target);
    }
}
