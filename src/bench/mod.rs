// 基準測試框架：計時、統計與報告

use std::time::Instant;

use anyhow::Result;
use rand::RngCore;
use serde::Serialize;

use crate::protocols::{BenchClient, BenchSubscriber};

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

// ── Pub/Sub 模式（純吞吐量測量） ──

/// Pub/Sub 基準測試結果
#[derive(Debug, Serialize)]
pub struct PubSubResult {
    pub protocol: String,
    pub payload_size: usize,
    pub num_messages: usize,
    pub publishers: usize,
    pub total_duration_secs: f64,
    pub throughput_msg_per_sec: f64,
    pub throughput_mb_per_sec: f64,
}

/// 執行 Pub/Sub 基準測試（純吞吐量）
pub async fn run_pubsub_benchmark(
    subscriber: &BenchSubscriber,
    protocol_name: &str,
    payload_size: usize,
    num_messages: usize,
    warmup: usize,
    publishers: usize,
) -> Result<PubSubResult> {
    let total_count = warmup + num_messages;
    println!(
        "Receiving {} messages ({} warmup + {} measured, {} bytes payload, {} publisher(s))...",
        total_count, warmup, num_messages, payload_size, publishers
    );

    let total_start = Instant::now();
    let received = subscriber.subscribe(total_count, payload_size, publishers).await?;
    let total_elapsed = total_start.elapsed();

    if received < total_count {
        anyhow::bail!(
            "Only received {} of {} messages",
            received,
            total_count
        );
    }

    let total_bytes = payload_size * num_messages;
    let measured_secs = total_elapsed.as_secs_f64() * (num_messages as f64 / total_count as f64);

    let result = PubSubResult {
        protocol: protocol_name.to_string(),
        payload_size,
        num_messages,
        publishers,
        total_duration_secs: measured_secs,
        throughput_msg_per_sec: num_messages as f64 / measured_secs,
        throughput_mb_per_sec: total_bytes as f64 / (1024.0 * 1024.0) / measured_secs,
    };

    Ok(result)
}

/// 印出 Pub/Sub 單項結果
pub fn print_pubsub_report(result: &PubSubResult) {
    println!();
    println!("=== Pub/Sub Benchmark Result ===");
    println!("Protocol:       {}", result.protocol);
    println!("Payload size:   {} bytes", result.payload_size);
    println!("Publishers:     {}", result.publishers);
    println!("Messages:       {}", result.num_messages);
    println!("Duration:       {:.3}s", result.total_duration_secs);
    println!("---");
    println!("Throughput:");
    println!("  {:>10.1} msg/s", result.throughput_msg_per_sec);
    println!("  {:>10.2} MB/s", result.throughput_mb_per_sec);
    println!("================================");
}

/// 印出 Pub/Sub 比較表格（依吞吐量排序）
pub fn print_pubsub_comparison(results: &[PubSubResult]) {
    if results.is_empty() {
        return;
    }

    let mut sorted: Vec<&PubSubResult> = results.iter().collect();
    sorted.sort_by(|a, b| {
        b.throughput_msg_per_sec
            .partial_cmp(&a.throughput_msg_per_sec)
            .unwrap()
    });

    let payload_desc = if sorted
        .iter()
        .all(|r| r.payload_size == sorted[0].payload_size)
    {
        format!("{} bytes", sorted[0].payload_size)
    } else {
        "mixed".to_string()
    };

    println!();
    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║   Pub/Sub Throughput Comparison (payload: {:>10})   ║", payload_desc);
    println!("╠════════════════╦═════════════════╦═════════════════════╣");
    println!("║ Protocol       ║     msg/s       ║       MB/s          ║");
    println!("╠════════════════╬═════════════════╬═════════════════════╣");

    for r in &sorted {
        println!(
            "║ {:<14} ║ {:>13.1}   ║ {:>13.2}       ║",
            r.protocol, r.throughput_msg_per_sec, r.throughput_mb_per_sec
        );
    }

    println!("╚════════════════╩═════════════════╩═════════════════════╝");
}

// ── Payload Size Sweep 報表 ──

/// 格式化 payload 大小為人類可讀字串
fn format_payload_size(size: usize) -> String {
    if size >= 1_048_576 {
        format!("{}MB", size / 1_048_576)
    } else if size >= 1024 {
        format!("{}KB", size / 1024)
    } else {
        format!("{}B", size)
    }
}

/// 印出 Request/Reply sweep 矩陣（p50 延遲）
pub fn print_rr_sweep(results: &[BenchResult], payload_sizes: &[usize]) {
    if results.is_empty() || payload_sizes.is_empty() {
        return;
    }

    // 收集所有出現過的協定名稱（保持排序穩定）
    let mut protocols: Vec<String> = Vec::new();
    for r in results {
        if !protocols.contains(&r.protocol) {
            protocols.push(r.protocol.clone());
        }
    }

    let size_labels: Vec<String> = payload_sizes.iter().map(|s| format_payload_size(*s)).collect();

    // 標題
    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║              Request/Reply p50 Latency (us) by Payload Size                 ║");
    println!("╠════════════════╦{}╣", "══════════╦".repeat(payload_sizes.len()).trim_end_matches('╦').to_string() + "══════════");

    // 欄位標題
    print!("║ {:14} ║", "Protocol");
    for label in &size_labels {
        print!(" {:>8} ║", label);
    }
    println!();
    println!("╠════════════════╬{}╣", "══════════╬".repeat(payload_sizes.len()).trim_end_matches('╬').to_string() + "══════════");

    // 資料行
    for proto in &protocols {
        print!("║ {:<14} ║", proto);
        for size in payload_sizes {
            let val = results
                .iter()
                .find(|r| r.protocol == *proto && r.payload_size == *size)
                .map(|r| format!("{:>8.1}", r.p50_us))
                .unwrap_or_else(|| format!("{:>8}", "-"));
            print!(" {} ║", val);
        }
        println!();
    }

    println!("╚════════════════╩{}╝", "══════════╩".repeat(payload_sizes.len()).trim_end_matches('╩').to_string() + "══════════");
}

/// 印出 Pub/Sub sweep 矩陣（吞吐量 MB/s）
pub fn print_pubsub_sweep(results: &[PubSubResult], payload_sizes: &[usize]) {
    if results.is_empty() || payload_sizes.is_empty() {
        return;
    }

    let mut protocols: Vec<String> = Vec::new();
    for r in results {
        if !protocols.contains(&r.protocol) {
            protocols.push(r.protocol.clone());
        }
    }

    let size_labels: Vec<String> = payload_sizes.iter().map(|s| format_payload_size(*s)).collect();

    println!();
    println!("╔══════════════════════════════════════════════════════════════════════════════╗");
    println!("║              Pub/Sub Throughput (MB/s) by Payload Size                      ║");
    println!("╠════════════════╦{}╣", "══════════╦".repeat(payload_sizes.len()).trim_end_matches('╦').to_string() + "══════════");

    print!("║ {:14} ║", "Protocol");
    for label in &size_labels {
        print!(" {:>8} ║", label);
    }
    println!();
    println!("╠════════════════╬{}╣", "══════════╬".repeat(payload_sizes.len()).trim_end_matches('╬').to_string() + "══════════");

    for proto in &protocols {
        print!("║ {:<14} ║", proto);
        for size in payload_sizes {
            let val = results
                .iter()
                .find(|r| r.protocol == *proto && r.payload_size == *size)
                .map(|r| format!("{:>8.2}", r.throughput_mb_per_sec))
                .unwrap_or_else(|| format!("{:>8}", "-"));
            print!(" {} ║", val);
        }
        println!();
    }

    println!("╚════════════════╩{}╝", "══════════╩".repeat(payload_sizes.len()).trim_end_matches('╩').to_string() + "══════════");
}

// ── Sustained Load 模式 ──

/// 時間窗口快照
#[derive(Debug, Serialize)]
pub struct WindowSnapshot {
    pub window_start_secs: f64,
    pub window_end_secs: f64,
    pub messages: usize,
    pub throughput_msg_per_sec: f64,
    pub throughput_mb_per_sec: f64,
}

/// Sustained Load 基準測試結果
#[derive(Debug, Serialize)]
pub struct SustainedResult {
    pub protocol: String,
    pub payload_size: usize,
    pub publishers: usize,
    pub duration_secs: f64,
    pub windows: Vec<WindowSnapshot>,
    pub total_messages: usize,
    pub avg_throughput_msg_per_sec: f64,
    pub avg_throughput_mb_per_sec: f64,
}

/// 執行 Sustained Load 基準測試
pub async fn run_sustained_benchmark(
    subscriber: &BenchSubscriber,
    protocol_name: &str,
    payload_size: usize,
    duration_secs: u64,
    window_secs: u64,
    publishers: usize,
) -> Result<SustainedResult> {
    println!(
        "Sustained load test: {} bytes payload, {}s duration, {}s windows, {} publisher(s)...",
        payload_size, duration_secs, window_secs, publishers
    );

    let total_start = Instant::now();
    let window_duration = std::time::Duration::from_secs(window_secs);
    let total_duration = std::time::Duration::from_secs(duration_secs);
    let mut windows: Vec<WindowSnapshot> = Vec::new();
    let mut total_received: usize = 0;
    let mut window_start = Instant::now();
    let mut window_count: usize = 0;
    let mut window_idx: usize = 0;

    loop {
        // 檢查是否該記錄窗口
        if window_start.elapsed() >= window_duration {
            let w_start = window_idx as f64 * window_secs as f64;
            let w_end = w_start + window_secs as f64;
            let w_secs = window_start.elapsed().as_secs_f64();
            let msg_per_sec = window_count as f64 / w_secs;
            let mb_per_sec = (window_count * payload_size) as f64 / (1024.0 * 1024.0) / w_secs;

            println!(
                "  [{:>5.0}-{:>5.0}s] {:>10.1} msg/s  {:>10.2} MB/s  ({} msgs)",
                w_start, w_end, msg_per_sec, mb_per_sec, window_count
            );

            windows.push(WindowSnapshot {
                window_start_secs: w_start,
                window_end_secs: w_end,
                messages: window_count,
                throughput_msg_per_sec: msg_per_sec,
                throughput_mb_per_sec: mb_per_sec,
            });

            window_idx += 1;
            window_count = 0;
            window_start = Instant::now();

            if total_start.elapsed() >= total_duration {
                break;
            }
            continue;
        }

        // 嘗試接收訊息
        let remaining = window_duration.saturating_sub(window_start.elapsed());
        match tokio::time::timeout(remaining, subscriber.subscribe(1, payload_size, publishers))
            .await
        {
            Ok(Ok(n)) => {
                window_count += n;
                total_received += n;
            }
            Ok(Err(_)) => break,
            Err(_) => {} // 窗口超時
        }

        if total_start.elapsed() >= total_duration {
            // 記錄最後不完整窗口
            if window_count > 0 {
                let w_start = window_idx as f64 * window_secs as f64;
                let w_secs = window_start.elapsed().as_secs_f64();
                let w_end = w_start + w_secs;
                let msg_per_sec = window_count as f64 / w_secs;
                let mb_per_sec =
                    (window_count * payload_size) as f64 / (1024.0 * 1024.0) / w_secs;

                windows.push(WindowSnapshot {
                    window_start_secs: w_start,
                    window_end_secs: w_end,
                    messages: window_count,
                    throughput_msg_per_sec: msg_per_sec,
                    throughput_mb_per_sec: mb_per_sec,
                });
            }
            break;
        }
    }

    let actual_duration = total_start.elapsed().as_secs_f64();
    let avg_msg = if actual_duration > 0.0 {
        total_received as f64 / actual_duration
    } else {
        0.0
    };
    let avg_mb = if actual_duration > 0.0 {
        (total_received * payload_size) as f64 / (1024.0 * 1024.0) / actual_duration
    } else {
        0.0
    };

    Ok(SustainedResult {
        protocol: protocol_name.to_string(),
        payload_size,
        publishers,
        duration_secs: actual_duration,
        windows,
        total_messages: total_received,
        avg_throughput_msg_per_sec: avg_msg,
        avg_throughput_mb_per_sec: avg_mb,
    })
}

/// 印出 Sustained Load 單項結果
pub fn print_sustained_report(result: &SustainedResult) {
    println!();
    println!("=== Sustained Load Result ===");
    println!("Protocol:       {}", result.protocol);
    println!("Payload size:   {} bytes", result.payload_size);
    println!("Publishers:     {}", result.publishers);
    println!("Duration:       {:.1}s", result.duration_secs);
    println!("Total messages: {}", result.total_messages);
    println!("---");
    println!("Avg throughput:");
    println!("  {:>10.1} msg/s", result.avg_throughput_msg_per_sec);
    println!("  {:>10.2} MB/s", result.avg_throughput_mb_per_sec);

    if !result.windows.is_empty() {
        println!("---");
        println!("╔══════════════╦═════════════════╦═════════════════════╗");
        println!("║ Window       ║     msg/s       ║       MB/s          ║");
        println!("╠══════════════╬═════════════════╬═════════════════════╣");
        for w in &result.windows {
            println!(
                "║ {:>5.0}-{:<5.0}s ║ {:>13.1}   ║ {:>13.2}       ║",
                w.window_start_secs, w.window_end_secs, w.throughput_msg_per_sec, w.throughput_mb_per_sec
            );
        }
        println!("╚══════════════╩═════════════════╩═════════════════════╝");
    }

    println!("=============================");
}

/// 印出 Sustained Load 比較表格（依平均吞吐量排序）
pub fn print_sustained_comparison(results: &[SustainedResult]) {
    if results.is_empty() {
        return;
    }

    let mut sorted: Vec<&SustainedResult> = results.iter().collect();
    sorted.sort_by(|a, b| {
        b.avg_throughput_msg_per_sec
            .partial_cmp(&a.avg_throughput_msg_per_sec)
            .unwrap()
    });

    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║   Sustained Load Comparison (avg throughput)                    ║");
    println!("╠════════════════╦═════════════════╦═════════════════╦════════════╣");
    println!("║ Protocol       ║     msg/s       ║       MB/s      ║ duration   ║");
    println!("╠════════════════╬═════════════════╬═════════════════╬════════════╣");

    for r in &sorted {
        println!(
            "║ {:<14} ║ {:>13.1}   ║ {:>13.2}   ║ {:>7.1}s   ║",
            r.protocol, r.avg_throughput_msg_per_sec, r.avg_throughput_mb_per_sec, r.duration_secs
        );
    }

    println!("╚════════════════╩═════════════════╩═════════════════╩════════════╝");
}
