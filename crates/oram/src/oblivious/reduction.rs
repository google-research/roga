// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory of this source tree.
// You may select, at your option, one of the above-listed licenses.

//! Oblivious run-length deduplication and reduction algorithms.

use crate::OramMetrics;

/// A trait for types that can be obliviously reduced (deduplicated).
pub trait Reducible<V = u64>: Copy {
    /// Returns `1` if the block is real, `0` if it is dummy.
    fn ct_is_real(&self) -> u8;

    /// Returns `1` if `self` and `other` have the same identity (key), `0` otherwise.
    fn ct_identity_eq(&self, other: &Self) -> u8;

    /// Returns a mutable reference to the block value.
    fn value_mut(&mut self) -> &mut V;

    /// Returns a mutable reference to the block epoch.
    fn epoch_mut(&mut self) -> &mut u8;

    /// Conditionally marks the block as dummy (if `cond == 1`).
    fn conditional_dummy(&mut self, cond: u8);
}

/// Pure accumulator trait: defines how two candidate values/buffers combine.
/// Clients using the ORAM should define their custom accumulators implementing this trait.
pub trait ObliviousAccumulator<V = u64>: Copy + Send + Sync + 'static {
    /// Combines value `a` and value `b` into a new candidate value.
    fn combine(a: V, b: V) -> V;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct SumAccumulator<V = u64>(std::marker::PhantomData<V>);

use crate::OramValue;

impl<V: OramValue> ObliviousAccumulator<V> for SumAccumulator<V> {
    #[inline]
    fn combine(a: V, b: V) -> V {
        a + b
    }
}

/// Obliviously reduces runs of identical elements, adding values and writing dummy status.
/// Works on any type implementing `Reducible`.
pub fn reduce_equal_runs<V: OramValue, T: Reducible<V>>(blocks: &mut [T]) -> OramMetrics {
    reduce_equal_runs_with_accumulator::<V, T, SumAccumulator<V>>(blocks)
}

/// Obliviously reduces runs of identical elements using a custom `ObliviousAccumulator`.
pub fn reduce_equal_runs_with_accumulator<
    V: OramValue,
    T: Reducible<V>,
    A: ObliviousAccumulator<V>,
>(
    blocks: &mut [T],
) -> OramMetrics {
    use cmov::Cmov as _;

    #[allow(unused_mut)]
    let mut metrics = OramMetrics::default();
    if blocks.is_empty() {
        return metrics;
    }

    {
        let _t = crate::timing_scope!(&mut metrics.merge_detail_reduce);
        let mut acc_val: V = V::default();
        let mut acc_epoch: u8 = 0u8;

        let (first, rest) = blocks.split_first_mut().unwrap();
        let first_real = first.ct_is_real();
        let v = *first.value_mut();
        let e = *first.epoch_mut();

        acc_val.cmovnz(&v, first_real);
        acc_epoch.cmovnz(&e, first_real);
        first.value_mut().cmovnz(&acc_val, first_real);
        first.epoch_mut().cmovnz(&acc_epoch, first_real);

        let mut prev = first;
        for cur in rest.iter_mut() {
            let v = *cur.value_mut();
            let e = *cur.epoch_mut();
            let cur_real = cur.ct_is_real();

            let same = cur.ct_identity_eq(prev) & cur_real & prev.ct_is_real();
            let start_new = cur_real & crate::ct::ct_not(same);
            let combined_val = A::combine(acc_val, v);
            let min_epoch = crate::ct::ct_min_u8(acc_epoch, e);

            acc_val.cmovnz(&combined_val, same);
            acc_val.cmovnz(&v, start_new);
            acc_epoch.cmovnz(&min_epoch, same);
            acc_epoch.cmovnz(&e, start_new);

            cur.value_mut().cmovnz(&acc_val, cur_real);
            cur.epoch_mut().cmovnz(&acc_epoch, cur_real);
            prev.conditional_dummy(same);

            prev = cur;
        }
    }
    metrics
}
