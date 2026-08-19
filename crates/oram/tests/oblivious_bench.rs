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

use oram::djbsort::sort_with_payload;
use oram::oblivious::compaction::{compact_marks, compact_payload, distribute_payload};
use oram::oblivious::reduction::reduce_equal_runs;
use oram::testing::OramBlock;
use rand::{rngs::StdRng, RngExt, SeedableRng};
use std::time::Instant;

const P: usize = 8;

fn get_rng() -> StdRng {
    StdRng::seed_from_u64(42)
}

#[test]
fn bench_sort() {
    let mut rng = get_rng();
    let n = 240;
    let mut keys = Vec::with_capacity(n);
    let mut blocks = Vec::with_capacity(n);
    for _ in 0..n {
        let key = rng.random::<u64>();
        let tag = rng.random::<u64>();
        let value = rng.random::<u64>();
        keys.push(key);
        blocks.push(OramBlock::real(tag, 0, value, [0u8; P]));
    }

    let iterations = 10000;

    // Warm up
    let mut dummy_keys = keys.clone();
    let mut dummy_blocks = blocks.clone();
    sort_with_payload(&mut dummy_keys, &mut dummy_blocks);

    let start = Instant::now();
    for _ in 0..iterations {
        let mut copy_keys = keys.clone();
        let mut copy_blocks = blocks.clone();
        sort_with_payload(&mut copy_keys, &mut copy_blocks);
        std::hint::black_box(&copy_blocks);
    }
    let elapsed = start.elapsed();
    let ns_per_op = (elapsed.as_nanos() as f64) / (iterations as f64);

    println!("__ALPHA_EVOLVE_RESULT__");
    println!("{{");
    println!("  \"scores\": {{");
    println!("    \"real_time\": {}", ns_per_op);
    println!("  }}");
    println!("}}");
    println!("__END__");
}

#[test]
fn bench_compact() {
    let mut rng = get_rng();
    let n = 304;
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let tag = rng.random::<u64>();
        let tag = if i % 2 == 0 { 0 } else { tag }; // 0 is dummy
        let value = rng.random::<u64>();
        blocks.push(OramBlock::real(tag, 0, value, [0u8; P]));
    }

    let iterations = 10000;
    let mut marks_buf = vec![0; n + 1];
    compact_marks(&blocks, &mut marks_buf);

    // Warm up
    let mut dummy = blocks.clone();
    compact_payload(&mut dummy, &marks_buf);

    let start = Instant::now();
    for _ in 0..iterations {
        let mut copy = blocks.clone();
        compact_payload(&mut copy, &marks_buf);
        std::hint::black_box(&copy);
    }
    let elapsed = start.elapsed();
    let ns_per_op = (elapsed.as_nanos() as f64) / (iterations as f64);

    println!("__ALPHA_EVOLVE_RESULT__");
    println!("{{");
    println!("  \"scores\": {{");
    println!("    \"real_time\": {}", ns_per_op);
    println!("  }}");
    println!("}}");
    println!("__END__");
}

#[test]
fn bench_distribute() {
    let mut rng = get_rng();
    let n = 304;
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let tag = rng.random::<u64>();
        let tag = if i % 2 == 0 { 0 } else { tag }; // 0 is dummy
        let value = rng.random::<u64>();
        blocks.push(OramBlock::real(tag, 0, value, [0u8; P]));
    }

    let iterations = 10000;
    let mut marks_buf = vec![0; n + 1];
    compact_marks(&blocks, &mut marks_buf);

    let mut compacted = blocks.clone();
    compact_payload(&mut compacted, &marks_buf);

    // Warm up
    let mut dummy = compacted.clone();
    distribute_payload(&mut dummy, &marks_buf);

    let start = Instant::now();
    for _ in 0..iterations {
        let mut copy = compacted.clone();
        distribute_payload(&mut copy, &marks_buf);
        std::hint::black_box(&copy);
    }
    let elapsed = start.elapsed();
    let ns_per_op = (elapsed.as_nanos() as f64) / (iterations as f64);

    println!("__ALPHA_EVOLVE_RESULT__");
    println!("{{");
    println!("  \"scores\": {{");
    println!("    \"real_time\": {}", ns_per_op);
    println!("  }}");
    println!("}}");
    println!("__END__");
}

#[test]
fn bench_reduce() {
    let mut rng = get_rng();
    let n = 304;
    let mut blocks = Vec::with_capacity(n);
    for i in 0..n {
        let run_id = i / 4;
        let tag = run_id as u64;
        let payload = (run_id as u64).to_le_bytes();
        let value = rng.random_range(0..1_000_000);
        blocks.push(OramBlock::real(tag, 0, value, payload));
    }

    let iterations = 10000;

    // Warm up
    let mut dummy = blocks.clone();
    reduce_equal_runs(&mut dummy);

    let start = Instant::now();
    for _ in 0..iterations {
        let mut copy = blocks.clone();
        reduce_equal_runs(&mut copy);
        std::hint::black_box(&copy);
    }
    let elapsed = start.elapsed();
    let ns_per_op = (elapsed.as_nanos() as f64) / (iterations as f64);

    println!("__ALPHA_EVOLVE_RESULT__");
    println!("{{");
    println!("  \"scores\": {{");
    println!("    \"real_time\": {}", ns_per_op);
    println!("  }}");
    println!("}}");
    println!("__END__");
}

use memmap2::MmapMut;
use std::ops::{Deref, DerefMut};

struct VirtualVector<T> {
    mmap: MmapMut,
    len: usize,
    _marker: std::marker::PhantomData<T>,
}

impl<T> VirtualVector<T> {
    pub fn new(initial_len: usize, max_capacity: usize) -> std::io::Result<Self> {
        let size_in_bytes = max_capacity * std::mem::size_of::<T>();
        let mmap = MmapMut::map_anon(size_in_bytes)?;
        Ok(Self { mmap, len: initial_len, _marker: std::marker::PhantomData })
    }

    pub fn resize(&mut self, new_len: usize, default_val: T)
    where
        T: Clone,
    {
        if new_len > self.len {
            let slice = unsafe {
                std::slice::from_raw_parts_mut(self.mmap.as_mut_ptr() as *mut T, new_len)
            };
            slice[self.len..new_len].fill(default_val);
        }
        self.len = new_len;
    }
}

impl<T> Deref for VirtualVector<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        unsafe { std::slice::from_raw_parts(self.mmap.as_ptr() as *const T, self.len) }
    }
}

impl<T> DerefMut for VirtualVector<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { std::slice::from_raw_parts_mut(self.mmap.as_mut_ptr() as *mut T, self.len) }
    }
}

#[derive(Clone, Copy)]
struct DummyBucket {
    _data: [u8; 512],
}

impl Default for DummyBucket {
    fn default() -> Self {
        Self { _data: [0u8; 512] }
    }
}

#[test]
fn bench_vector_grow() {
    let start_cap = 1 << 12; // 4096
    let end_cap = 1 << 21; // 2,097,152

    // 1. Bench standard Vec resize
    let iterations = 100;
    let start_vec = Instant::now();
    for _ in 0..iterations {
        let mut v = vec![DummyBucket::default(); start_cap];
        let mut cap = start_cap;
        while cap < end_cap {
            cap *= 2;
            v.resize(cap, DummyBucket::default());
        }
        std::hint::black_box(&v);
    }
    let vec_elapsed = start_vec.elapsed() / iterations;

    // 2. Bench VirtualVector resize
    let start_mmap = Instant::now();
    for _ in 0..iterations {
        let mut v = VirtualVector::new(start_cap, end_cap).unwrap();
        let mut cap = start_cap;
        while cap < end_cap {
            cap *= 2;
            v.resize(cap, DummyBucket::default());
        }
        std::hint::black_box(&*v);
    }
    let mmap_elapsed = start_mmap.elapsed() / iterations;

    println!("\n__VECTOR_GROW_BENCHMARK__");
    println!("Standard Vec grow time:  {:?}", vec_elapsed);
    println!("VirtualVector grow time: {:?}", mmap_elapsed);
    println!(
        "Speedup:                 {:.2}x",
        vec_elapsed.as_secs_f64() / mmap_elapsed.as_secs_f64()
    );
}

#[test]
fn bench_stats_solver_speed() {
    use oram::oblivious::binomial_solver::{dp_det_threshold, suggested_per_shard_quota};

    let iterations = 1_000;

    // 1. Measure suggested_per_shard_quota execution time
    let start_quota = Instant::now();
    for _ in 0..iterations {
        let q = suggested_per_shard_quota(1000, 4, 40);
        std::hint::black_box(q);
    }
    let quota_time = start_quota.elapsed() / iterations;

    // 2. Measure dp_det_threshold execution time
    let start_dp = Instant::now();
    for _ in 0..iterations {
        let t = dp_det_threshold(65536.0, 16384.0, 1024.0, 0.05, 0.0);
        std::hint::black_box(t);
    }
    let dp_time = start_dp.elapsed() / iterations;

    println!("\n__STATS_SOLVER_BENCHMARK__");
    println!("Per-shard Quota Solver Time: {:?}", quota_time);
    println!("DP Threshold Solver Time:    {:?}", dp_time);
}

#[test]
fn bench_tree_depth_performance() {
    use oram::ObliviousHistogram;
    let mut rng = get_rng();

    // Generate test keys
    let keys: Vec<[u8; 8]> = (0..100).map(|i| (i as u64).to_le_bytes()).collect();

    // 1. Measure depth D=10 (Capacity N=4096, Z=4)
    let mut tree_d10 = ObliviousHistogram::<4, 8, 1>::new(4096, &mut rng);
    let start_d10 = Instant::now();
    for _ in 0..100 {
        for key in &keys {
            tree_d10.append(key, 1);
        }
    }
    let elapsed_d10 = start_d10.elapsed();

    // 2. Measure depth D=11 (Capacity N=8192, Z=4) - the doubled tree height
    let mut tree_d11 = ObliviousHistogram::<4, 8, 1>::new(8192, &mut rng);
    let start_d11 = Instant::now();
    for _ in 0..100 {
        for key in &keys {
            tree_d11.append(key, 1);
        }
    }
    let elapsed_d11 = start_d11.elapsed();

    let slowdown =
        (elapsed_d11.as_secs_f64() - elapsed_d10.as_secs_f64()) / elapsed_d10.as_secs_f64() * 100.0;
    let speedup = elapsed_d11.as_secs_f64() / elapsed_d10.as_secs_f64();

    println!("\n__TREE_DEPTH_PERFORMANCE_BENCHMARK__");
    println!("Height D=10 (N=4,096)  100 batch flushes time: {:?}", elapsed_d10);
    println!("Height D=11 (N=8,192)  100 batch flushes time: {:?}", elapsed_d11);
    println!(
        "Height Doubling Penalty:                       +{:.1}% slower ({:.2}x latency)",
        slowdown, speedup
    );
    println!("Exact Binomial Speedup (Staying in D=10):      {:.2}x faster throughput", speedup);
}
