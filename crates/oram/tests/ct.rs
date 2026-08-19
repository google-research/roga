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

use oram::ct::{ct_eq, ct_lt, ct_swap};
use rand::{rngs::StdRng, RngExt, SeedableRng};

#[test]
fn eq() {
    assert_eq!(ct_eq(1u64, 1u64), 1);
    assert_eq!(ct_eq(1u64, 2u64), 0);
    assert_eq!(ct_eq(0u64, 0u64), 1);
    assert_eq!(ct_eq(u64::MAX, u64::MAX), 1);
    assert_eq!(ct_eq(u64::MAX, 0u64), 0);
    assert_eq!(ct_eq(7u32, 7u32), 1);
    assert_eq!(ct_eq(7u32, 8u32), 0);
}

#[test]
fn lt() {
    assert_eq!(ct_lt(1, 2), 1);
    assert_eq!(ct_lt(2, 1), 0);
    assert_eq!(ct_lt(1, 1), 0);
    assert_eq!(ct_lt(0, u64::MAX), 1);
}

#[test]
fn swap() {
    let mut a = 1u64;
    let mut b = 2u64;
    ct_swap(&mut a, &mut b, 0);
    assert_eq!((a, b), (1, 2));
    ct_swap(&mut a, &mut b, 1);
    assert_eq!((a, b), (2, 1));

    let mut x = 7u32;
    let mut y = 9u32;
    ct_swap(&mut x, &mut y, 1);
    assert_eq!((x, y), (9, 7));
}

#[test]
fn test_oram_block_cmov() {
    use oram::oblivious::ct::Cmov;
    use oram::testing::OramBlock;

    const P: usize = 8;
    let mut block1 = OramBlock::<P> { tag: 1, epoch: 0, value: 10, payload: [0u8; P] };
    let block2 = OramBlock::<P> { tag: 2, epoch: 0, value: 20, payload: [0u8; P] };

    block1.cmov(&block2, false);
    assert_eq!(block1.tag, 1);
    assert_eq!(block1.value, 10);

    block1.cmov(&block2, true);
    assert_eq!(block1.tag, 2);
    assert_eq!(block1.value, 20);
}

#[test]
fn test_oram_block_cxchg() {
    use oram::oblivious::ct::Cmov;
    use oram::testing::OramBlock;

    const P: usize = 8;
    let mut block1 = OramBlock::<P> { tag: 1, epoch: 0, value: 10, payload: [0u8; P] };
    let mut block2 = OramBlock::<P> { tag: 2, epoch: 0, value: 20, payload: [0u8; P] };

    block1.cxchg(&mut block2, false);
    assert_eq!(block1.tag, 1);
    assert_eq!(block1.value, 10);
    assert_eq!(block2.tag, 2);
    assert_eq!(block2.value, 20);

    block1.cxchg(&mut block2, true);
    assert_eq!(block1.tag, 2);
    assert_eq!(block1.value, 20);
    assert_eq!(block2.tag, 1);
    assert_eq!(block2.value, 10);
}

fn run_compaction_test_suite<FC, FD>(compact: FC, distribute: FD)
where
    FC: Fn(&mut [u32], &[usize]),
    FD: Fn(&mut [u32], &[usize]),
{
    // Test case 1: simple
    let mut arr = vec![10u32, 99u32, 20u32, 88u32, 30u32];
    let payload = vec![0, 1, 1, 2, 2, 3];
    let arr_copy = arr.clone();

    compact(&mut arr, &payload);
    assert_eq!(arr[0], 10);
    assert_eq!(arr[1], 20);
    assert_eq!(arr[2], 30);
    assert!((arr[3] == 99 && arr[4] == 88) || (arr[3] == 88 && arr[4] == 99));

    distribute(&mut arr, &payload);
    assert_eq!(arr, arr_copy);

    // Test case 2: all real
    let mut arr2 = vec![10u32, 20u32, 30u32];
    let payload2 = vec![0, 1, 2, 3];
    let arr2_copy = arr2.clone();
    compact(&mut arr2, &payload2);
    assert_eq!(arr2, arr2_copy);
    distribute(&mut arr2, &payload2);
    assert_eq!(arr2, arr2_copy);

    // Test case 3: all dummy
    let mut arr3 = vec![99u32, 88u32, 77u32];
    let payload3 = vec![0, 0, 0, 0];
    let arr3_copy = arr3.clone();
    compact(&mut arr3, &payload3);
    distribute(&mut arr3, &payload3);
    assert_eq!(arr3, arr3_copy);

    // Test case 4: randomized
    let mut rng = StdRng::seed_from_u64(42); // Fixed seed

    for _ in 0..200 {
        let len = rng.random_range(1..100);
        let mut arr: Vec<u32> = (0..len).map(|_| rng.random::<u32>()).collect();
        let marks: Vec<bool> = (0..len).map(|_| rng.random::<bool>()).collect();
        let arr_copy = arr.clone();

        let mut payload = vec![0; len + 1];
        for i in 0..len {
            payload[i + 1] = payload[i] + usize::from(marks[i]);
        }

        compact(&mut arr, &payload);

        let n_real = payload[len];

        let mut expected_real = Vec::new();
        let mut expected_dummies = Vec::new();
        for i in 0..len {
            if marks[i] {
                expected_real.push(arr_copy[i]);
            } else {
                expected_dummies.push(arr_copy[i]);
            }
        }

        let mut got_real = arr[..n_real].to_vec();
        let mut got_dummies = arr[n_real..].to_vec();

        got_real.sort();
        expected_real.sort();
        assert_eq!(got_real, expected_real);

        got_dummies.sort();
        expected_dummies.sort();
        assert_eq!(got_dummies, expected_dummies);

        distribute(&mut arr, &payload);
        assert_eq!(arr, arr_copy);
    }
}

#[test]
fn test_compaction_sequential() {
    use oram::oblivious::compaction::{compact_payload, distribute_payload};
    run_compaction_test_suite(compact_payload, distribute_payload);
}

#[test]
fn test_cmov_array_external() {
    use cmov::Cmov;
    let mut a = [1u8; 16];
    let b = [0u8; 16];
    a.cmovnz(&b, 1);
    assert_eq!(a, [0u8; 16]);

    let mut a = [1u8; 16];
    a.cmovnz(&b, 0);
    assert_eq!(a, [1u8; 16]);
}

#[test]
fn test_compaction_oram_block_large() {
    use oram::oblivious::compaction::compact_payload;
    use oram::testing::OramBlock;

    const P: usize = 16;
    let len = 160;
    let mut arr = vec![OramBlock::<P>::dummy(); len];
    let mut rng = StdRng::seed_from_u64(12345);
    let mut marks = vec![false; len];
    for i in 0..len {
        if rng.random::<bool>() {
            let mut tag = rng.random::<u64>();
            if tag == 0 {
                tag = 1;
            }
            arr[i] = OramBlock::real(tag, 0, rng.random::<u64>(), [0u8; P]);
            marks[i] = true;
        }
    }

    let mut payload = vec![0; len + 1];
    for i in 0..len {
        payload[i + 1] = payload[i] + usize::from(marks[i]);
    }

    compact_payload(&mut arr, &payload);

    for (i, b) in arr.iter().enumerate() {
        if b.tag == u64::MAX {
            panic!("CORRUPTION DETECTED at index {i} in compacted array!");
        }
    }
}
