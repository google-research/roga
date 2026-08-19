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
#![allow(clippy::needless_range_loop)]

use cmov::Cmov;
use oram::ct::{ct_eq, ct_lt, ct_swap, Cmov as _};
use oram::oblivious::reduction::{reduce_equal_runs, Reducible};
use oram::testing::{Bucket, ObliviousStash, OramAddress, OramBlock, TreeIndex};

const P: usize = 8;

struct MockObliviousStash<const Z: usize, const P: usize> {
    blocks: Vec<OramBlock<P>>,
    path_size: usize,
    sort_keys: Vec<u64>,
    insert_scratch: Vec<OramBlock<P>>,
    compact_marks: Vec<usize>,
    distribute_marks: Vec<usize>,
}

// Globals to hold secret inputs.
#[no_mangle]
pub static mut SECRET_A: u64 = 0;
#[no_mangle]
pub static mut SECRET_B: u64 = 0;
#[no_mangle]
pub static mut SECRET_COND: u8 = 0;

// Globals to hold outputs, to prevent optimization.
#[no_mangle]
pub static mut OUT_U8: u8 = 0;
#[no_mangle]
pub static mut OUT_U64_1: u64 = 0;

// Globals for block-based verification
const NUM_BLOCKS: usize = 3;
#[no_mangle]
pub static mut SECRET_TAG: u64 = 0;
#[no_mangle]
pub static mut SECRET_BLOCKS: [OramBlock<P>; NUM_BLOCKS] =
    [OramBlock { tag: 0, epoch: 0, value: 0, payload: [0u8; P] }; NUM_BLOCKS];

// Globals for physical memory (Z=2 legacy, kept for SECRET_PM reference)
const PM_SIZE: usize = 8;
#[no_mangle]
pub static mut SECRET_PM: [Bucket<2, P>; PM_SIZE] =
    [Bucket { blocks: [OramBlock { tag: 0, epoch: 0, value: 0, payload: [0u8; P] }; 2] }; PM_SIZE];

#[no_mangle]
pub static mut SECRET_REDUCED_LEN: usize = 0;
#[no_mangle]
pub static mut SECRET_COUNTS: [u64; 4] = [0; 4];

#[used]
#[no_mangle]
pub static mut HEAP_PTR: u64 = 0x30000000;

// ---- CT primitives (Z-independent) ----

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_ct_eq() {
    unsafe {
        core::hint::black_box(&raw const HEAP_PTR);
        let a = core::ptr::read_volatile(&raw const SECRET_A);
        let b = core::ptr::read_volatile(&raw const SECRET_B);
        let res = ct_eq(a, b);
        core::ptr::write_volatile(&raw mut OUT_U8, res);
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_ct_lt() {
    unsafe {
        let a = core::ptr::read_volatile(&raw const SECRET_A);
        let b = core::ptr::read_volatile(&raw const SECRET_B);
        let res = ct_lt(a, b);
        core::ptr::write_volatile(&raw mut OUT_U8, res);
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_ct_swap() {
    unsafe {
        let mut a = core::ptr::read_volatile(&raw const SECRET_A);
        let mut b = core::ptr::read_volatile(&raw const SECRET_B);
        let cond = core::ptr::read_volatile(&raw const SECRET_COND);
        ct_swap(&mut a, &mut b, cond);
        core::ptr::write_volatile(&raw mut OUT_U64_1, a ^ b);
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_ct_eq_bytes() {
    unsafe {
        let a = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].payload);
        let b = core::ptr::read_volatile(&raw const SECRET_BLOCKS[1].payload);
        let res = oram::ct::ct_eq_bytes(&a, &b);
        core::ptr::write_volatile(&raw mut OUT_U8, res);
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_ct_swap_u8() {
    unsafe {
        let mut a = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].payload[0]);
        let mut b = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].payload[1]);
        let cond = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].value) as u8;
        oram::ct::ct_swap(&mut a, &mut b, cond);
        core::ptr::write_volatile(&raw mut OUT_U8, a);
        core::ptr::write_volatile(&raw mut OUT_U8, b);
    }
}

// ---- Reduction & merge (Z-independent) ----

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_reduce_equal_runs() {
    unsafe {
        let mut local_blocks = [OramBlock::<P>::dummy(); NUM_BLOCKS];
        for i in 0..NUM_BLOCKS {
            local_blocks[i] = core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]);
        }
        let _profile = reduce_equal_runs(&mut local_blocks);
        for i in 0..NUM_BLOCKS {
            core::ptr::write_volatile(&raw mut SECRET_BLOCKS[i], local_blocks[i]);
        }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_merge_accumulate_for_path() {
    unsafe {
        let mut blocks = [OramBlock::<P>::dummy(); 8];
        for i in 0..3 {
            blocks[i] = core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]);
        }
        for i in 3..8 {
            blocks[i] = OramBlock::dummy();
        }
        let position = 5u64;
        let height = 2u64;
        let mut keys = [0u64; 8];
        for i in 0..8 {
            keys[i] = blocks[i].tag.path_rank_key(position, height);
        }
        oram::djbsort::sort_with_payload(&mut keys, &mut blocks);
        reduce_equal_runs(&mut blocks);
        for i in 0..3 {
            core::ptr::write_volatile(&raw mut SECRET_BLOCKS[i], blocks[i]);
        }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_read_and_remove() {
    unsafe {
        let mut local_blocks = [OramBlock::<P>::dummy(); NUM_BLOCKS];
        for i in 0..NUM_BLOCKS {
            local_blocks[i] = core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]);
        }
        let target_payload = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].payload);
        let mut result_value = 0u64;
        let mut result_payload = [0u8; P];
        for block in &mut local_blocks {
            let is_target =
                oram::ct::ct_eq_bytes(&block.payload, &target_payload) & block.tag.ct_is_real();
            let value = block.value;
            result_value.cmovnz(&value, is_target);
            result_payload.cmov(&block.payload, is_target != 0);
            block.conditional_dummy(is_target);
        }
        core::ptr::write_volatile(&raw mut OUT_U64_1, result_value);
        for i in 0..NUM_BLOCKS {
            core::ptr::write_volatile(&raw mut SECRET_BLOCKS[i], local_blocks[i]);
        }
    }
}

#[no_mangle]
#[inline(never)]
pub extern "C" fn verify_shard_routing_counts() {
    unsafe {
        let mut counts = [0u64; 4];
        let mut local_blocks = [OramBlock::<P>::dummy(); NUM_BLOCKS];
        for i in 0..NUM_BLOCKS {
            local_blocks[i] = core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]);
        }
        let reduced_len = core::ptr::read_volatile(&raw const SECRET_REDUCED_LEN);
        let reduced_len = reduced_len.min(NUM_BLOCKS);
        for i in 0..NUM_BLOCKS {
            let tag = local_blocks[i].tag;
            let block_is_real = tag.ct_is_real();
            let active = core::hint::black_box(ct_lt(i as u64, reduced_len as u64) & block_is_real);
            let shard = (tag >> 32) as usize & (4 - 1);
            for s in 0..4 {
                let match_shard = ct_eq(shard as u64, s as u64);
                let should_inc = match_shard & active;
                let new_count = counts[s] + 1;
                counts[s].cmovnz(&new_count, should_inc);
            }
        }
        for i in 0..4 {
            core::ptr::write_volatile(&raw mut SECRET_COUNTS[i], counts[i]);
        }
    }
}

// ---- Per-Z write_to_path ----
// Tree depth=3 (height=2), 8 buckets. Stash = Z*3 + 2.

macro_rules! gen_write_to_path {
    ($fn_name:ident, $Z:expr) => {
        #[no_mangle]
        #[inline(never)]
        pub extern "C" fn $fn_name() {
            const SL: usize = $Z * 3 + 2;
            const SL1: usize = SL + 1;
            const PMB: usize = 8 * $Z;
            static mut BLK: [core::mem::MaybeUninit<OramBlock<P>>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut SK: [core::mem::MaybeUninit<u64>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut CM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            static mut DM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            unsafe {
                let bp = BLK.as_mut_ptr() as *mut OramBlock<P>;
                for i in 0..NUM_BLOCKS {
                    core::ptr::write(
                        bp.add(i),
                        core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]),
                    );
                }
                for i in NUM_BLOCKS..SL {
                    core::ptr::write(bp.add(i), OramBlock::dummy());
                }
                let blocks = Vec::from_raw_parts(bp, SL, SL);
                let skp = SK.as_mut_ptr() as *mut u64;
                for i in 0..SL {
                    core::ptr::write(skp.add(i), 0);
                }
                let sort_keys = Vec::from_raw_parts(skp, SL, SL);
                let cmp = CM.as_mut_ptr() as *mut usize;
                let dmp = DM.as_mut_ptr() as *mut usize;
                for i in 0..SL1 {
                    core::ptr::write(cmp.add(i), 0);
                    core::ptr::write(dmp.add(i), 0);
                }
                let compact_marks = Vec::from_raw_parts(cmp, SL1, SL1);
                let distribute_marks = Vec::from_raw_parts(dmp, SL1, SL1);
                let ms = MockObliviousStash {
                    blocks,
                    path_size: $Z * 3,
                    sort_keys,
                    insert_scratch: Vec::new(),
                    compact_marks,
                    distribute_marks,
                };
                let mut stash =
                    core::mem::transmute::<MockObliviousStash<$Z, P>, ObliviousStash<$Z, P>>(ms);
                let mut pm = [Bucket::<$Z, P>::default(); 8];
                for i in 0..8 {
                    for j in 0..$Z {
                        pm[i].blocks[j] =
                            core::ptr::read_volatile(&raw const SECRET_BLOCKS[j % NUM_BLOCKS]);
                    }
                }
                let flat_pm: &mut [OramBlock<P>] =
                    core::slice::from_raw_parts_mut(pm.as_mut_ptr() as *mut OramBlock<P>, PMB);
                stash.write_to_path(flat_pm, 5);
                for i in 0..NUM_BLOCKS {
                    core::ptr::write_volatile(
                        &raw mut SECRET_BLOCKS[i],
                        *stash.blocks.get_unchecked(i),
                    );
                }
                core::mem::forget(stash);
            }
        }
    };
}

// ---- Per-Z insert ----

macro_rules! gen_insert {
    ($fn_name:ident, $Z:expr) => {
        #[no_mangle]
        #[inline(never)]
        pub extern "C" fn $fn_name() {
            const SL: usize = $Z * 3 + 2;
            const SL1: usize = SL + 1;
            static mut BLK: [core::mem::MaybeUninit<OramBlock<P>>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut SK: [core::mem::MaybeUninit<u64>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut CM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            static mut DM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            unsafe {
                let bp = BLK.as_mut_ptr() as *mut OramBlock<P>;
                for i in 0..NUM_BLOCKS {
                    core::ptr::write(
                        bp.add(i),
                        core::ptr::read_volatile(&raw const SECRET_BLOCKS[i]),
                    );
                }
                for i in NUM_BLOCKS..SL {
                    core::ptr::write(bp.add(i), OramBlock::dummy());
                }
                let blocks = Vec::from_raw_parts(bp, SL, SL);
                let skp = SK.as_mut_ptr() as *mut u64;
                for i in 0..SL {
                    core::ptr::write(skp.add(i), 0);
                }
                let sort_keys = Vec::from_raw_parts(skp, SL, SL);
                let cmp = CM.as_mut_ptr() as *mut usize;
                let dmp = DM.as_mut_ptr() as *mut usize;
                for i in 0..SL1 {
                    core::ptr::write(cmp.add(i), 0);
                    core::ptr::write(dmp.add(i), 0);
                }
                let compact_marks = Vec::from_raw_parts(cmp, SL1, SL1);
                let distribute_marks = Vec::from_raw_parts(dmp, SL1, SL1);
                let ms = MockObliviousStash {
                    blocks,
                    path_size: $Z * 3,
                    sort_keys,
                    insert_scratch: Vec::new(),
                    compact_marks,
                    distribute_marks,
                };
                let mut stash =
                    core::mem::transmute::<MockObliviousStash<$Z, P>, ObliviousStash<$Z, P>>(ms);
                let addr = core::ptr::read_volatile(&raw const SECRET_TAG);
                stash.insert(addr, 0, 42u64, &[0u8; P]);
                for i in 0..NUM_BLOCKS {
                    core::ptr::write_volatile(
                        &raw mut SECRET_BLOCKS[i],
                        *stash.blocks.get_unchecked(i),
                    );
                }
                core::mem::forget(stash);
            }
        }
    };
}

// ---- Per-Z multi-op: insert + insert + merge + evict ----
// Two inserts with symbolic tags that may or may not match, followed by
// sort+reduce (exercises merge vs no-merge on symbolic data) and write_to_path.

macro_rules! gen_insert_insert_evict {
    ($fn_name:ident, $Z:expr) => {
        #[no_mangle]
        #[inline(never)]
        pub extern "C" fn $fn_name() {
            const SL: usize = $Z * 3 + 2;
            const SL1: usize = SL + 1;
            const PMB: usize = 8 * $Z;
            static mut BLK: [core::mem::MaybeUninit<OramBlock<P>>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut SK: [core::mem::MaybeUninit<u64>; $Z * 3 + 2] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 2];
            static mut CM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            static mut DM: [core::mem::MaybeUninit<usize>; $Z * 3 + 3] =
                [core::mem::MaybeUninit::uninit(); $Z * 3 + 3];
            unsafe {
                let bp = BLK.as_mut_ptr() as *mut OramBlock<P>;
                for i in 0..SL {
                    core::ptr::write(bp.add(i), OramBlock::dummy());
                }
                let blocks = Vec::from_raw_parts(bp, SL, SL);
                let skp = SK.as_mut_ptr() as *mut u64;
                for i in 0..SL {
                    core::ptr::write(skp.add(i), 0);
                }
                let sort_keys = Vec::from_raw_parts(skp, SL, SL);
                let cmp = CM.as_mut_ptr() as *mut usize;
                let dmp = DM.as_mut_ptr() as *mut usize;
                for i in 0..SL1 {
                    core::ptr::write(cmp.add(i), 0);
                    core::ptr::write(dmp.add(i), 0);
                }
                let compact_marks = Vec::from_raw_parts(cmp, SL1, SL1);
                let distribute_marks = Vec::from_raw_parts(dmp, SL1, SL1);
                let ms = MockObliviousStash {
                    blocks,
                    path_size: $Z * 3,
                    sort_keys,
                    insert_scratch: Vec::new(),
                    compact_marks,
                    distribute_marks,
                };
                let mut stash =
                    core::mem::transmute::<MockObliviousStash<$Z, P>, ObliviousStash<$Z, P>>(ms);

                // Insert 1: secret tag & value from SECRET_BLOCKS[0] / SECRET_A
                let a1 = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].tag);
                let v1 = core::ptr::read_volatile(&raw const SECRET_A);
                let p1 = core::ptr::read_volatile(&raw const SECRET_BLOCKS[0].payload);
                stash.insert(a1, 0, v1, &p1);

                // Insert 2: secret tag & value from SECRET_BLOCKS[1] / SECRET_B
                // May or may not match addr1 — Binsec explores both symbolically.
                let a2 = core::ptr::read_volatile(&raw const SECRET_BLOCKS[1].tag);
                let v2 = core::ptr::read_volatile(&raw const SECRET_B);
                let p2 = core::ptr::read_volatile(&raw const SECRET_BLOCKS[1].payload);
                stash.insert(a2, 0, v2, &p2);

                // Merge-accumulate: sort + reduce (merge vs no-merge on symbolic keys)
                let len = stash.blocks.len();
                let mut keys = vec![0u64; len];
                for i in 0..len {
                    keys[i] = stash.blocks[i].tag.path_rank_key(5, 2);
                }
                oram::djbsort::sort_with_payload(&mut keys, &mut stash.blocks);
                reduce_equal_runs(&mut stash.blocks);

                // Write-to-path eviction
                let mut pm = [Bucket::<$Z, P>::default(); 8];
                let flat_pm: &mut [OramBlock<P>] =
                    core::slice::from_raw_parts_mut(pm.as_mut_ptr() as *mut OramBlock<P>, PMB);
                stash.write_to_path(flat_pm, 5);

                for i in 0..NUM_BLOCKS {
                    core::ptr::write_volatile(
                        &raw mut SECRET_BLOCKS[i],
                        *stash.blocks.get_unchecked(i),
                    );
                }
                core::mem::forget(stash);
            }
        }
    };
}

// Generate all per-Z targets for Z in {4, 8, 16, 32}.
gen_write_to_path!(verify_write_to_path_z4, 4);
gen_write_to_path!(verify_write_to_path_z8, 8);
gen_write_to_path!(verify_write_to_path_z16, 16);
gen_write_to_path!(verify_write_to_path_z32, 32);

gen_insert!(verify_insert_z4, 4);
gen_insert!(verify_insert_z8, 8);
gen_insert!(verify_insert_z16, 16);
gen_insert!(verify_insert_z32, 32);

gen_insert_insert_evict!(verify_insert_insert_evict_z4, 4);
gen_insert_insert_evict!(verify_insert_insert_evict_z8, 8);
gen_insert_insert_evict!(verify_insert_insert_evict_z16, 16);
gen_insert_insert_evict!(verify_insert_insert_evict_z32, 32);

fn main() {
    verify_ct_eq();
    verify_ct_lt();
    verify_ct_swap();
    verify_ct_eq_bytes();
    verify_ct_swap_u8();
    verify_reduce_equal_runs();
    verify_merge_accumulate_for_path();
    verify_read_and_remove();
    verify_shard_routing_counts();
    verify_write_to_path_z4();
    verify_write_to_path_z8();
    verify_write_to_path_z16();
    verify_write_to_path_z32();
    verify_insert_z4();
    verify_insert_z8();
    verify_insert_z16();
    verify_insert_z32();
    verify_insert_insert_evict_z4();
    verify_insert_insert_evict_z8();
    verify_insert_insert_evict_z16();
    verify_insert_insert_evict_z32();
}
