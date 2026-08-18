// Copyright (c) Meta Platforms, Inc. and affiliates.
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! A trait representing a Path ORAM stash.

use crate::{
    bucket::{Bucket, PathOramBlock},
    utils::{bitonic_sort_by_keys, CompleteBinaryTreeIndex, TreeIndex},
    Address, BucketSize, OramBlock, OramError, StashSize,
};

use subtle::{Choice, ConditionallySelectable, ConstantTimeEq};

const STASH_GROWTH_INCREMENT: usize = 10;

#[derive(Debug)]
/// A fixed-size, obliviously accessed Path ORAM stash data structure implemented using oblivious sorting.
pub struct ObliviousStash<V: OramBlock> {
    blocks: Vec<PathOramBlock<V>>,
    path_size: StashSize,
}

impl<V: OramBlock> ObliviousStash<V> {
    fn len(&self) -> usize {
        self.blocks.len()
    }
}

impl<V: OramBlock> ObliviousStash<V> {
    pub fn new(path_size: StashSize, overflow_size: StashSize) -> Result<Self, OramError> {
        let num_stash_blocks: usize = (path_size + overflow_size).try_into()?;

        Ok(Self { blocks: vec![PathOramBlock::<V>::dummy(); num_stash_blocks], path_size })
    }

    pub fn write_to_path<const Z: BucketSize>(
        &mut self,
        physical_memory: &mut [Bucket<V, Z>],
        position: TreeIndex,
    ) -> Result<(), OramError> {
        let height = position.ct_depth();
        let mut level_assignments = vec![TreeIndex::MAX; self.len()];
        let mut level_counts = vec![0; usize::try_from(height)? + 1];

        // Assign all non-dummy blocks in the stash to either the path or the overflow.
        for (i, block) in self.blocks.iter().enumerate() {
            // If `block` is a dummy, the rest of this loop iteration will be a no-op, and the values don't matter.
            let block_is_dummy = block.ct_is_dummy();

            // Set up valid but meaningless input to the computation in case `block` is a dummy.
            let an_arbitrary_leaf: TreeIndex = 1 << height;
            let block_position =
                TreeIndex::conditional_select(&block.position, &an_arbitrary_leaf, block_is_dummy);

            // Assign the block to a bucket or to the overflow.
            let mut assigned = Choice::from(0);
            // Obliviously scan through the buckets from leaf to root,
            // assigning the block to the first empty bucket satisfying the invariant.
            for (level, count) in level_counts.iter_mut().enumerate().rev() {
                let level_bucket_full: Choice = count.ct_eq(&(u64::try_from(Z)?));

                let level_u64 = u64::try_from(level)?;
                let level_satisfies_invariant = block_position
                    .ct_node_on_path(level_u64, height)
                    .ct_eq(&position.ct_node_on_path(level_u64, height));

                let should_assign = level_satisfies_invariant
                    & (!level_bucket_full)
                    & (!block_is_dummy)
                    & (!assigned);
                assigned |= should_assign;

                let level_count_incremented = *count + 1;
                count.conditional_assign(&level_count_incremented, should_assign);
                level_assignments[i].conditional_assign(&level_u64, should_assign);
            }
            // If the block was not able to be assigned to any bucket, assign it to the overflow.
            level_assignments[i]
                .conditional_assign(&(TreeIndex::MAX - 1), (!assigned) & (!block_is_dummy));
        }

        // Assign dummy blocks to the remaining non-full buckets until all buckets are full.
        let mut exists_unfilled_levels: Choice = 1.into();
        let mut first_unassigned_block_index: usize = 0;
        // Unless the stash overflows, this loop will execute exactly once, and the inner `if` will not execute.
        // If the stash overflows, this loop will execute twice and the inner `if` will execute.
        // This difference in control flow will leak the fact that the stash has overflowed.
        // This is a violation of obliviousness, but the alternative is simply to fail.
        // If the stash is set large enough when the ORAM is initialized,
        // stash overflow will occur only with negligible probability.
        while exists_unfilled_levels.into() {
            // Make a pass over the stash, assigning dummy blocks to unfilled levels in the path.
            for (i, block) in self.blocks.iter().enumerate().skip(first_unassigned_block_index) {
                // Skip the last block. It is reserved for handling writes to uninitialized addresses.
                if i == self.blocks.len() - 1 {
                    break;
                }

                let block_free = block.ct_is_dummy();

                let mut assigned: Choice = 0.into();
                for (level, count) in level_counts.iter_mut().enumerate() {
                    let full = count.ct_eq(&(u64::try_from(Z)?));
                    let no_op = assigned | full | !block_free;

                    level_assignments[i].conditional_assign(&(u64::try_from(level))?, !no_op);
                    count.conditional_assign(&(*count + 1), !no_op);
                    assigned |= !no_op;
                }
            }

            // Check that all levels have been filled.
            exists_unfilled_levels = 0.into();
            for count in level_counts.iter() {
                let full = count.ct_eq(&(u64::try_from(Z)?));
                exists_unfilled_levels |= !full;
            }

            // If not, there must not have been enough dummy blocks remaining in the stash.
            // That is, the stash has overflowed.
            // So, extend the stash with STASH_GROWTH_INCREMENT more dummy blocks,
            // and repeat the process of trying to fill all unfilled levels with dummy blocks.
            if exists_unfilled_levels.into() {
                first_unassigned_block_index = self.blocks.len() - 1;

                self.blocks.resize(
                    self.blocks.len() + STASH_GROWTH_INCREMENT,
                    PathOramBlock::<V>::dummy(),
                );
                level_assignments
                    .resize(level_assignments.len() + STASH_GROWTH_INCREMENT, TreeIndex::MAX);

                log::warn!(
                    "Stash overflow occurred. Stash resized to {} blocks.",
                    self.blocks.len()
                );
            }
        }

        bitonic_sort_by_keys(&mut self.blocks, &mut level_assignments);

        // Write the first Z * height blocks into slots in the tree
        for depth in 0..=height {
            let bucket_to_write =
                &mut physical_memory[usize::try_from(position.ct_node_on_path(depth, height))?];
            for slot_number in 0..Z {
                let stash_index = (usize::try_from(depth)?) * Z + slot_number;

                bucket_to_write.blocks[slot_number] = self.blocks[stash_index];
            }
        }

        Ok(())
    }

    pub fn access<F: Fn(&V) -> V>(
        &mut self,
        address: Address,
        new_position: TreeIndex,
        value_callback: F,
    ) -> Result<V, OramError> {
        let mut result: V = V::default();
        let mut found: Choice = 0.into();

        // Iterate over stash, updating the block with address `address` if one exists.
        for block in &mut self.blocks {
            let is_requested_index = block.address.ct_eq(&address);
            found.conditional_assign(&1.into(), is_requested_index);

            // Read current value of target block into `result`.
            result.conditional_assign(&block.value, is_requested_index);
            // Write new position into target block.
            block.position.conditional_assign(&new_position, is_requested_index);
            // If a write, write new value into target block.
            let value_to_write = value_callback(&result);
            block.value.conditional_assign(&value_to_write, is_requested_index);
        }

        // If a block with address `address` is not found,
        // initialize one by writing to the last block in the stash,
        // which will always be a dummy block.
        let last_block_index = self.blocks.len() - 1;
        let last_block = &mut self.blocks[last_block_index];
        assert!(bool::from(last_block.ct_is_dummy()));
        last_block.conditional_assign(
            &PathOramBlock { value: value_callback(&result), address, position: new_position },
            !found,
        );

        // Return the value of the found block (or the default value, if no block was found)
        Ok(result)
    }

    #[cfg(test)]
    pub fn occupancy(&self) -> StashSize {
        let mut result = 0;
        for i in self.path_size.try_into().unwrap()..(self.blocks.len()) {
            if !self.blocks[i].is_dummy() {
                result += 1;
            }
        }
        result
    }

    pub fn read_from_path<const Z: crate::BucketSize>(
        &mut self,
        physical_memory: &mut [Bucket<V, Z>],
        position: TreeIndex,
    ) -> Result<(), OramError> {
        let height = position.ct_depth();

        for i in (0..(self.path_size / u64::try_from(Z)?)).rev() {
            let bucket_index = position.ct_node_on_path(i, height);
            let bucket = physical_memory[usize::try_from(bucket_index)?];
            for slot_index in 0..Z {
                self.blocks[Z * (usize::try_from(i)?) + slot_index] = bucket.blocks[slot_index];
            }
        }

        Ok(())
    }
}
