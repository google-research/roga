// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Generic block-address routing helpers for tree ORAMs.

use super::tree::TreeIndex;

/// Maximum supported ORAM binary tree height (31 levels).
pub const MAX_TREE_HEIGHT: u64 = 31;
pub(crate) const REAL_ADDR_MARKER: u64 = 1u64 << MAX_TREE_HEIGHT;
pub(crate) const ROUTING_ADDR_MASK: u64 = (REAL_ADDR_MARKER << 1) - 1;
pub(crate) const ROUTING_BITS_MASK: u64 = REAL_ADDR_MARKER - 1;
pub(crate) const HIGH_IDENTITY_BITS_MASK: u64 = !ROUTING_ADDR_MASK;

/// Maps a 64-bit PRF routing address to a tree leaf index at the given tree height ($O(1)$).
pub fn address_to_leaf(addr: u64, height: u64) -> TreeIndex {
    let routing = (addr & ROUTING_BITS_MASK) | REAL_ADDR_MARKER;
    routing >> (MAX_TREE_HEIGHT - height)
}

/// Overwrites the top routing prefix bits of `addr` to match `leaf` at the specified height ($O(1)$).
pub fn force_leaf_routing(addr: &mut u64, leaf: TreeIndex, height: u64) {
    let mut raw = *addr;
    assert!(height <= MAX_TREE_HEIGHT);
    assert!(leaf >= (1u64 << height) && leaf < (1u64 << (height + 1)));

    let prefix_bits = height + 1;
    let prefix_shift = MAX_TREE_HEIGHT - height;
    let prefix_mask = ((1u64 << prefix_bits) - 1) << prefix_shift;
    let forced_prefix = leaf << prefix_shift;

    raw = (raw & ROUTING_BITS_MASK) | REAL_ADDR_MARKER | (raw & HIGH_IDENTITY_BITS_MASK);
    raw = (raw & !prefix_mask) | forced_prefix;
    *addr = raw;
}

/// Trait providing constant-time bit operations, leaf mapping, and path ranking for 64-bit tags.
pub trait OramAddress {
    /// Returns 1 if real (`self != 0`), 0 if dummy (`self == 0`) in constant time.
    fn ct_is_real(&self) -> u8;
    /// Extracts the low 31 routing bits.
    fn routing_bits(&self) -> u64;
    /// Maps the address to a 1-based leaf index at `height`.
    fn to_leaf(&self, height: u64) -> TreeIndex;
    /// Generates a 64-bit sort key ranking blocks along the eviction path to `position`.
    fn path_rank_key(&self, position: TreeIndex, height: u64) -> u64;
}

/// Trait for mutating routing bits to target a specific leaf.
pub trait OramAddressMut {
    /// Overwrites routing bits to force assignment to `leaf`.
    fn force_leaf(&mut self, leaf: TreeIndex, height: u64);
}

impl OramAddress for u64 {
    #[inline]
    fn ct_is_real(&self) -> u8 {
        ((*self | self.wrapping_neg()) >> 63) as u8
    }

    #[inline]
    fn routing_bits(&self) -> u64 {
        *self & ROUTING_BITS_MASK
    }

    #[inline]
    fn to_leaf(&self, height: u64) -> TreeIndex {
        address_to_leaf(*self, height)
    }

    #[inline]
    fn path_rank_key(&self, position: TreeIndex, height: u64) -> u64 {
        let routing = self.routing_bits();
        let leaf_suffix_mask = (1u64 << height).wrapping_sub(1);
        let evict_suffix = position & leaf_suffix_mask;
        let evict_routing = evict_suffix << (MAX_TREE_HEIGHT - height);
        let ranked_routing = routing ^ evict_routing;

        let marker = self.ct_is_real();
        let dummy_bit = crate::ct::ct_not(marker) as u64;
        let identity = *self >> 32;

        (dummy_bit << 63) | (ranked_routing << 32) | identity
    }
}

impl OramAddressMut for u64 {
    #[inline]
    fn force_leaf(&mut self, leaf: TreeIndex, height: u64) {
        force_leaf_routing(self, leaf, height);
    }
}
