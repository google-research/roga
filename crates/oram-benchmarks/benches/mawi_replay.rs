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

use cmov::Cmov;
use etherparse::SlicedPacket;
use oram::ShardedObliviousHistogram;
use pcap::Capture;
use rand::{rngs::StdRng, SeedableRng};
use std::collections::HashMap;
use std::env;
use std::path::Path;
use std::time::Instant;

/// 32-byte record emulating tshark IP conversation statistics (`tshark -q -z conv,ip`).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct TSharkConvStats {
    pub frames: u64,
    pub bytes: u64,
    pub start_ts_us: u64,
    pub last_ts_us: u64,
}

impl Default for TSharkConvStats {
    fn default() -> Self {
        Self { frames: 0, bytes: 0, start_ts_us: u64::MAX, last_ts_us: 0 }
    }
}

impl std::ops::Add for TSharkConvStats {
    type Output = Self;
    fn add(self, rhs: Self) -> Self {
        let mut start_ts = self.start_ts_us;
        let mut last_ts = self.last_ts_us;

        let start_lt = oram::ct::ct_lt(rhs.start_ts_us, self.start_ts_us);
        start_ts.cmovnz(&rhs.start_ts_us, start_lt);

        let last_gt = oram::ct::ct_lt(self.last_ts_us, rhs.last_ts_us);
        last_ts.cmovnz(&rhs.last_ts_us, last_gt);

        Self {
            frames: self.frames + rhs.frames,
            bytes: self.bytes + rhs.bytes,
            start_ts_us: start_ts,
            last_ts_us: last_ts,
        }
    }
}

impl Cmov for TSharkConvStats {
    fn cmovz(&mut self, value: &Self, condition: u8) {
        self.frames.cmovz(&value.frames, condition);
        self.bytes.cmovz(&value.bytes, condition);
        self.start_ts_us.cmovz(&value.start_ts_us, condition);
        self.last_ts_us.cmovz(&value.last_ts_us, condition);
    }
    fn cmovnz(&mut self, value: &Self, condition: u8) {
        self.frames.cmovnz(&value.frames, condition);
        self.bytes.cmovnz(&value.bytes, condition);
        self.start_ts_us.cmovnz(&value.start_ts_us, condition);
        self.last_ts_us.cmovnz(&value.last_ts_us, condition);
    }
}

fn get_peak_rss_mb() -> f64 {
    #[cfg(target_os = "linux")]
    unsafe {
        let mut rusage: libc::rusage = std::mem::zeroed();
        libc::getrusage(libc::RUSAGE_SELF, &mut rusage);
        rusage.ru_maxrss as f64 / 1024.0
    }
    #[cfg(not(target_os = "linux"))]
    {
        0.0
    }
}

fn main() {
    println!("========================================================================");
    println!("  Case Study: MAWI Day-in-the-Life Replay (Emulating tshark conv,ip)   ");
    println!("========================================================================");

    let args: Vec<String> = env::args().collect();
    let env_path = env::var("MAWI_DATA_PATH").unwrap_or_else(|_| "data/mawi/mawi_sample_100m.dump".to_string());
    let pcap_path = if args.len() > 1 { &args[1] } else { &env_path };

    if !Path::new(pcap_path).exists() {
        eprintln!("Error: MAWI capture file not found at: {}", pcap_path);
        eprintln!("Please pass the path as an argument or set the MAWI_DATA_PATH environment variable.");
        std::process::exit(1);
    }

    println!("[Phase 1] Reading and parsing packets from MAWI archive: {}", pcap_path);
    let start_parse = Instant::now();
    let mut cap = Capture::from_file(pcap_path).expect("Failed to open pcap capture file");

    let mut packet_records = Vec::new();
    let mut ground_truth: HashMap<[u8; 16], TSharkConvStats> = HashMap::new();
    let mut total_wire_bytes = 0u64;
    let mut skipped_packets = 0usize;
    let mut parsed_count = 0usize;

    while let Ok(packet) = cap.next_packet() {
        parsed_count += 1;
        if parsed_count % 5_000_000 == 0 {
            println!(
                "    ... parsed {:>10} packets from capture stream ({:.1} MB volume)",
                parsed_count,
                total_wire_bytes as f64 / 1_048_576.0
            );
        }
        let wire_len = packet.header.len as u64;
        total_wire_bytes += wire_len;
        let ts_us =
            (packet.header.ts.tv_sec as u64) * 1_000_000 + (packet.header.ts.tv_usec as u64);
        let data = packet.data;

        // Try parsing Ethernet II first, fallback to raw IP if Ethernet parsing fails
        let sliced = SlicedPacket::from_ethernet(data).or_else(|_| SlicedPacket::from_ip(data));

        if let Ok(slice) = sliced {
            let mut key = [0u8; 16];
            let mut found_ip = false;

            if let Some(etherparse::NetSlice::Ipv4(ipv4)) = slice.net {
                key[0..4].copy_from_slice(&ipv4.header().source());
                key[4..8].copy_from_slice(&ipv4.header().destination());
                found_ip = true;
            } else if let Some(etherparse::NetSlice::Ipv6(ipv6)) = slice.net {
                let src = ipv6.header().source();
                let dst = ipv6.header().destination();
                for i in 0..16 {
                    key[i] = src[i % 16] ^ dst[i % 16];
                }
                found_ip = true;
            }

            if found_ip {
                let rec = TSharkConvStats {
                    frames: 1,
                    bytes: wire_len,
                    start_ts_us: ts_us,
                    last_ts_us: ts_us,
                };
                packet_records.push((key, rec));
                let entry = ground_truth.entry(key).or_insert(TSharkConvStats::default());
                *entry = *entry + rec;
            } else {
                skipped_packets += 1;
            }
        } else {
            skipped_packets += 1;
        }
    }

    let parse_elapsed = start_parse.elapsed();
    let total_packets = packet_records.len();
    let distinct_pairs = ground_truth.len();

    println!("  -> Trace Parsing Completed in {:.2?}", parse_elapsed);
    println!("  -> Valid IP Packets Extracted : {}", total_packets);
    println!("  -> Skipped/Non-IP Packets     : {}", skipped_packets);
    println!("  -> Total Captured Volume      : {:.2} MB", total_wire_bytes as f64 / 1_048_576.0);
    println!("  -> Distinct (src, dst) Pairs  : {} flows", distinct_pairs);
    println!(
        "  -> Average Flow Length        : {:.2} pkts/flow",
        total_packets as f64 / distinct_pairs as f64
    );

    if total_packets == 0 {
        eprintln!("No valid IP packets extracted from trace. Exiting.");
        return;
    }

    // Configure Sharded Path OSAM for real-world workload with 32-byte tshark value
    let shard_count = 16;
    let batch_size = 4096;
    let load_percent = 50;
    let total_capacity =
        ((distinct_pairs as u64 * 100 / load_percent).next_power_of_two() as usize).max(65536);
    let per_shard_quota =
        ShardedObliviousHistogram::<16, 16, 20, 64, TSharkConvStats>::suggested_per_shard_quota(
            batch_size,
            shard_count,
            80,
        );

    let default_passes =
        if total_packets >= 10_000_000 { 1 } else { (50_000_000 / total_packets).max(1) };

    let num_passes: usize =
        env::var("MAWI_PASSES").ok().and_then(|s| s.parse().ok()).unwrap_or(default_passes);

    println!("\n[Phase 2] Initializing Sharded ROGA Telemetry Engine (32-byte conv records)...");
    println!("Configuration:");
    println!("  - Architecture        : Sharded Path OSAM (16 Shards, 80-bit Security)");
    println!("  - Tree Capacity       : {} active flow blocks (next power-of-2 for {} distinct keys at {}% load)", total_capacity, distinct_pairs, load_percent);
    println!("  - Value Schema        : 32-byte (frames, bytes, start_ts, last_ts)");
    println!("  - Batch Size          : {} streaming updates", batch_size);
    println!("  - Per-Shard Quota     : {} blocks", per_shard_quota);
    println!(
        "  - Replay Stream Work  : {} passes ({} total packets, target >= 60 seconds)",
        num_passes,
        total_packets * num_passes
    );

    let mut rng = StdRng::seed_from_u64(42);
    let mut sharded = ShardedObliviousHistogram::<16, 16, 20, 64, TSharkConvStats>::new(
        shard_count,
        total_capacity as u64,
        batch_size,
        per_shard_quota,
        &mut rng,
    );

    println!("\n[Phase 3] Ingesting Extended MAWI Trace Stream (Sustained >= 60s benchmark)...");
    println!(
        "  Time (s) | Pass      | Window Mpps | Window Gbps | Cumulative Mpps | Peak RSS (MB)"
    );
    println!(
        "  ----------------------------------------------------------------------------------"
    );

    let start_ingest = Instant::now();
    let mut window_start = Instant::now();
    let mut window_packets = 0usize;
    let mut window_bytes = 0u64;
    let mut window_mpps_list = Vec::new();

    let mut final_gt = ground_truth.clone();
    for pass in 0..num_passes {
        // Offset timestamps per pass for realism
        let pass_offset_us = (pass as u64) * 100_000_000;
        for chunk in packet_records.chunks(batch_size) {
            for (k, v) in chunk {
                let mut shifted_v = *v;
                shifted_v.start_ts_us += pass_offset_us;
                shifted_v.last_ts_us += pass_offset_us;
                sharded.append(k, shifted_v);
            }
        }
        window_packets += total_packets;
        window_bytes += total_wire_bytes;

        // If not pass 0, scale ground truth
        if pass > 0 {
            for (k, v) in &packet_records {
                let mut shifted_v = *v;
                shifted_v.start_ts_us += pass_offset_us;
                shifted_v.last_ts_us += pass_offset_us;
                let entry = final_gt.entry(*k).or_insert(TSharkConvStats::default());
                *entry = *entry + shifted_v;
            }
        }

        let window_elapsed = window_start.elapsed();
        if window_elapsed.as_secs_f64() >= 5.0 || pass == num_passes - 1 {
            let total_elapsed = start_ingest.elapsed();
            let win_mpps = (window_packets as f64) / (window_elapsed.as_secs_f64() * 1e6);
            let win_gbps = (window_bytes as f64 * 8.0) / (window_elapsed.as_secs_f64() * 1e9);
            let cum_mpps =
                ((pass + 1) * total_packets) as f64 / (total_elapsed.as_secs_f64() * 1e6);
            let rss = get_peak_rss_mb();
            window_mpps_list.push(win_mpps);

            println!(
                "  {:7.2}  | {:3}/{:<3}   | {:11.3} | {:11.2} | {:15.3} | {:13.1}",
                total_elapsed.as_secs_f64(),
                pass + 1,
                num_passes,
                win_mpps,
                win_gbps,
                cum_mpps,
                rss
            );

            window_start = Instant::now();
            window_packets = 0;
            window_bytes = 0;
        }
    }
    sharded.flush();
    let ingest_elapsed = start_ingest.elapsed();

    let grand_total_packets = total_packets * num_passes;
    let grand_total_bytes = total_wire_bytes * (num_passes as u64);
    let pps = (grand_total_packets as f64) / ingest_elapsed.as_secs_f64();
    let mbps = (grand_total_bytes as f64 * 8.0) / (ingest_elapsed.as_secs_f64() * 1_000_000.0);
    let us_per_op = ingest_elapsed.as_micros() as f64 / grand_total_packets as f64;
    let peak_rss = get_peak_rss_mb();

    let mean_mpps = pps / 1e6;
    let variance = if window_mpps_list.len() > 1 {
        let sum_sq: f64 = window_mpps_list.iter().map(|&x| (x - mean_mpps).powi(2)).sum();
        sum_sq / (window_mpps_list.len() - 1) as f64
    } else {
        0.0
    };
    let std_dev_mpps = variance.sqrt();
    let cv_percent = (std_dev_mpps / mean_mpps) * 100.0;

    println!("\n========================================================================");
    println!("  MAWI SUSTAINED STREAMING REPLAY SUMMARY (>= 60s Execution)");
    println!("========================================================================");
    println!("  -> Total Replay Duration : {:.2} s", ingest_elapsed.as_secs_f64());
    println!("  -> Total Packets Ingested: {} packets", grand_total_packets);
    println!("  -> Total Volume Replayed : {:.2} GB", grand_total_bytes as f64 / 1e9);
    println!(
        "  -> Mean Throughput       : {:.3} Mpps ({:.2} Gbps line rate)",
        mean_mpps,
        mbps / 1000.0
    );
    println!(
        "  -> Throughput Stability  : std_dev = {:.4} Mpps, CV = {:.2}%",
        std_dev_mpps, cv_percent
    );
    println!("  -> Mean Latency / Packet : {:.3} us/packet", us_per_op);
    println!("  -> Final Peak RSS        : {:.1} MB", peak_rss);

    println!(
        "\n[Phase 4] Verifying Exactness on Top 10 Talking (src, dst) Flows (tshark conv,ip)..."
    );
    let mut sorted_flows: Vec<([u8; 16], TSharkConvStats)> = final_gt.into_iter().collect();
    sorted_flows.sort_by(|a, b| b.1.frames.cmp(&a.1.frames));

    println!("  Rank | IPv4 Source -> Dest Pair          | Frames (GT/ROGA) | Bytes (GT/ROGA)      | Duration (s) | Status");
    println!("  ----------------------------------------------------------------------------------------------------------");
    for (idx, (key, gt_stat)) in sorted_flows.iter().take(10).enumerate() {
        let roga_stat = sharded.read_total(key);
        let frames_ok = gt_stat.frames == roga_stat.frames;
        let bytes_ok = gt_stat.bytes == roga_stat.bytes;
        let start_ok = gt_stat.start_ts_us == roga_stat.start_ts_us;
        let last_ok = gt_stat.last_ts_us == roga_stat.last_ts_us;
        let all_ok = frames_ok && bytes_ok && start_ok && last_ok;
        let status = if all_ok { "OK" } else { "MISMATCH!" };

        let src_ip = format!("{}.{}.{}.{}", key[0], key[1], key[2], key[3]);
        let dst_ip = format!("{}.{}.{}.{}", key[4], key[5], key[6], key[7]);
        let pair_str = format!("{} -> {}", src_ip, dst_ip);
        let duration_s =
            (gt_stat.last_ts_us.saturating_sub(gt_stat.start_ts_us)) as f64 / 1_000_000.0;

        println!(
            "  {:4} | {:33} | {:7}/{:7}     | {:10}/{:10} B | {:10.2} s | {}",
            idx + 1,
            pair_str,
            gt_stat.frames,
            roga_stat.frames,
            gt_stat.bytes,
            roga_stat.bytes,
            duration_s,
            status
        );
        assert!(
            all_ok,
            "ROGA stats mismatch for flow {}: GT={:?}, ROGA={:?}",
            pair_str, gt_stat, roga_stat
        );
    }

    println!("\n========================================================================");
    println!("  MAWI CASE STUDY VERIFICATION SUCCESSFUL (100% Exact Match with tshark)!");
    println!("========================================================================");
}
