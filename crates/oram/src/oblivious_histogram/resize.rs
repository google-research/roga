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

//! Resize support for tree ORAMs: the differentially private resize *decision*
//! (config, state, DP math) together with the resize *mechanism* (grow + fill).

use rand::RngExt;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use crate::oblivious::ct::ct_lt;

use super::routing::MAX_TREE_HEIGHT;
use super::tree::{leaf_count, TreeIndex};
use super::ObliviousHistogram;

/// Precision used for the dyadic approximation of the geometric failure
/// probability. Leaving sixteen low RNG bits unused makes the integer
/// threshold exact while keeping the approximation error negligible.
const SAMPLER_PROBABILITY_BITS: u32 = 48;
const SAMPLER_PROBABILITY_SCALE: u64 = 1u64 << SAMPLER_PROBABILITY_BITS;
/// The probability that either fixed-work geometric draw reaches its cap is
/// bounded by `2^-SAMPLER_STATISTICAL_BITS`.
const SAMPLER_STATISTICAL_BITS: u32 = 128;
#[cfg(test)]
const EPS_ONE_FAILURE_CUTOFF: u64 = 238_263_423_799_583;
#[cfg(test)]
const EPS_ONE_TRIALS: usize = 537;
const RESIZE_ATTEMPTS: u64 = 2;
#[cfg(test)]
const EPS_ONE_RANDOM_WORDS: usize = 2 * EPS_ONE_TRIALS;

/// Differentially private auto-resize configuration parameters ($\epsilon$ privacy).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoResizeConfig {
    /// Initial distinct-key capacity estimate for baseline `Z=4` bucket width.
    pub t_capacity: u64,
    /// Differential privacy epsilon parameter ($\epsilon$).
    pub eps: f64,
    /// Failure probability bound for the Chernoff margin ($\alpha$).
    pub alpha: f64,
    /// Seed for discrete-Laplace-noise RNG generation.
    pub seed: u64,
}

impl AutoResizeConfig {
    /// Creates a new configuration with default privacy parameters ($\epsilon=1.0, \alpha=0.05$).
    pub fn new(t_capacity: u64) -> Self {
        Self {
            t_capacity,
            eps: 1.0,
            alpha: 0.05,
            seed: 0,
        }
    }
}

pub(crate) type AutoResizeRng = ChaCha20Rng;

/// Fixed-work sampler for integer-valued two-sided geometric (discrete
/// Laplace) noise. A sample is the difference of two independent geometric
/// variables, each evaluated for the same public number of trials.
#[derive(Debug, Clone, Copy)]
pub(crate) struct CtDiscreteLaplace {
    failure_cutoff: u64,
    trials: usize,
}

impl CtDiscreteLaplace {
    pub(crate) fn new(eps: f64) -> Self {
        assert!(eps.is_finite() && eps > 0.0, "epsilon must be finite and positive");

        // The target distribution has q = exp(-eps / 6). Round q upward so
        // the implemented distribution is (very slightly) more private than
        // requested. Two threshold units cover floating-point rounding in the
        // public, one-time parameter setup.
        let target_q = (-eps / 6.0).exp();
        let scaled_q = target_q * (SAMPLER_PROBABILITY_SCALE as f64);
        let failure_cutoff = (scaled_q.floor() as u64).saturating_add(2);
        assert!(
            failure_cutoff < SAMPLER_PROBABILITY_SCALE,
            "epsilon is too small for the sampler's probability precision"
        );

        let q = (failure_cutoff as f64) / (SAMPLER_PROBABILITY_SCALE as f64);
        debug_assert!(-6.0 * q.ln() <= eps);

        // If G is an uncapped geometric variable, Pr[G >= trials] = q^trials.
        // A union bound over the two variables makes the statistical distance
        // introduced by capping at most 2^-128.
        let target_log_probability = -((SAMPLER_STATISTICAL_BITS + 1) as f64) * core::f64::consts::LN_2;
        let trials = (target_log_probability / q.ln()).ceil() as usize;
        assert!(trials > 0, "sampler must perform at least one trial");

        Self { failure_cutoff, trials }
    }

    #[inline]
    fn geometric_with(&self, next_word: &mut impl FnMut() -> u64) -> i64 {
        let mut active = 1u64;
        let mut magnitude = 0u64;

        for _ in 0..self.trials {
            let draw = next_word() >> (64 - SAMPLER_PROBABILITY_BITS);
            active &= ct_lt(draw, self.failure_cutoff) as u64;
            magnitude = magnitude.wrapping_add(active);
        }

        magnitude as i64
    }

    #[inline]
    pub(crate) fn sample_with(&self, next_word: &mut impl FnMut() -> u64) -> i64 {
        let positive = self.geometric_with(next_word);
        let negative = self.geometric_with(next_word);
        positive.wrapping_sub(negative)
    }

    pub(crate) fn sample(&self, rng: &mut AutoResizeRng) -> i64 {
        self.sample_with(&mut || rng.random::<u64>())
    }

    /// Smallest non-negative integer z whose upper tail satisfies
    /// `Pr[X > z] <= probability` for the uncapped discrete distribution.
    fn upper_quantile(&self, probability: f64) -> i64 {
        assert!(
            probability.is_finite() && probability > 0.0 && probability < 0.5,
            "tail probability must be finite and in (0, 0.5)"
        );
        let q = (self.failure_cutoff as f64) / (SAMPLER_PROBABILITY_SCALE as f64);
        let z = ((probability * (1.0 + q)).ln() / q.ln()).ceil() - 1.0;
        (z.max(0.0) as i64).min(self.trials as i64)
    }

    #[cfg(test)]
    fn truncation_bound(&self) -> f64 {
        let q = (self.failure_cutoff as f64) / (SAMPLER_PROBABILITY_SCALE as f64);
        2.0 * q.powi(self.trials as i32)
    }

    #[cfg(test)]
    fn effective_epsilon(&self) -> f64 {
        let q = (self.failure_cutoff as f64) / (SAMPLER_PROBABILITY_SCALE as f64);
        -6.0 * q.ln()
    }
}

/// Runs the epsilon-one sampler constants over caller-provided RNG output so
/// the unit test can compare them with the parameterized production sampler.
#[cfg(test)]
fn discrete_laplace_eps_one_from_words(words: &[u64; EPS_ONE_RANDOM_WORDS]) -> i64 {
    let sampler = CtDiscreteLaplace {
        failure_cutoff: EPS_ONE_FAILURE_CUTOFF,
        trials: EPS_ONE_TRIALS,
    };
    let mut index = 0usize;
    sampler.sample_with(&mut || {
        let word = words[index];
        index += 1;
        word
    })
}

#[derive(Debug, Clone)]
pub(crate) struct AutoResizeState {
    pub(crate) config: AutoResizeConfig,
    pub(crate) k: u64,
    pub(crate) t_hat_units: i64,
    pub(crate) evictions_since_check: u64,
    pub(crate) c_sum: u64,
    pub(crate) total_resizes: u64,
    pub(crate) deferred: bool,
    pub(crate) emits_grow_signal: bool,
    pub(crate) pending_grow: Option<u64>,
    pub(crate) sampler: CtDiscreteLaplace,
    pub(crate) rng: AutoResizeRng,
}

impl AutoResizeState {
    pub(crate) fn record_eviction(&mut self, matching_blocks: u64) {
        self.c_sum += matching_blocks;
        self.evictions_since_check += 1;
    }

    pub(crate) fn should_check_epoch(&self) -> bool {
        (!self.emits_grow_signal || self.pending_grow.is_none()) && self.evictions_since_check >= self.k
    }

    pub(crate) fn evaluate_signal<const Z: usize>(&mut self, _height: u64) -> Option<u64> {
        let effective_t = effective_t_capacity::<Z>(self.config.t_capacity);

        // Divide the estimator inequality by its public sensitivity L/k.
        // c_sum and the noise are consequently both integer estimator units.
        let eta = self.sampler.sample(&mut self.rng);
        let noisy_estimate_units = (self.c_sum as i128) + (eta as i128);
        let do_grow = noisy_estimate_units > (self.t_hat_units as i128);

        self.c_sum = 0;
        self.evictions_since_check = 0;

        do_grow.then_some(effective_t)
    }

    pub(crate) fn reset_counters(&mut self) {
        self.c_sum = 0;
        self.evictions_since_check = 0;
        self.pending_grow = None;
    }

    pub(crate) fn on_tree_grown<const Z: usize, const A: usize>(&mut self, new_height: u64, _fill_target: u64) {
        self.total_resizes += 1;
        self.config.t_capacity = self.config.t_capacity.saturating_mul(2);

        let l_new = leaf_count(new_height);
        let effective_t = effective_t_capacity::<Z>(self.config.t_capacity);
        self.k = dp_initial_k(effective_t, l_new, A as u64);
        self.t_hat_units = dp_epoch_units(effective_t, l_new, self.k, self.config.alpha, A as u64, &self.sampler, &mut self.rng);
    }
}

pub(crate) fn dp_initial_k(t_capacity: u64, l: u64, a: u64) -> u64 {
    let a_f = (a as f64).max(1.0);
    let raw = ((t_capacity as f64) * (l as f64) / (a_f * a_f)).cbrt();
    let k = (raw.floor() as u64).max(1);
    k.min(l)
}

pub(crate) fn effective_t_capacity<const Z: usize>(t_capacity: u64) -> u64 {
    let scale = (Z as u64 / 4).max(1);
    t_capacity.saturating_mul(scale)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn dp_epoch_units(t_capacity: u64, l: u64, k: u64, alpha: f64, a: u64, sampler: &CtDiscreteLaplace, rng: &mut AutoResizeRng) -> i64 {
    let (t, l_f, k_f, a_f) = (t_capacity as f64, l as f64, k as f64, a as f64);
    let sensitivity = l_f / k_f;
    let deterministic_units = ((dp_det_threshold(t, l_f, k_f, alpha, a_f, RESIZE_ATTEMPTS as f64) - t / 2.0) / sensitivity).floor() as i64;
    let estimator_zeta_units = sampler.upper_quantile(alpha);
    let attempts_tail = (2.0 * alpha).powi(RESIZE_ATTEMPTS as i32);
    let attempts_zeta_units = sampler.upper_quantile(attempts_tail);
    let theta_units = sampler.sample(rng);
    deterministic_units
        .wrapping_add(theta_units)
        .wrapping_sub(estimator_zeta_units)
        .wrapping_sub(attempts_zeta_units)
}

pub(crate) use crate::oblivious::binomial_solver::dp_det_threshold;

use crate::OramValue;

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue> ObliviousHistogram<Z, K, A, S, V> {
    /// Enables differentially private auto-resize for the histogram ($O(1)$).
    ///
    /// Periodically evaluates stash collision signals with discrete Laplace noise, doubling capacity when threshold is exceeded.
    pub fn enable_auto_resize(&mut self, cfg: AutoResizeConfig) {
        self.configure_auto_resize(cfg, false, true);
    }

    pub(crate) fn enable_deferred_auto_resize(&mut self, cfg: AutoResizeConfig, emits_grow_signal: bool) {
        self.configure_auto_resize(cfg, true, emits_grow_signal);
    }

    fn configure_auto_resize(&mut self, cfg: AutoResizeConfig, defer_checks: bool, emits_grow_signal: bool) {
        let l = leaf_count(self.height);
        let effective_t = effective_t_capacity::<Z>(cfg.t_capacity);
        let k = dp_initial_k(effective_t, l, A as u64);
        let mut rng = AutoResizeRng::seed_from_u64(cfg.seed);
        let sampler = CtDiscreteLaplace::new(cfg.eps);
        let t_hat_units = dp_epoch_units(effective_t, l, k, cfg.alpha, A as u64, &sampler, &mut rng);

        self.auto_resize = Some(AutoResizeState {
            config: cfg,
            k,
            t_hat_units,
            evictions_since_check: 0,
            c_sum: 0,
            total_resizes: 0,
            deferred: defer_checks,
            emits_grow_signal,
            pending_grow: None,
            sampler,
            rng,
        });
    }

    pub(crate) fn take_deferred_auto_resize_signal(&mut self) -> Option<u64> {
        debug_assert!(
            self.auto_resize.as_ref().is_some_and(|auto| auto.deferred && auto.emits_grow_signal),
            "deferred auto-resize signals should only be consumed in deferred mode"
        );
        self.auto_resize.as_mut()?.pending_grow.take()
    }

    pub(crate) fn apply_deferred_auto_resize_grow(&mut self, fill_target: u64) {
        debug_assert!(
            self.auto_resize.as_ref().map_or(false, |a| a.deferred),
            "deferred auto-resize growth should only be forced in deferred mode"
        );
        if let Some(auto) = self.auto_resize.as_mut() {
            auto.reset_counters();
        }
        self.apply_auto_resize_grow(fill_target);
    }

    pub(crate) fn check_and_maybe_resize(&mut self) {
        #[cfg(feature = "profile")]
        let start = std::time::Instant::now();

        if let Some(fill_target) = self.check_auto_resize_signal() {
            self.apply_auto_resize_grow(fill_target);
        }

        #[cfg(feature = "profile")]
        {
            self.metrics.resize_check += start.elapsed();
        }
    }

    pub(crate) fn should_check_auto_resize(&self) -> bool {
        self.auto_resize.as_ref().is_some_and(|auto| auto.should_check_epoch())
    }

    pub(crate) fn check_auto_resize_signal(&mut self) -> Option<u64> {
        let height = self.height;
        self.auto_resize.as_mut()?.evaluate_signal::<Z>(height)
    }

    fn grow_physical_storage(&mut self) -> u64 {
        assert!(self.height < MAX_TREE_HEIGHT, "cannot grow past MAX_TREE_HEIGHT");
        let old_height = self.height;
        let new_len = self.physical_memory.len() * 2;
        self.physical_memory
            .reserve_exact(new_len + S - self.physical_memory.len());
        self.physical_memory.resize(new_len, crate::OramBlock::<K, V>::dummy());
        self.height += 1;
        self.epoch = self.epoch.saturating_add(1);
        self.sweep_end = self.evict_ctr + leaf_count(old_height);
        self.stash.grow_extend_path_buffer();
        self.height
    }

    fn apply_auto_resize_grow(&mut self, fill_target: u64) {
        #[cfg(feature = "profile")]
        let start = std::time::Instant::now();

        let new_height = self.grow_physical_storage();

        if let Some(auto) = self.auto_resize.as_mut() {
            auto.on_tree_grown::<Z, A>(new_height, fill_target);
        }

        #[cfg(feature = "profile")]
        {
            self.metrics.resize_check += start.elapsed();
        }
    }

    /// Doubles tree capacity. This resize is visible to the storage server.
    pub fn grow(&mut self) {
        let new_height = self.grow_physical_storage();
        if let Some(auto) = self.auto_resize.as_mut() {
            let target = effective_t_capacity::<Z>(auto.config.t_capacity);
            auto.on_tree_grown::<Z, A>(new_height, target);
        }

        let migrated_leaves = leaf_count(new_height);

        for new_leaf in migrated_leaves..leaf_count(new_height + 1) {
            self.stash.read_from_path(&mut self.physical_memory, new_leaf);
            self.stash.merge_accumulate_for_path(new_leaf);
            self.stash.write_to_path(&mut self.physical_memory, new_leaf);
        }
    }

    pub(super) fn accumulate_resize_accounting(&mut self, evict_leaf: TreeIndex) {
        let _t = crate::timing_scope!(&mut self.metrics.resize_accounting);
        if let Some(auto) = self.auto_resize.as_mut() {
            let h = self.height;
            let current_epoch = self.epoch;
            let c_t = self.stash.count_matching_blocks(h, evict_leaf, current_epoch);
            auto.record_eviction(c_t);
        }
    }

    pub(super) fn check_and_maybe_resize_post_eviction(&mut self) {
        if !self.should_check_auto_resize() {
            return;
        }

        if self.auto_resize.as_ref().is_some_and(|auto| auto.deferred) {
            let height = self.height;
            let auto = self.auto_resize.as_mut().expect("auto-resize state disappeared");
            let grow_signal = auto.evaluate_signal::<Z>(height);
            if auto.emits_grow_signal {
                auto.pending_grow = grow_signal;
            }
        } else {
            self.check_and_maybe_resize();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discrete_laplace_parameters_are_conservative() {
        let sampler = CtDiscreteLaplace::new(1.0);

        assert!(sampler.effective_epsilon() <= 1.0);
        assert!(sampler.truncation_bound() <= 2.0f64.powi(-(SAMPLER_STATISTICAL_BITS as i32)));
        assert_eq!(sampler.failure_cutoff, EPS_ONE_FAILURE_CUTOFF);
        assert_eq!(sampler.trials, EPS_ONE_TRIALS);
    }

    #[test]
    fn discrete_laplace_always_consumes_the_fixed_number_of_words() {
        let sampler = CtDiscreteLaplace::new(1.0);

        for constant_word in [0, u64::MAX] {
            let mut words = 0usize;
            let sample = sampler.sample_with(&mut || {
                words += 1;
                constant_word
            });
            assert_eq!(words, 2 * sampler.trials);
            assert_eq!(sample, 0);
        }

        let mut words = 0usize;
        let positive_cap = sampler.sample_with(&mut || {
            words += 1;
            if words <= sampler.trials {
                0
            } else {
                u64::MAX
            }
        });
        assert_eq!(words, 2 * sampler.trials);
        assert_eq!(positive_cap, sampler.trials as i64);

        let mut words = 0usize;
        let negative_cap = sampler.sample_with(&mut || {
            words += 1;
            if words <= sampler.trials {
                u64::MAX
            } else {
                0
            }
        });
        assert_eq!(words, 2 * sampler.trials);
        assert_eq!(negative_cap, -(sampler.trials as i64));
    }

    #[test]
    fn discrete_laplace_quantile_bounds_the_upper_tail() {
        let sampler = CtDiscreteLaplace::new(1.0);
        let probability = 0.05;
        let z = sampler.upper_quantile(probability);
        let q = (sampler.failure_cutoff as f64) / (SAMPLER_PROBABILITY_SCALE as f64);

        assert!(q.powi((z + 1) as i32) / (1.0 + q) <= probability);
        if z > 0 {
            assert!(q.powi(z as i32) / (1.0 + q) > probability);
        }
    }

    #[test]
    fn discrete_laplace_seed_is_deterministic_and_non_degenerate() {
        let sampler = CtDiscreteLaplace::new(1.0);
        let mut first = AutoResizeRng::seed_from_u64(19);
        let mut second = AutoResizeRng::seed_from_u64(19);
        let first_samples: Vec<_> = (0..32).map(|_| sampler.sample(&mut first)).collect();
        let second_samples: Vec<_> = (0..32).map(|_| sampler.sample(&mut second)).collect();

        assert_eq!(first_samples, second_samples);
        assert!(first_samples.iter().any(|&sample| sample < 0));
        assert!(first_samples.iter().any(|&sample| sample > 0));
    }

    #[test]
    fn verification_core_matches_the_epsilon_one_sampler() {
        let sampler = CtDiscreteLaplace::new(1.0);
        let mut rng = AutoResizeRng::seed_from_u64(23);
        let mut words = [0u64; EPS_ONE_RANDOM_WORDS];
        for word in &mut words {
            *word = rng.random::<u64>();
        }
        let mut words_iter = words.iter().copied();

        assert_eq!(
            discrete_laplace_eps_one_from_words(&words),
            sampler.sample_with(&mut || words_iter.next().unwrap())
        );
    }
}
