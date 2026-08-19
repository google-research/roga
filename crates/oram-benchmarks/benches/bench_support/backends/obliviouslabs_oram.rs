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

use crate::bench_support::interface::{OramBenchBackend, OramOp};
use obliviouslabs_oram::{Key16, ParOMapBinding};
use oram::oblivious::compaction::{compact_marks, compact_payload};
use oram::oblivious::reduction::reduce_equal_runs;
use oram::OramBlock;

/// Adapter wrapping ObliviousLabs' ParOMap (parallel oblivious map C++ implementation).
pub struct ParOMapBenchWrapper {
    name: String,
    omap: ParOMapBinding,
    capacity: u32,
}

impl ParOMapBenchWrapper {
    /// Initializes and allocates an empty ParOMap with the given capacity and thread pool size.
    pub fn new(name: &str, capacity: u32, threads: u32) -> Self {
        let mut omap = unsafe { ParOMapBinding::new() };
        unsafe {
            omap.InitEmpty(capacity, threads);
        }
        Self { name: name.to_string(), omap, capacity }
    }
}

/// Helper converting a byte slice into a 16-byte fixed-size `Key16`.
fn to_key16(key: &[u8]) -> Key16 {
    let mut bytes = [0u8; 16];
    let len = key.len().min(16);
    bytes[..len].copy_from_slice(&key[..len]);
    Key16 { bytes }
}

impl OramBenchBackend for ParOMapBenchWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        if ops.is_empty() {
            return;
        }

        let batch_size = ops.len();
        let mut blocks = vec![OramBlock::<16>::dummy(); batch_size];
        let mut sort_keys = vec![0u64; batch_size];

        for (i, op) in ops.iter().enumerate() {
            let mut key_bytes = [0u8; 16];
            let len = op.key.len().min(16);
            key_bytes[..len].copy_from_slice(&op.key[..len]);

            let val = 1;

            blocks[i] = OramBlock::real(1, 0, val, key_bytes);

            let mut k1 = 0u64;
            let mut k2 = 0u64;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    key_bytes.as_ptr(),
                    &mut k1 as *mut u64 as *mut u8,
                    8,
                );
                std::ptr::copy_nonoverlapping(
                    key_bytes.as_ptr().add(8),
                    &mut k2 as *mut u64 as *mut u8,
                    8,
                );
            }
            sort_keys[i] = k1 ^ k2;
        }

        // Sort obliviously so identical keys become adjacent
        oram::djbsort::sort_with_payload(&mut sort_keys, &mut blocks);

        // Deduplicate adjacent equal keys and accumulate counts obliviously
        reduce_equal_runs(&mut blocks);

        // Obliviously compact real blocks to the front of the batch
        let mut marks = Vec::new();
        compact_marks(&blocks, &mut marks);
        compact_payload(&mut blocks, &marks);

        // Populate fixed-size FFI arrays to avoid leaking distinct real key counts
        let mut ffi_keys = vec![Key16 { bytes: [0u8; 16] }; batch_size];
        let mut val_u64 = vec![0u64; batch_size];
        let mut flags = vec![false; batch_size];

        for i in 0..batch_size {
            ffi_keys[i] = Key16 { bytes: blocks[i].payload };
        }

        unsafe {
            self.omap.FindBatch(
                batch_size as u32,
                ffi_keys.as_ptr(),
                val_u64.as_mut_ptr(),
                flags.as_mut_ptr(),
            );
        }

        let mut insert_keys = vec![Key16 { bytes: [0u8; 16] }; batch_size];
        let mut insert_vals = vec![0u64; batch_size];

        for i in 0..batch_size {
            let is_real = blocks[i].tag != 0;
            let cond = is_real as u8;
            let mask = (0u8).wrapping_sub(cond);

            let current = if flags[i] { val_u64[i] } else { 0 };
            let real_val = current.wrapping_add(blocks[i].value);

            // Branchless bitwise selection of key and value
            insert_vals[i] = real_val.wrapping_mul(cond as u64);
            for j in 0..16 {
                insert_keys[i].bytes[j] = blocks[i].payload[j] & mask;
            }
        }

        let mut insert_flags = vec![false; batch_size];
        unsafe {
            self.omap.InsertBatch(
                batch_size as u32,
                insert_keys.as_ptr(),
                insert_vals.as_ptr(),
                insert_flags.as_mut_ptr(),
            );
        }
    }

    fn capacity(&self) -> u64 {
        self.capacity as u64
    }

    fn read_total(&mut self, key: &[u8], _idx: usize) -> u64 {
        let key_16 = to_key16(key);
        let mut val_u64 = 0u64;
        let mut flag = false;
        unsafe {
            self.omap.FindBatch(1, &key_16, &mut val_u64, &mut flag);
        }
        if flag {
            val_u64
        } else {
            0
        }
    }
}
