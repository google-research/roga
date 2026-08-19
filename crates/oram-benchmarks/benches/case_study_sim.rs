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

use oram::{FlowRecord, ShardedObliviousHistogram};
use rand::{rngs::StdRng, SeedableRng};
use rand_distr::{Distribution, Zipf};
use std::time::Instant;

fn main() {
    println!("========================================================================");
    println!("  Case Study: Real-Time Oblivious DDoS Telemetry & Network Analysis");
    println!("========================================================================");

    let shard_count = 16;
    let batch_size = 4096;
    let total_capacity = 262_144;
    let per_shard_quota =
        ShardedObliviousHistogram::<16, 16>::suggested_per_shard_quota(batch_size, shard_count, 80);

    println!("Configuration:");
    println!("  - Target Architecture : Sharded Path OSAM (16 Shards, 80-bit Security)");
    println!("  - Batch Size          : {} streaming updates", batch_size);
    println!("  - Tree Capacity       : {} active flow blocks", total_capacity);
    println!("  - Per-Shard Quota     : {} blocks", per_shard_quota);

    let mut rng = StdRng::seed_from_u64(42);

    // --- Part 1: Standard 8-byte u64 Value ---
    println!("\n------------------------------------------------------------------------");
    println!("  [Part 1] Standard 8-byte Value (Packet Counter)");
    println!("------------------------------------------------------------------------");
    let mut sharded_8b = ShardedObliviousHistogram::<16, 16, 20, 64, u64>::new(
        shard_count,
        total_capacity,
        batch_size,
        per_shard_quota,
        &mut rng,
    );

    let n_packets = 500_000usize;
    let n_distinct_ips = 131_072usize;
    let zipf_normal = Zipf::new(n_distinct_ips as f64, 1.2).unwrap();

    let mut normal_keys = Vec::with_capacity(n_packets);
    for _ in 0..n_packets {
        let ip_idx = zipf_normal.sample(&mut rng) as usize - 1;
        let key = format!("10.0.{}.{}", (ip_idx >> 8) & 0xFF, ip_idx & 0xFF).into_bytes();
        normal_keys.push(key);
    }

    let start_normal = Instant::now();
    for chunk in normal_keys.chunks(batch_size) {
        for k in chunk {
            sharded_8b.append(k, 1);
        }
    }
    sharded_8b.flush();
    let elapsed_normal = start_normal.elapsed();
    let pps_normal = (n_packets as f64) / elapsed_normal.as_secs_f64();
    println!(
        "  -> Background Traffic Ingestion: {:.2?} | Throughput: {:.3} Mpps ({:.2} us/packet)",
        elapsed_normal,
        pps_normal / 1e6,
        elapsed_normal.as_micros() as f64 / n_packets as f64
    );

    let n_ddos_packets = 500_000usize;
    let zipf_ddos = Zipf::new(n_distinct_ips as f64, 2.8).unwrap();
    let mut ddos_keys = Vec::with_capacity(n_ddos_packets);
    for _ in 0..n_ddos_packets {
        let ip_idx = zipf_ddos.sample(&mut rng) as usize - 1;
        let key = format!("10.0.{}.{}", (ip_idx >> 8) & 0xFF, ip_idx & 0xFF).into_bytes();
        ddos_keys.push(key);
    }

    let start_ddos = Instant::now();
    for chunk in ddos_keys.chunks(batch_size) {
        for k in chunk {
            sharded_8b.append(k, 1);
        }
    }
    sharded_8b.flush();
    let elapsed_ddos = start_ddos.elapsed();
    let pps_ddos = (n_ddos_packets as f64) / elapsed_ddos.as_secs_f64();
    println!(
        "  -> DDoS Burst Traffic Ingestion: {:.2?} | Throughput: {:.3} Mpps ({:.2} us/packet)",
        elapsed_ddos,
        pps_ddos / 1e6,
        elapsed_ddos.as_micros() as f64 / n_ddos_packets as f64
    );

    // --- Part 2: Rich 32-byte FlowRecord Value (Section 6 of Paper) ---
    println!("\n------------------------------------------------------------------------");
    println!("  [Part 2] Rich 32-byte Value (FlowRecord: Pkts, Bytes, Timestamps, Flags)");
    println!("------------------------------------------------------------------------");

    let mut rng32 = StdRng::seed_from_u64(42);
    let mut sharded_32b = ShardedObliviousHistogram::<16, 16, 20, 64, FlowRecord>::new(
        shard_count,
        total_capacity,
        batch_size,
        per_shard_quota,
        &mut rng32,
    );

    let flow_req1 = FlowRecord {
        packet_count: 1,
        byte_sum: 1420,
        first_seen: 1000,
        last_seen: 1000,
        record_count: 1,
        tcp_flags: 0x02, // SYN
        _padding: 0,
    };

    let start_flow_normal = Instant::now();
    for chunk in normal_keys.chunks(batch_size) {
        for k in chunk {
            sharded_32b.append(k, flow_req1);
        }
    }
    sharded_32b.flush();
    let elapsed_flow_normal = start_flow_normal.elapsed();
    let pps_flow_normal = (n_packets as f64) / elapsed_flow_normal.as_secs_f64();
    println!(
        "  -> Background Traffic (32B Value): {:.2?} | Throughput: {:.3} Mpps ({:.2} us/packet)",
        elapsed_flow_normal,
        pps_flow_normal / 1e6,
        elapsed_flow_normal.as_micros() as f64 / n_packets as f64
    );

    let flow_req2 = FlowRecord {
        packet_count: 1,
        byte_sum: 64,
        first_seen: 1050,
        last_seen: 1100,
        record_count: 1,
        tcp_flags: 0x10, // ACK
        _padding: 0,
    };

    let start_flow_ddos = Instant::now();
    for chunk in ddos_keys.chunks(batch_size) {
        for k in chunk {
            sharded_32b.append(k, flow_req2);
        }
    }
    sharded_32b.flush();
    let elapsed_flow_ddos = start_flow_ddos.elapsed();
    let pps_flow_ddos = (n_ddos_packets as f64) / elapsed_flow_ddos.as_secs_f64();
    println!(
        "  -> DDoS Burst Traffic (32B Value): {:.2?} | Throughput: {:.3} Mpps ({:.2} us/packet)",
        elapsed_flow_ddos,
        pps_flow_ddos / 1e6,
        elapsed_flow_ddos.as_micros() as f64 / n_ddos_packets as f64
    );

    // Verify Readout on Victim Target IP
    let victim_key = b"10.0.0.0";
    let victim_readout = sharded_32b.read_total(victim_key);
    println!("\n[Readout Verification for Victim Target IP (10.0.0.0)]");
    println!("  - Total Packets Aggregated : {}", victim_readout.packet_count);
    println!("  - Total Bytes Aggregated   : {}", victim_readout.byte_sum);
    println!("  - First-Seen Timestamp     : {}", victim_readout.first_seen);
    println!("  - Last-Seen Timestamp      : {}", victim_readout.last_seen);
    println!("  - Record Count Aggregated  : {}", victim_readout.record_count);
    println!("  - Observed TCP Flags (OR)  : 0x{:02X}", victim_readout.tcp_flags);

    let perf_impact =
        (elapsed_flow_normal.as_secs_f64() / elapsed_normal.as_secs_f64() - 1.0) * 100.0;

    println!("\n========================================================================");
    println!("  CASE STUDY SUMMARY & KEY FINDINGS:");
    println!(
        "  1. 8-byte Value Throughput   : {:.3} Mpps ({:.2} us/pkt)",
        pps_normal / 1e6,
        elapsed_normal.as_micros() as f64 / n_packets as f64
    );
    println!(
        "  2. 32-byte Value Throughput  : {:.3} Mpps ({:.2} us/pkt)",
        pps_flow_normal / 1e6,
        elapsed_flow_normal.as_micros() as f64 / n_packets as f64
    );
    println!("  3. 32-byte Value Cost Impact : {:.1}% overhead over 8-byte value", perf_impact);
    println!("========================================================================");
}
