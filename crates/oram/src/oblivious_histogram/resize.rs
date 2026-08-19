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

use super::routing::MAX_TREE_HEIGHT;
use super::tree::{leaf_count, TreeIndex};
use super::ObliviousHistogram;

/// Differentially private auto-resize configuration parameters ($(\epsilon, \delta)$ privacy).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AutoResizeConfig {
    /// Initial distinct-key capacity estimate for baseline `Z=4` bucket width.
    pub t_capacity: u64,
    /// Differential privacy epsilon parameter ($\epsilon$).
    pub eps: f64,
    /// Differential privacy delta parameter ($\delta$).
    pub delta: f64,
    /// Failure probability bound for the Chernoff margin ($\alpha$).
    pub alpha: f64,
    /// Stream items per flush ($r$).
    pub r: u64,
    /// Seed for Laplace-noise RNG generation.
    pub seed: u64,
}

impl AutoResizeConfig {
    /// Creates a new configuration with default privacy parameters ($\epsilon=1.0, \delta=10^{-6}, \alpha=0.05, r=1$).
    pub fn new(t_capacity: u64) -> Self {
        Self { t_capacity, eps: 1.0, delta: 1e-6, alpha: 0.05, r: 1, seed: 0 }
    }
}

pub(crate) type AutoResizeRng = ChaCha20Rng;

#[derive(Debug, Clone)]
pub(crate) struct AutoResizeState {
    pub(crate) config: AutoResizeConfig,
    pub(crate) k: u64,
    pub(crate) t_hat: f64,
    pub(crate) flushes_since_check: u64,
    pub(crate) c_sum: u64,
    pub(crate) last_estimate: f64,
    pub(crate) total_resizes: u64,
    pub(crate) deferred: bool,
    pub(crate) rng: AutoResizeRng,
}

impl AutoResizeState {
    pub(crate) fn record_flush(&mut self, matching_blocks: u64) {
        self.c_sum += matching_blocks;
        self.flushes_since_check += 1;
    }

    pub(crate) fn should_check_epoch(&self) -> bool {
        self.flushes_since_check >= self.k
    }

    pub(crate) fn evaluate_signal<const Z: usize>(&mut self, height: u64) -> Option<u64> {
        let l = leaf_count(height);
        let effective_t = effective_t_capacity::<Z>(self.config.t_capacity);
        let (l_f, k_f) = (l as f64, self.k as f64);

        let e_t = (l_f / k_f) * (self.c_sum as f64);
        self.last_estimate = e_t;

        let b = 6.0 * l_f / (k_f * self.config.eps);
        let eta = laplace_sample(b, &mut self.rng);
        let n_half = (effective_t as f64) / 2.0;
        let do_grow = (n_half + e_t + eta) > self.t_hat;

        self.c_sum = 0;
        self.flushes_since_check = 0;

        do_grow.then_some(effective_t)
    }

    pub(crate) fn reset_counters(&mut self) {
        self.c_sum = 0;
        self.flushes_since_check = 0;
    }

    pub(crate) fn on_tree_grown<const Z: usize, const A: usize>(
        &mut self,
        new_height: u64,
        _fill_target: u64,
    ) {
        self.total_resizes += 1;
        self.config.t_capacity = self.config.t_capacity.saturating_mul(2);

        let l_new = leaf_count(new_height);
        let effective_t = effective_t_capacity::<Z>(self.config.t_capacity);
        self.k = dp_initial_k(effective_t, l_new, A as u64);
        self.t_hat = dp_epoch(
            effective_t,
            l_new,
            self.k,
            self.config.eps,
            self.config.delta,
            self.config.alpha,
            A as u64,
            &mut self.rng,
        );
    }
}

pub(crate) fn laplace_sample(b: f64, rng: &mut AutoResizeRng) -> f64 {
    if b <= 0.0 {
        return 0.0;
    }
    // Sample u in (-0.5, 0.5) excluding endpoints to avoid ln(0.0) -> -inf
    let u: f64 = loop {
        let val = rng.random::<f64>() - 0.5;
        if val.abs() < 0.5 {
            break val;
        }
    };
    -b * u.signum() * (1.0_f64 - 2.0_f64 * u.abs()).ln()
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
pub(crate) fn dp_epoch(
    t_capacity: u64,
    l: u64,
    k: u64,
    eps: f64,
    _delta: f64,
    alpha: f64,
    a: u64,
    rng: &mut AutoResizeRng,
) -> f64 {
    let (t, l_f, k_f, a_f) = (t_capacity as f64, l as f64, k as f64, a as f64);
    let b = 6.0 * l_f / (k_f * eps);
    let zeta = b * (0.5 / alpha).ln();
    let theta = laplace_sample(b, rng);
    dp_det_threshold(t, l_f, k_f, alpha, a_f) + theta - 2.0 * zeta
}

pub(crate) use crate::oblivious::binomial_solver::dp_det_threshold;

use crate::OramValue;

impl<const Z: usize, const K: usize, const A: usize, const S: usize, V: OramValue>
    ObliviousHistogram<Z, K, A, S, V>
{
    /// Enables differentially private auto-resize for the histogram ($O(1)$).
    ///
    /// Periodically evaluates stash collision signals with Laplace noise, doubling capacity when threshold is exceeded.
    pub fn enable_auto_resize(&mut self, cfg: AutoResizeConfig) {
        self.configure_auto_resize(cfg, false);
    }

    pub(crate) fn enable_deferred_auto_resize(&mut self, cfg: AutoResizeConfig) {
        self.configure_auto_resize(cfg, true);
    }

    fn configure_auto_resize(&mut self, cfg: AutoResizeConfig, defer_checks: bool) {
        let l = leaf_count(self.height);
        let effective_t = effective_t_capacity::<Z>(cfg.t_capacity);
        let k = dp_initial_k(effective_t, l, A as u64);
        let mut rng = AutoResizeRng::seed_from_u64(cfg.seed);
        let t_hat = dp_epoch(effective_t, l, k, cfg.eps, cfg.delta, cfg.alpha, A as u64, &mut rng);

        self.auto_resize = Some(AutoResizeState {
            config: cfg,
            k,
            t_hat,
            flushes_since_check: 0,
            c_sum: 0,
            last_estimate: 0.0,
            total_resizes: 0,
            deferred: defer_checks,
            rng,
        });
    }

    pub(crate) fn check_deferred_auto_resize_signal(&mut self) -> Option<u64> {
        self.auto_resize.as_ref()?;
        debug_assert!(
            self.auto_resize.as_ref().map_or(false, |a| a.deferred),
            "deferred auto-resize checks should only be polled in deferred mode"
        );
        if self.should_check_auto_resize() {
            self.check_auto_resize_signal()
        } else {
            None
        }
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
        self.physical_memory.reserve_exact(new_len - self.physical_memory.len());
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
            auto.record_flush(c_t);
        }
    }

    pub(super) fn check_and_maybe_resize_post_eviction(&mut self) {
        if self.auto_resize.as_ref().map_or(false, |a| !a.deferred)
            && self.should_check_auto_resize()
        {
            self.check_and_maybe_resize();
        }
    }
}
