// Copyright (c) Meta Platforms, Inc. and affiliates.
// Copyright 2026 Google LLC
//
// This source code is dual-licensed under either the MIT license found in the
// LICENSE-MIT file in the root directory of this source tree or the Apache
// License, Version 2.0 found in the LICENSE-APACHE file in the root directory
// of this source tree. You may select, at your option, one of the above-listed licenses.

//! Shared ORAM block and sort-entry structures.

use crate::oblivious::reduction::Reducible;
use crate::oblivious_histogram::routing::OramAddress;
use cmov::Cmov as ScalarCmov;

/// The basic block stored in tree nodes and stash buffers.
///
/// Invariant: `tag == 0` encodes a dummy slot; `tag != 0` encodes a real block with PRF routing bits.
/// Uses 64-byte alignment to match hardware cache-line size and prevent cache-line splits in SIMD operations.
#[repr(C, align(64))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OramBlock<const K: usize = 16, V = u64> {
    /// PRF-derived routing tag (0 = dummy, non-zero = real).
    pub tag: u64,
    /// Resize epoch at which this block was created or migrated.
    pub epoch: u8,
    /// Aggregated payload value.
    pub value: V,
    /// Logical key payload of length `K` bytes.
    pub payload: [u8; K],
}

impl<const K: usize, V: crate::OramValue> Default for OramBlock<K, V> {
    fn default() -> Self {
        Self::dummy()
    }
}

impl<const K: usize, V: crate::OramValue> ScalarCmov for OramBlock<K, V> {
    #[inline(always)]
    fn cmovnz(&mut self, other: &Self, condition: u8) {
        use crate::oblivious::ct::Cmov;
        self.cmov(other, condition != 0);
    }
}

impl<const K: usize, V: crate::OramValue> crate::oblivious::ct::Cmov for OramBlock<K, V> {
    #[inline(always)]
    fn cmov(&mut self, other: &Self, choice: bool) {
        use cmov::Cmov;
        self.tag.cmovnz(&other.tag, u8::from(choice));
        self.epoch.cmovnz(&other.epoch, u8::from(choice));
        self.payload.cmovnz(&other.payload, u8::from(choice));
        self.value.cmovnz(&other.value, u8::from(choice));
    }
}

impl<const K: usize, V: crate::OramValue> Reducible<V> for OramBlock<K, V> {
    #[inline]
    fn ct_is_real(&self) -> u8 {
        self.tag.ct_is_real()
    }

    #[inline]
    fn ct_identity_eq(&self, other: &Self) -> u8 {
        crate::oblivious::ct::ct_eq_bytes(&self.payload, &other.payload)
    }

    #[inline]
    fn value_mut(&mut self) -> &mut V {
        &mut self.value
    }

    #[inline]
    fn epoch_mut(&mut self) -> &mut u8 {
        &mut self.epoch
    }

    #[inline]
    fn conditional_dummy(&mut self, cond: u8) {
        use cmov::Cmov;
        self.tag.cmovnz(&0, cond);
    }
}

impl<const K: usize, V: crate::OramValue> OramBlock<K, V> {
    #[inline]
    pub(crate) fn assign_if(&mut self, tag: u64, epoch: u8, value: V, payload: &[u8; K], cond: u8) {
        use crate::oblivious::ct::Cmov;
        let other = Self { tag, epoch, value, payload: *payload };
        self.cmov(&other, cond != 0);
    }

    #[inline]
    pub(crate) fn conditional_dummy(&mut self, cond: u8) {
        <Self as Reducible<V>>::conditional_dummy(self, cond);
    }

    /// Constructs a real block with the given routing tag, epoch, value, and payload ($O(1)$).
    #[inline]
    pub fn real(tag: u64, epoch: u8, value: V, payload: [u8; K]) -> Self {
        Self { tag, epoch, value, payload }
    }

    /// Constructs a dummy block (`tag = 0`, `value = V::default()`) ($O(1)$).
    #[inline]
    pub fn dummy() -> Self {
        Self { tag: 0, epoch: 0, value: V::default(), payload: [0u8; K] }
    }

    /// Returns a reference to the block key payload bytes ($O(1)$).
    #[inline]
    pub fn key(&self) -> &[u8; K] {
        &self.payload
    }

    /// Returns a mutable reference to the block key payload bytes ($O(1)$).
    #[inline]
    pub fn key_mut(&mut self) -> &mut [u8; K] {
        &mut self.payload
    }
}

/// 32-byte rich telemetry flow record value matching Section 6 of the paper.
///
/// Invariant: associative addition aggregates packet/byte counters, tracks timestamp ranges,
/// and computes bitwise OR of TCP flags in constant time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct FlowRecord {
    /// Total packet counter.
    pub packet_count: u64,
    /// Total byte counter across all packets.
    pub byte_sum: u64,
    /// Earliest observed timestamp in microseconds.
    pub first_seen: u32,
    /// Latest observed timestamp in microseconds.
    pub last_seen: u32,
    /// Number of distinct flow records merged.
    pub record_count: u32,
    /// Bitwise OR of observed TCP flags (SYN, ACK, FIN, RST, etc.).
    pub tcp_flags: u16,
    /// Padding to maintain 32-byte alignment.
    pub _padding: u16,
}

impl Default for FlowRecord {
    fn default() -> Self {
        Self {
            packet_count: 0,
            byte_sum: 0,
            first_seen: u32::MAX,
            last_seen: 0,
            record_count: 0,
            tcp_flags: 0,
            _padding: 0,
        }
    }
}

impl cmov::Cmov for FlowRecord {
    #[inline(always)]
    fn cmovnz(&mut self, other: &Self, condition: u8) {
        self.packet_count.cmovnz(&other.packet_count, condition);
        self.byte_sum.cmovnz(&other.byte_sum, condition);
        self.first_seen.cmovnz(&other.first_seen, condition);
        self.last_seen.cmovnz(&other.last_seen, condition);
        self.record_count.cmovnz(&other.record_count, condition);
        self.tcp_flags.cmovnz(&other.tcp_flags, condition);
    }
}

impl crate::oblivious::ct::Cmov for FlowRecord {
    #[inline(always)]
    fn cmov(&mut self, other: &Self, choice: bool) {
        use cmov::Cmov;
        self.cmovnz(other, u8::from(choice));
    }
}

impl std::ops::Add for FlowRecord {
    type Output = Self;

    #[inline]
    fn add(self, rhs: Self) -> Self {
        Self {
            packet_count: self.packet_count.wrapping_add(rhs.packet_count),
            byte_sum: self.byte_sum.wrapping_add(rhs.byte_sum),
            first_seen: crate::ct::ct_min_u32(self.first_seen, rhs.first_seen),
            last_seen: crate::ct::ct_max_u32(self.last_seen, rhs.last_seen),
            record_count: self.record_count.wrapping_add(rhs.record_count),
            tcp_flags: self.tcp_flags | rhs.tcp_flags,
            _padding: 0,
        }
    }
}
