// 基準測試框架：計時、統計與報告

use std::time::Instant;

use anyhow::Result;
use rand::RngCore;
use serde::Serialize;

use crate::protocols::BenchClient;

/// 基準測試結果
#[derive(Debug, Serialize)]
pub struct BenchResult {
    pub protocol: String,
    pub payload_size: usize,
    pub iterations: usize,
    pub warmup: usize,
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub mean_us: f64,
    pub min_us: f64,
    pub max_us: f64,
    pub throughput_msg_per_sec: f64,
    pub throughput_mb_per_sec: f64,
}

/// 執行基準測試
pub async fn run_benchmark(
    transport: &BenchClient,
    protocol_name: &str,
    payload_size: usize,
    iterations: usize,
    warmup: usize,
) -> Result<BenchResult> {
    // 產生隨機 payload
    let mut payload = vec![0u8; payload_size];
    rand::rng().fill_bytes(&mut payload);

    // 暖機階段（不計入統計）
    println!("Warming up ({} iterations)...", warmup);
    for _ in 0..warmup {
        let reply = transport.send(&payload).await?;
        assert_eq!(reply.len(), payload.len(), "Echo reply size mismatch");
    }

    // 正式測試
    println!("Running benchmark ({} iterations, {} bytes payload)...", iterations, payload_size);
    let mut latencies_ns: Vec<u64> = Vec::with_capacity(iterations);
    let total_start = Instant::now();

    for _ in 0..iterations {
        let start = Instant::now();
        let reply = transport.send(&payload).await?;
        let elapsed = start.elapsed().as_nanos() as u64;
        latencies_ns.push(elapsed);

        // 驗證回應正確性
        if reply != payload {
            anyhow::bail!("Echo verification failed: reply does not match payload");
        }
    }

    let total_elapsed = total_start.elapsed();

    // 排序以計算百分位數
    latencies_ns.sort_unstable();

    let mean_ns = latencies_ns.iter().sum::<u64>() as f64 / iterations as f64;
    let total_bytes = payload_size * iterations * 2; // 來回各一次
    let total_secs = total_elapsed.as_secs_f64();

    let result = BenchResult {
        protocol: protocol_name.to_string(),
        payload_size,
        iterations,
        warmup,
        p50_us: percentile(&latencies_ns, 0.50) as f64 / 1000.0,
        p95_us: percentile(&latencies_ns, 0.95) as f64 / 1000.0,
        p99_us: percentile(&latencies_ns, 0.99) as f64 / 1000.0,
        mean_us: mean_ns / 1000.0,
        min_us: latencies_ns[0] as f64 / 1000.0,
        max_us: latencies_ns[latencies_ns.len() - 1] as f64 / 1000.0,
        throughput_msg_per_sec: iterations as f64 / total_secs,
        throughput_mb_per_sec: total_bytes as f64 / (1024.0 * 1024.0) / total_secs,
    };

    Ok(result)
}

/// 計算已排序陣列的百分位數
fn percentile(sorted: &[u64], p: f64) -> u64 {
    let idx = ((sorted.len() as f64) * p).ceil() as usize;
    let idx = idx.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

/// 將結果印出為表格
pub fn print_report(result: &BenchResult) {
    println!();
    println!("=== Benchmark Result ===");
    println!("Protocol:       {}", result.protocol);
    println!("Payload size:   {} bytes", result.payload_size);
    println!("Iterations:     {} (warmup: {})", result.iterations, result.warmup);
    println!("---");
    println!("Latency (us):");
    println!("  min:    {:>10.1}", result.min_us);
    println!("  p50:    {:>10.1}", result.p50_us);
    println!("  p95:    {:>10.1}", result.p95_us);
    println!("  p99:    {:>10.1}", result.p99_us);
    println!("  max:    {:>10.1}", result.max_us);
    println!("  mean:   {:>10.1}", result.mean_us);
    println!("---");
    println!("Throughput:");
    println!("  {:>10.1} msg/s", result.throughput_msg_per_sec);
    println!("  {:>10.2} MB/s", result.throughput_mb_per_sec);
    println!("========================");
}

/// 將多個結果印出為比較表格（依 p50 延遲排序）
pub fn print_comparison(results: &[BenchResult]) {
    if results.is_empty() {
        return;
    }

    let mut sorted: Vec<&BenchResult> = results.iter().collect();
    sorted.sort_by(|a, b| a.p50_us.partial_cmp(&b.p50_us).unwrap());

    let payload_desc = if sorted.iter().all(|r| r.payload_size == sorted[0].payload_size) {
        format!("{} bytes", sorted[0].payload_size)
    } else {
        "mixed".to_string()
    };

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║                    Benchmark Comparison (payload: {:>10})                ║", payload_desc);
    println!("╠════════════════╦══════════╦══════════╦══════════╦══════════╦═════════════════╣");
    println!("║ Protocol       ║  p50(us) ║  p95(us) ║  p99(us) ║ mean(us) ║     msg/s       ║");
    println!("╠════════════════╬══════════╬══════════╬══════════╬══════════╬═════════════════╣");

    for r in &sorted {
        println!(
            "║ {:<14} ║ {:>8.1} ║ {:>8.1} ║ {:>8.1} ║ {:>8.1} ║ {:>13.1}   ║",
            r.protocol, r.p50_us, r.p95_us, r.p99_us, r.mean_us, r.throughput_msg_per_sec
        );
    }

    println!("╚════════════════╩══════════╩══════════╩══════════╩══════════╩═════════════════╝");
}
