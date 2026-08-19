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

use oram::{Address, AutoResizeConfig, FlowRecord, ObliviousHistogram};
use rand::{rngs::StdRng, RngExt, SeedableRng};

fn make_hist_sum(capacity: Address) -> ObliviousHistogram<4, 16, 1> {
    let mut rng = StdRng::seed_from_u64(0);
    ObliviousHistogram::<4, 16, 1>::new(capacity, &mut rng)
}

fn dp_cfg(t_capacity: u64, seed: u64) -> AutoResizeConfig {
    let mut cfg = AutoResizeConfig::new(t_capacity);
    cfg.seed = seed;
    cfg
}

fn run_lazy_eviction_test_a1(expected: &[u64], ops: &[usize], keys: &[Vec<u8>]) {
    let mut r = StdRng::seed_from_u64(7);
    let mut h = ObliviousHistogram::<4, 16, 1>::new(1024, &mut r);

    for &i in ops {
        h.append(&keys[i], 1u64);
    }

    for (i, k) in keys.iter().enumerate() {
        assert_eq!(h.read_total(k), expected[i], "key {i}");
    }
}

#[test]
fn test_flow_record_32byte_accumulation() {
    let mut r = StdRng::seed_from_u64(42);
    let mut h = ObliviousHistogram::<4, 16, 1, 64, FlowRecord>::new(1024, &mut r);

    let key_a = b"192.168.1.1:8080";
    let key_b = b"10.0.0.1:443----";

    let rec1 = FlowRecord {
        packet_count: 10,
        byte_sum: 1500,
        first_seen: 100,
        last_seen: 105,
        record_count: 1,
        tcp_flags: 0x02, // SYN
        _padding: 0,
    };

    let rec2 = FlowRecord {
        packet_count: 20,
        byte_sum: 3000,
        first_seen: 90, // Earlier first_seen
        last_seen: 120, // Later last_seen
        record_count: 1,
        tcp_flags: 0x10, // ACK
        _padding: 0,
    };

    h.append(key_a, rec1);
    h.append(key_a, rec2);

    let readback = h.read_total(key_a);
    assert_eq!(readback.packet_count, 30);
    assert_eq!(readback.byte_sum, 4500);
    assert_eq!(readback.first_seen, 90);
    assert_eq!(readback.last_seen, 120);
    assert_eq!(readback.record_count, 2);
    assert_eq!(readback.tcp_flags, 0x12); // SYN | ACK

    let readback_b = h.read_total(key_b);
    assert_eq!(readback_b.packet_count, 0);
}

#[test]
fn lazy_eviction_preserves_counts() {
    let w = 64usize;
    let iters = 400usize;
    let keys: Vec<Vec<u8>> = (0..w).map(|i| format!("k{i}").into_bytes()).collect();

    let mut rng = StdRng::seed_from_u64(42);
    let ops: Vec<usize> = (0..iters)
        .map(|_| {
            let u: f64 = rng.random();
            ((w as f64) * u * u) as usize % w
        })
        .collect();
    let mut expected = vec![0u64; w];
    for &i in &ops {
        expected[i] += 1;
    }

    run_lazy_eviction_test_a1(&expected, &ops, &keys);
}

#[test]
fn basic_sum_accumulation() {
    let mut hs = make_hist_sum(8);
    for v in [1u64, 2, 3] {
        hs.append(b"s", v);
    }
    assert_eq!(hs.read_total(b"s"), 6);
    assert_eq!(hs.read_total(b"never"), 0);
}

#[test]
fn repeated_appends_in_overflow_before_eviction() {
    let mut rng = StdRng::seed_from_u64(0);
    let mut h = ObliviousHistogram::<4, 16, 100>::new(1024, &mut rng);

    for _ in 0..32 {
        h.append(b"hot", 1);
    }

    assert_eq!(h.stash_occupancy(), 32);
    assert_eq!(h.read_total(b"hot"), 32);
}

#[test]
fn small_key_mode_accumulates_u128_keys() {
    let mut rng = StdRng::seed_from_u64(19);
    let mut h = ObliviousHistogram::<4, 16, 16>::new(1024, &mut rng);

    let keys = [0u128, 1, u64::MAX as u128, 1u128 << 96, (1u128 << 127) | 7];
    for (i, key) in keys.iter().copied().enumerate() {
        h.append(&key.to_le_bytes(), (i as u64) + 1);
        h.append(&key.to_le_bytes(), 10);
    }

    for (i, key) in keys.iter().copied().enumerate() {
        assert_eq!(h.read_total(&key.to_le_bytes()), (i as u64) + 11);
    }
    assert_eq!(h.read_total(&99u128.to_le_bytes()), 0);
}

#[test]
fn small_key_mode_lazy_eviction_preserves_counts() {
    let w = 128usize;
    let iters = 600usize;
    let keys: Vec<u128> =
        (0..w).map(|i| ((i as u128) << 65) | (u128::from((i * 17) as u64))).collect();

    let mut rng = StdRng::seed_from_u64(142);
    let ops: Vec<usize> = (0..iters)
        .map(|_| {
            let u: f64 = rng.random();
            ((w as f64) * u * u) as usize % w
        })
        .collect();
    let mut expected = vec![0u64; w];
    for &i in &ops {
        expected[i] += 1;
    }

    let mut setup_rng = StdRng::seed_from_u64(143);
    let mut h = ObliviousHistogram::<4, 16, 8, 160>::new(2048, &mut setup_rng);

    for &i in &ops {
        h.append(&keys[i].to_le_bytes(), 1);
    }

    for (i, key) in keys.iter().copied().enumerate() {
        assert_eq!(h.read_total(&key.to_le_bytes()), expected[i], "small key {i}");
    }
}

#[test]
fn sort_reduce_merge_preserves_histogram_counts() {
    let w = 96usize;
    let iters = 500usize;
    let keys: Vec<Vec<u8>> = (0..w).map(|i| format!("sr_{i}").into_bytes()).collect();

    let mut rng = StdRng::seed_from_u64(616);
    let ops: Vec<usize> = (0..iters)
        .map(|_| {
            let u: f64 = rng.random();
            ((w as f64) * u * u) as usize % w
        })
        .collect();
    let mut expected = vec![0u64; w];
    for &i in &ops {
        expected[i] += 1;
    }

    let mut setup_rng = StdRng::seed_from_u64(617);
    let mut h = ObliviousHistogram::<4, 16, 8, 96>::new(2048, &mut setup_rng);

    for &i in &ops {
        h.append(&keys[i], 1u64);
    }

    for (i, key) in keys.iter().enumerate() {
        assert_eq!(h.read_total(key), expected[i], "key {i}");
    }
}

#[test]
fn compact_writeback_preserves_histogram_counts() {
    let w = 96usize;
    let iters = 500usize;
    let keys: Vec<Vec<u8>> = (0..w).map(|i| format!("pc_{i}").into_bytes()).collect();

    let mut rng = StdRng::seed_from_u64(716);
    let ops: Vec<usize> = (0..iters)
        .map(|_| {
            let u: f64 = rng.random();
            ((w as f64) * u * u) as usize % w
        })
        .collect();
    let mut expected = vec![0u64; w];
    for &i in &ops {
        expected[i] += 1;
    }

    let mut setup_rng = StdRng::seed_from_u64(717);
    let mut h = ObliviousHistogram::<4, 16, 8, 96>::new(2048, &mut setup_rng);

    for &i in &ops {
        h.append(&keys[i], 1u64);
    }

    for (i, key) in keys.iter().enumerate() {
        assert_eq!(h.read_total(key), expected[i], "key {i}");
    }
}

#[test]
fn manual_grow_preserves_values() {
    let mut hs = make_hist_sum(8);
    hs.append(b"a", 10u64);
    hs.append(b"a", 20u64);
    for _ in 0..3 {
        hs.grow();
    }
    hs.append(b"fresh", 7u64);
    assert_eq!(hs.read_total(b"a"), 30);
    assert_eq!(hs.read_total(b"fresh"), 7);
}

#[test]
fn dp_auto_resize_grows_and_preserves() {
    let mut rng = StdRng::seed_from_u64(101);
    let mut h = ObliviousHistogram::<4, 16, 1>::new(512, &mut rng);
    h.enable_auto_resize(dp_cfg(200, 7));

    let h0 = h.height();
    let n = 260usize;
    let mut peak_stash = 0u64;
    for i in 0..n {
        h.append(format!("c_{i}").as_bytes(), (i + 1) as u64);
        peak_stash = peak_stash.max(h.stash_occupancy());
    }

    assert!(h.height() > h0);
    assert!(peak_stash < 64, "stash overflow peak {peak_stash}");

    for i in 0..n {
        assert_eq!(h.read_total(format!("c_{i}").as_bytes()), (i + 1) as u64);
    }
}

#[test]
fn dp_auto_resize_randomized_mixed_workload() {
    let mut setup_rng = StdRng::seed_from_u64(105);
    let mut h = ObliviousHistogram::<4, 16, 1>::new(512, &mut setup_rng);
    h.enable_auto_resize(dp_cfg(160, 9));

    let n_keys = 220usize;
    let mut rng = StdRng::seed_from_u64(206);
    let mut expected = vec![0u64; n_keys];
    for _ in 0..200 {
        let i = rng.random_range(0..n_keys);
        let v = rng.random_range(1u64..100u64);
        h.append(format!("r_{i}").as_bytes(), v);
        expected[i] = expected[i].wrapping_add(v);
    }

    for (i, e) in expected.iter().enumerate() {
        assert_eq!(h.read_total(format!("r_{i}").as_bytes()), *e);
    }
}

#[test]
fn dp_no_resize_with_huge_threshold() {
    let mut rng = StdRng::seed_from_u64(107);
    let mut h = ObliviousHistogram::<4, 16, 1>::new(256, &mut rng);
    let h0 = h.height();
    h.enable_auto_resize(dp_cfg(10_000_000, 3));

    for i in 0..200usize {
        h.append(format!("e_{i}").as_bytes(), 1u64);
    }

    assert_eq!(h.height(), h0, "huge T must not resize");
    for i in 0..200usize {
        assert_eq!(h.read_total(format!("e_{i}").as_bytes()), 1);
    }
}

#[test]
fn oblivious_histogram_increment_batch() {
    let mut rng = StdRng::seed_from_u64(999);
    let mut h = ObliviousHistogram::<4, 16, 1>::new(1024, &mut rng);
    h.append(b"single", 1);
    for key in &[b"batch1", b"batch2", b"single", b"batch1"] {
        h.append(&key[..], 1);
    }
    assert_eq!(h.read_total(b"single"), 2);
    assert_eq!(h.read_total(b"batch1"), 2);
    assert_eq!(h.read_total(b"batch2"), 1);
}

#[test]
fn new_with_evict_interval_validates_a() {
    let mut rng = StdRng::seed_from_u64(3);
    // A = 3 construction should succeed.
    let _ok = ObliviousHistogram::<4, 16, 3>::new(1024, &mut rng);

    // A = 4 construction should now also succeed.
    let _ok4 = ObliviousHistogram::<4, 16, 4>::new(1024, &mut rng);
}

#[test]
fn safe_evict_interval_preserves_counts() {
    let keys: Vec<Vec<u8>> = (0..32).map(|i| format!("key{i}").into_bytes()).collect();
    let mut rng = StdRng::seed_from_u64(123);
    let mut h = ObliviousHistogram::<4>::new(1024, &mut rng);

    let mut expected = vec![0u64; keys.len()];
    for _ in 0..400 {
        let i = rng.random_range(0..keys.len());
        h.append(&keys[i], 1);
        expected[i] += 1;
    }
    h.flush();
    for (i, key) in keys.iter().enumerate() {
        assert_eq!(h.read_total(key), expected[i], "count mismatch for key index {i}");
    }
}

#[test]
fn test_resize_no_panic() {
    let mut rng = StdRng::seed_from_u64(17);
    // Z=16, K=16, A=20, S=32
    let mut h = ObliviousHistogram::<16, 16, 20, 32>::new(512, &mut rng);

    // Configure resizing with the CORRECT t_capacity
    let start_cap = h.capacity();
    let scale = (16u64 / 4).max(1); // Z=16
    let t_capacity = (start_cap * 50 / 100) / scale; // 50% load factor

    let cfg = AutoResizeConfig {
        t_capacity,
        eps: 1.0,
        delta: 1e-6,
        alpha: 0.05,
        r: 1,
        seed: 17 ^ 0x0A00_5000_EAEA,
    };
    h.enable_auto_resize(cfg);

    for i in 0..800 {
        let key = format!("key_{i}");
        h.append(key.as_bytes(), 1);
    }

    assert!(h.capacity() > 512);
}


