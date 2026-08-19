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

#![cfg(feature = "h2o2ram-baseline")]

use crate::bench_support::interface::{OramBenchBackend, OramOp};

/// Adapter wrapping H2O2RAM's multi-threaded `ObliviousMap` baseline.
pub struct H2O2RamBenchWrapper {
    pub name: String,
    pub map: h2o2ram_oram::ObliviousMap,
    pub capacity: u64,
}

unsafe impl Send for H2O2RamBenchWrapper {}
unsafe impl Sync for H2O2RamBenchWrapper {}

impl H2O2RamBenchWrapper {
    /// Initializes an H2O2RAM instance configured for the specified thread/core count.
    pub fn new(name: impl Into<String>, capacity: u64, cores: u32) -> Self {
        let map = h2o2ram_oram::ObliviousMap::with_threads(cores)
            .expect("Failed to initialize H2O2RAM ObliviousMap");
        Self { name: name.into(), map, capacity }
    }

    /// Initializes a key-value configured H2O2RAM instance.
    pub fn new_kv(name: impl Into<String>, capacity: u64, cores: u32) -> Self {
        Self::new(name, capacity, cores)
    }
}

impl OramBenchBackend for H2O2RamBenchWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn step(&mut self, ops: &[OramOp]) {
        for op in ops {
            let mut key_16 = [0u8; 16];
            let len = op.key.len().min(16);
            key_16[..len].copy_from_slice(&op.key[..len]);

            let current_bytes =
                self.map.get(&key_16).unwrap_or(None).unwrap_or([0u8; h2o2ram_oram::VAL_SIZE]);
            let mut val_u64_bytes = [0u8; 8];
            let copy_len = h2o2ram_oram::VAL_SIZE.min(8);
            val_u64_bytes[..copy_len].copy_from_slice(&current_bytes[..copy_len]);
            let current_val = u64::from_le_bytes(val_u64_bytes);

            let updated_val = current_val.wrapping_add(1);
            let updated_bytes = updated_val.to_le_bytes();
            let mut insert_bytes = [0u8; h2o2ram_oram::VAL_SIZE];
            insert_bytes[..copy_len].copy_from_slice(&updated_bytes[..copy_len]);

            let _ = self.map.insert(&key_16, &insert_bytes);
        }
    }

    fn capacity(&self) -> u64 {
        self.capacity
    }

    fn read_total(&mut self, key: &[u8], _idx: usize) -> u64 {
        let mut key_16 = [0u8; 16];
        let len = key.len().min(16);
        key_16[..len].copy_from_slice(&key[..len]);

        match self.map.get(&key_16) {
            Ok(Some(bytes)) => {
                let mut val_u64_bytes = [0u8; 8];
                let copy_len = h2o2ram_oram::VAL_SIZE.min(8);
                val_u64_bytes[..copy_len].copy_from_slice(&bytes[..copy_len]);
                u64::from_le_bytes(val_u64_bytes)
            }
            _ => 0,
        }
    }
}
