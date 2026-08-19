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

use crate::bench_support::interface::{OramBenchBackend, OramOp, OramOpKind};
use aligned_cmov::A8Bytes;
use mc_oblivious_map::{CuckooHashTable, CuckooHashTableCreator};
use mc_oblivious_ram::PathORAM4096Z4Creator;
use mc_oblivious_traits::{HeapORAMStorageCreator, OMapCreator, ObliviousHashMap, OMAP_FOUND};
use rand_core::{CryptoRng, RngCore};
use typenum::{U1024, U16, U8};

/// Deterministic 64-bit linear congruential generator implementing `RngCore` and `CryptoRng`.
pub struct SimpleRng(pub u64);

impl RngCore for SimpleRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1);
        self.0
    }
    fn fill_bytes(&mut self, dest: &mut [u8]) {
        for chunk in dest.chunks_mut(8) {
            let val = self.next_u64().to_le_bytes();
            let len = chunk.len().min(8);
            chunk[..len].copy_from_slice(&val[..len]);
        }
    }
    fn try_fill_bytes(&mut self, dest: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(dest);
        Ok(())
    }
}
impl CryptoRng for SimpleRng {}

/// Adapter wrapping MobileCoin's `mc-oblivious` CuckooHashTable backed by PathORAM.
pub struct McObliviousBenchWrapper {
    name: String,
    rng: SimpleRng,
    map: CuckooHashTable<
        U16,
        U8,
        U1024,
        SimpleRng,
        <PathORAM4096Z4Creator<SimpleRng, HeapORAMStorageCreator> as mc_oblivious_traits::ORAMCreator<U1024, SimpleRng>>::Output,
    >,
}

impl McObliviousBenchWrapper {
    /// Initializes a CuckooHashTable on PathORAM with 16-byte keys and 8-byte counter values.
    pub fn new(name: &str, capacity: u64, seed: u64) -> Self {
        type ORAMCreatorZ4 = PathORAM4096Z4Creator<SimpleRng, HeapORAMStorageCreator>;
        type CuckooCreatorZ4 = CuckooHashTableCreator<U1024, SimpleRng, ORAMCreatorZ4>;

        let map = CuckooCreatorZ4::create(capacity, 16, move || SimpleRng(seed));
        Self { name: name.to_string(), rng: SimpleRng(seed ^ 0x9E3779B97F4A7C15), map }
    }
}

impl OramBenchBackend for McObliviousBenchWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        for op in ops {
            let mut query = A8Bytes::<U16>::default();
            let len = op.key.len().min(16);
            query[..len].copy_from_slice(&op.key[..len]);

            if op.kind == OramOpKind::Increment {
                let default_val = A8Bytes::<U8>::default();
                self.map.access_and_insert(&query, &default_val, &mut self.rng, |status, val| {
                    let mut cur_count = 0u64;
                    if status == OMAP_FOUND {
                        let mut bytes = [0u8; 8];
                        bytes.copy_from_slice(&val[..8]);
                        cur_count = u64::from_le_bytes(bytes);
                    }
                    val[..8].copy_from_slice(&(cur_count + 1).to_le_bytes());
                });
            } else {
                let mut out_val = A8Bytes::<U8>::default();
                self.map.read(&query, &mut out_val);
            }
        }
    }

    fn read_total(&mut self, key: &[u8], _idx: usize) -> u64 {
        let mut query = A8Bytes::<U16>::default();
        let len = key.len().min(16);
        query[..len].copy_from_slice(&key[..len]);

        let mut out_val = A8Bytes::<U8>::default();
        if self.map.read(&query, &mut out_val) == OMAP_FOUND {
            let mut bytes = [0u8; 8];
            bytes.copy_from_slice(&out_val[..8]);
            u64::from_le_bytes(bytes)
        } else {
            0
        }
    }
}
