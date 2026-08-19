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

use oram::{AutoResizeConfig, ShardedObliviousHistogram};
use rand::{rngs::StdRng, SeedableRng};

fn make_sharded(
    batch_capacity: usize,
    per_shard_quota: usize,
) -> ShardedObliviousHistogram<4, 16, 8, 96> {
    let mut rng = StdRng::seed_from_u64(9001);
    ShardedObliviousHistogram::<4, 16, 8, 96>::new(
        4,
        4096, // Total block capacity (4 shards * 1024 capacity each)
        batch_capacity,
        per_shard_quota,
        &mut rng,
    )
}

fn dp_cfg(t_capacity: u64, seed: u64) -> AutoResizeConfig {
    let mut cfg = AutoResizeConfig::new(t_capacity);
    cfg.seed = seed;
    cfg
}

#[test]
fn sharded_batch_histogram_preserves_counts() {
    let mut hist = make_sharded(256, 128);
    let mut keys = Vec::new();
    for i in 0..100 {
        keys.push(format!("key_{i}").into_bytes());
    }

    for key in &keys {
        hist.append(key, 5);
    }
    hist.flush();

    for key in &keys {
        assert_eq!(hist.read_total(key), 5);
    }
}

#[test]
fn sharded_batch_histogram_increment_batch() {
    let mut hist = make_sharded(256, 128);
    let keys = vec![b"alpha".to_vec(), b"beta".to_vec(), b"gamma".to_vec()];

    for _ in 0..10 {
        for key in &keys {
            hist.append(key, 1);
        }
    }
    hist.flush();

    for key in &keys {
        assert_eq!(hist.read_total(key), 10);
    }
}

#[test]
fn sharded_batch_histogram_handles_skewed_keys() {
    let mut hist = make_sharded(256, 200);
    let hot_key = b"hot_spot".to_vec();
    for _ in 0..150 {
        hist.append(&hot_key, 1);
    }
    hist.flush();

    assert_eq!(hist.read_total(&hot_key), 150);
}

#[test]
fn sharded_batch_histogram_multiple_flushes() {
    let mut hist = make_sharded(128, 64);
    let keys: Vec<Vec<u8>> = (0..32).map(|i| format!("key_{i}").into_bytes()).collect();
    let ops: Vec<usize> = (0..500).map(|i| i % 32).collect();
    let mut expected = vec![0u64; 32];

    let batch_keys: Vec<&[u8]> = ops.iter().map(|&idx| keys[idx].as_slice()).collect();
    for (_i, &key) in batch_keys.iter().enumerate() {
        hist.append(key, 1);
    }
    hist.flush();

    for &idx in &ops {
        expected[idx] += 1;
    }

    for (idx, key) in keys.iter().enumerate() {
        assert_eq!(hist.read_total(key), expected[idx], "key {idx}");
    }
}

#[test]
fn sharded_flush_uses_fixed_quota_subarrays() {
    let batch_capacity = 64usize;
    let per_shard_quota = 32usize;
    let mut hist = make_sharded(batch_capacity, per_shard_quota);

    let mut keys_to_inc = Vec::new();
    for i in 0..batch_capacity {
        keys_to_inc.push(format!("route_{i}").into_bytes());
    }

    for key in &keys_to_inc {
        hist.append(key, 1);
    }

    for key in &keys_to_inc {
        assert_eq!(hist.read_total(key), 1);
    }
}

#[test]
fn sharded_batch_reduces_duplicates_before_routing() {
    let batch_capacity = 64usize;
    let mut hist = make_sharded(batch_capacity, 32);

    let mut keys_to_inc = Vec::new();
    for _ in 0..batch_capacity {
        keys_to_inc.push(b"hot".to_vec());
    }

    for key in &keys_to_inc {
        hist.append(key, 1);
    }

    assert_eq!(hist.read_total(b"hot"), batch_capacity as u64);
}

#[test]
fn global_sort_reduces_duplicates_across_full_batch() {
    let batch_capacity = 64usize;
    let mut hist = make_sharded(batch_capacity, 16);

    let mut keys_to_inc = Vec::new();
    for _ in 0..batch_capacity {
        keys_to_inc.push(b"hot".to_vec());
    }

    for key in &keys_to_inc {
        hist.append(key, 1);
    }

    assert_eq!(hist.read_total(b"hot"), batch_capacity as u64);
}

#[test]
fn suggested_quota_is_above_mean_for_nonempty_batches() {
    let quota = ShardedObliviousHistogram::<4>::suggested_per_shard_quota(1024, 16, 40);
    assert!(quota > 1024 / 16);
}

#[test]
fn grow_all_preserves_counts_and_syncs_capacity() {
    let mut hist = make_sharded(64, 32);
    assert_eq!(hist.shard_capacity(), 1024);
    assert_eq!(hist.total_capacity(), 4096);

    let mut keys_to_inc = Vec::new();
    for _ in 0..25 {
        keys_to_inc.push(b"same".to_vec());
    }
    keys_to_inc.push(b"before".to_vec());

    // pad to batch size 64 to trigger flush
    while keys_to_inc.len() < 64 {
        keys_to_inc.push(format!("dummy_{}", keys_to_inc.len()).into_bytes());
    }

    for key in &keys_to_inc {
        hist.append(key, 1);
    }

    hist.grow();
    assert_eq!(hist.shard_capacity(), 2048);
    assert_eq!(hist.total_capacity(), 8192);

    let mut keys_after = Vec::new();
    keys_after.push(b"same".to_vec());
    keys_after.push(b"after".to_vec());
    while keys_after.len() < 64 {
        keys_after.push(format!("dummy_{}", keys_after.len()).into_bytes());
    }

    for key in &keys_after {
        hist.append(key, 1);
    }

    assert_eq!(hist.read_total(b"same"), 26);
    assert_eq!(hist.read_total(b"before"), 1);
    assert_eq!(hist.read_total(b"after"), 1);
}

#[test]
fn deferred_auto_resize_grows_all_shards_and_preserves_counts() {
    let mut rng = StdRng::seed_from_u64(9010);
    let mut hist = ShardedObliviousHistogram::<4, 16, 4, 256>::new(4, 2048, 64, 48, &mut rng);
    hist.enable_auto_resize(dp_cfg(80, 9011));

    let initial_capacity = hist.shard_capacity();
    let mut saw_grow = false;

    let mut keys = Vec::new();
    for i in 0..700usize {
        keys.push(format!("auto_{i}").into_bytes());
    }

    for chunk in keys.chunks(64) {
        let slices: Vec<&[u8]> = chunk.iter().map(|k| k.as_slice()).collect();
        // pad if last chunk is small
        let mut padded = slices.clone();
        while padded.len() < 64 {
            padded.push(b"dummy_pad");
        }
        for key in &padded {
            hist.append(key, 1);
        }
        if hist.shard_capacity() > initial_capacity {
            saw_grow = true;
        }
    }

    assert!(saw_grow, "expected synchronized auto-resize to fire");
    assert!(hist.shard_capacity() > initial_capacity);
    assert_eq!(hist.total_capacity(), hist.shard_capacity() * 4);

    for i in [0usize, 17, 91, 313, 699] {
        assert_eq!(hist.read_total(format!("auto_{i}").as_bytes()), 1);
    }
}
