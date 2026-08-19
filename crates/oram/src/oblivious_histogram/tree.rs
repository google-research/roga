// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Complete-binary-tree index math for `ObliviousHistogram`.

use super::routing::MAX_TREE_HEIGHT;
use crate::OramValue;
// Tree logic

/// 1-based complete binary tree index (root is 1, left child is $2x$, right child is $2x+1$).
pub type TreeIndex = u64;

/// Complete binary tree index operations for ORAM path routing.
pub trait CompleteBinaryTreeIndex
where
    Self: Sized,
{
    /// Computes the node index at depth `depth` along the path to `self` in constant time ($O(1)$).
    fn ct_node_on_path(&self, depth: u64, height: u64) -> Self;
    /// Computes the depth of this node in the tree in constant time ($O(1)$).
    fn ct_depth(&self) -> u64;
    /// Checks if this node is a leaf at the given tree height ($O(1)$).
    fn is_leaf(&self, height: u64) -> bool;
}

impl CompleteBinaryTreeIndex for TreeIndex {
    fn ct_node_on_path(&self, depth: u64, height: u64) -> Self {
        debug_assert_ne!(*self, 0);
        debug_assert!(self.is_leaf(height));
        let shift = height - depth;
        self >> shift
    }

    fn ct_depth(&self) -> u64 {
        63u64.saturating_sub(self.leading_zeros().into())
    }

    fn is_leaf(&self, height: u64) -> bool {
        debug_assert_ne!(*self, 0);
        self.ct_depth() == height
    }
}

pub(super) fn tree_height_for_capacity(block_capacity: crate::Address, z: usize) -> u64 {
    assert!(z.is_power_of_two(), "Z must be a power of two");
    let height = u64::from(block_capacity.ilog2()).saturating_sub(z.trailing_zeros() as u64).max(1);
    debug_assert!(
        height <= MAX_TREE_HEIGHT,
        "height {} exceeds MAX_TREE_HEIGHT {}",
        height,
        MAX_TREE_HEIGHT
    );
    height
}

pub(super) fn leaf_count(height: u64) -> u64 {
    1u64 << height
}

/// Physical tree node storing a fixed array of $Z$ blocks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Bucket<const Z: usize = 16, const K: usize = 16, V = u64> {
    /// Array of $Z$ block slots.
    pub blocks: [crate::OramBlock<K, V>; Z],
}

impl<const Z: usize, const K: usize, V: OramValue> Default for Bucket<Z, K, V> {
    fn default() -> Self {
        Self { blocks: [crate::OramBlock::dummy(); Z] }
    }
}
