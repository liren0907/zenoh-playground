// 全協定基準測試
// 依序對所有協定執行 benchmark，支援多 payload 大小掃描

use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use zenoh_playground::bench;
use zenoh_playground::config::{BenchConfig, CliOverrides};
use zenoh_playground::protocols::{self, ALL_PROTOCOLS, Protocol};

#[derive(Debug, Clone, ValueEnum)]
enum RunMode {
    All,
    RequestReply,
    PubSub,
}

#[derive(Parser)]
#[command(name = "bench-all", about = "Run benchmarks for all protocols")]
struct Args {
    /// Which benchmark modes to run
    #[arg(long, value_enum, default_value_t = RunMode::All)]
    mode: RunMode,

    /// Single payload size in bytes (shorthand, overridden by --payload-sizes)
    #[arg(long)]
    payload_size: Option<usize>,

    /// Comma-separated payload sizes to sweep (e.g., 64,1024,65536)
    #[arg(long, value_delimiter = ',')]
    payload_sizes: Option<Vec<usize>>,

    /// Number of measured iterations per protocol
    #[arg(long, default_value_t = 500)]
    iterations: usize,

    /// Number of warmup iterations per protocol
    #[arg(long, default_value_t = 50)]
    warmup: usize,

    /// Number of concurrent publishers (pub/sub mode)
    #[arg(long, default_value_t = 1)]
    publishers: usize,

    /// Duration in seconds for sustained load mode (overrides --iterations for pub-sub)
    #[arg(long)]
    duration: Option<u64>,

    /// Window size in seconds for sustained load reporting
    #[arg(long, default_value_t = 5)]
    window: u64,

    /// Write JSON results to this file
    #[arg(long)]
    json_output: Option<String>,

    /// Path to bench.yaml config file (default: ./bench.yaml if exists)
    #[arg(long)]
    config: Option<PathBuf>,

    /// Add N to every protocol port (escape-hatch for port conflicts)
    #[arg(long)]
    port_offset: Option<u16>,

    /// Override HTTP port
    #[arg(long)]
    http_port: Option<u16>,

    /// Override gRPC port
    #[arg(long)]
    grpc_port: Option<u16>,

    /// Override Zenoh key prefix
    #[arg(long)]
    key_prefix: Option<String>,
}

fn server_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("Failed to get current exe path");
    path.set_file_name("bench-server");
    path
}

/// 組出要透傳給 child process 的設定 flag
fn config_passthrough_args(args: &Args) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(p) = &args.config {
        out.push("--config".to_string());
        out.push(p.to_string_lossy().to_string());
    }
    if let Some(o) = args.port_offset {
        out.push("--port-offset".to_string());
        out.push(o.to_string());
    }
    if let Some(p) = args.http_port {
        out.push("--http-port".to_string());
        out.push(p.to_string());
    }
    if let Some(p) = args.grpc_port {
        out.push("--grpc-port".to_string());
        out.push(p.to_string());
    }
    if let Some(k) = &args.key_prefix {
        out.push("--key-prefix".to_string());
        out.push(k.clone());
    }
    out
}

fn start_server(
    protocol: &Protocol,
    mode: &str,
    payload_size: usize,
    count: usize,
    publishers: usize,
    duration: Option<u64>,
    cfg_args: &[String],
) -> Result<Child> {
    let mut args = vec![
        "--protocol".to_string(),
        protocol.to_string(),
        "--mode".to_string(),
        mode.to_string(),
        "--payload-size".to_string(),
        payload_size.to_string(),
        "--count".to_string(),
        count.to_string(),
        "--publishers".to_string(),
        publishers.to_string(),
    ];

    if let Some(d) = duration {
        args.push("--duration".to_string());
        args.push(d.to_string());
    }

    // 透傳 config 相關的 flag
    args.extend_from_slice(cfg_args);

    let child = Command::new(server_binary_path())
        .args(&args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(child)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    // 載入設定 + 套用 CLI 覆寫（本 process 自己用）
    let mut cfg = BenchConfig::load(args.config.as_deref())?;
    cfg.apply_cli(CliOverrides {
        port_offset: args.port_offset,
        http_port: args.http_port,
        grpc_port: args.grpc_port,
        key_prefix: args.key_prefix.clone(),
    });
    let cfg = Arc::new(cfg);

    // 準備要透傳給 child process 的 flag
    let cfg_args = config_passthrough_args(&args);

    // 決定要測試的 payload 大小列表
    let payload_sizes: Vec<usize> = if let Some(sizes) = &args.payload_sizes {
        sizes.clone()
    } else if let Some(size) = args.payload_size {
        vec![size]
    } else {
        vec![64, 1024, 4096, 65536, 1048576]
    };

    let is_sweep = payload_sizes.len() > 1;

    println!("=== Running all protocol benchmarks ===");
    println!(
        "Mode: {:?}, Payload sizes: {:?}, Iterations: {}, Warmup: {}",
        args.mode, payload_sizes, args.iterations, args.warmup
    );
    println!();

    let run_rr = matches!(args.mode, RunMode::All | RunMode::RequestReply);
    let run_ps = matches!(args.mode, RunMode::All | RunMode::PubSub);

    let mut all_rr_results: Vec<bench::BenchResult> = Vec::new();
    let mut all_ps_results: Vec<bench::PubSubResult> = Vec::new();

    for &payload_size in &payload_sizes {
        if is_sweep {
            println!("████████████████████████████████████████████████");
            println!("  Payload size: {} bytes", payload_size);
            println!("████████████████████████████████████████████████");
            println!();
        }

        // ── Request/Reply 模式 ──
        if run_rr {
            println!("══════════════════════════════════════════");
            println!("  Request/Reply Benchmark (payload: {} bytes)", payload_size);
            println!("══════════════════════════════════════════");

            let mut rr_results: Vec<bench::BenchResult> = Vec::new();

            for protocol in ALL_PROTOCOLS {
                println!("────────────────────────────────────────");
                println!("Protocol: {}", protocol);
                println!("────────────────────────────────────────");

                let mut server = match start_server(protocol, "request-reply", payload_size, 0, 1, None, &cfg_args) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("  Failed to start server: {}", e);
                        continue;
                    }
                };

                tokio::time::sleep(Duration::from_secs(2)).await;

                let result = async {
                    let client = protocols::create_client(protocol, cfg.clone()).await?;
                    let r = bench::run_benchmark(
                        &client,
                        &protocol.to_string(),
                        payload_size,
                        args.iterations,
                        args.warmup,
                    )
                    .await;
                    drop(client);
                    r
                }
                .await;

                let _ = server.kill();
                let _ = server.wait();
                tokio::time::sleep(Duration::from_secs(2)).await;

                match result {
                    Ok(r) => {
                        bench::print_report(&r);
                        rr_results.push(r);
                    }
                    Err(e) => eprintln!("  Benchmark failed for {}: {}", protocol, e),
                }
                println!();
            }

            bench::print_comparison(&rr_results);
            all_rr_results.extend(rr_results);
            println!();
        }

        // ── Pub/Sub 模式 ──
        if run_ps {
            println!("══════════════════════════════════════════");
            println!("  Pub/Sub Benchmark (payload: {} bytes)", payload_size);
            println!("══════════════════════════════════════════");

            let total_messages = args.warmup + args.iterations;
            let mut ps_results: Vec<bench::PubSubResult> = Vec::new();

            for protocol in ALL_PROTOCOLS {
                println!("────────────────────────────────────────");
                println!("Protocol: {}", protocol);
                println!("────────────────────────────────────────");

                let mut server = match start_server(
                    protocol,
                    "pub-sub",
                    payload_size,
                    total_messages,
                    args.publishers,
                    None,
                    &cfg_args,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("  Failed to start server: {}", e);
                        continue;
                    }
                };

                tokio::time::sleep(Duration::from_secs(2)).await;

                let result = async {
                    let subscriber = protocols::create_subscriber(protocol, cfg.clone()).await?;
                    let r = bench::run_pubsub_benchmark(
                        &subscriber,
                        &protocol.to_string(),
                        payload_size,
                        args.iterations,
                        args.warmup,
                        args.publishers,
                    )
                    .await;
                    drop(subscriber);
                    r
                }
                .await;

                let _ = server.kill();
                let _ = server.wait();
                tokio::time::sleep(Duration::from_secs(2)).await;

                match result {
                    Ok(r) => {
                        bench::print_pubsub_report(&r);
                        ps_results.push(r);
                    }
                    Err(e) => eprintln!("  Benchmark failed for {}: {}", protocol, e),
                }
                println!();
            }

            bench::print_pubsub_comparison(&ps_results);
            all_ps_results.extend(ps_results);
            println!();
        }
    }

    // ── Sustained Load 模式 ──
    let mut sustained_results: Vec<bench::SustainedResult> = Vec::new();

    if let Some(duration) = args.duration {
        if run_ps {
            // 只用第一個 payload size 進行持續測試
            let payload_size = payload_sizes[0];

            println!("══════════════════════════════════════════");
            println!(
                "  Sustained Load (payload: {} bytes, {}s)",
                payload_size, duration
            );
            println!("══════════════════════════════════════════");

            for protocol in ALL_PROTOCOLS {
                println!("────────────────────────────────────────");
                println!("Protocol: {}", protocol);
                println!("────────────────────────────────────────");

                let mut server = match start_server(
                    protocol,
                    "pub-sub",
                    payload_size,
                    0,
                    args.publishers,
                    Some(duration + 5), // 給 server 多 5 秒確保不會提前停止
                    &cfg_args,
                ) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("  Failed to start server: {}", e);
                        continue;
                    }
                };

                tokio::time::sleep(Duration::from_secs(2)).await;

                let result = async {
                    let subscriber = protocols::create_subscriber(protocol, cfg.clone()).await?;
                    let r = bench::run_sustained_benchmark(
                        &subscriber,
                        &protocol.to_string(),
                        payload_size,
                        duration,
                        args.window,
                        args.publishers,
                    )
                    .await;
                    drop(subscriber);
                    r
                }
                .await;

                let _ = server.kill();
                let _ = server.wait();
                tokio::time::sleep(Duration::from_secs(2)).await;

                match result {
                    Ok(r) => {
                        bench::print_sustained_report(&r);
                        sustained_results.push(r);
                    }
                    Err(e) => eprintln!("  Sustained benchmark failed for {}: {}", protocol, e),
                }
                println!();
            }

            bench::print_sustained_comparison(&sustained_results);
            println!();
        }
    }

    // ── Sweep 矩陣總覽 ──
    if is_sweep {
        if run_rr {
            bench::print_rr_sweep(&all_rr_results, &payload_sizes);
        }
        if run_ps {
            bench::print_pubsub_sweep(&all_ps_results, &payload_sizes);
        }
    }

    // ── JSON 輸出 ──
    if let Some(path) = &args.json_output {
        #[derive(serde::Serialize)]
        struct AllResults {
            request_reply: Vec<bench::BenchResult>,
            pubsub: Vec<bench::PubSubResult>,
            sustained: Vec<bench::SustainedResult>,
        }
        let all = AllResults {
            request_reply: all_rr_results,
            pubsub: all_ps_results,
            sustained: sustained_results,
        };
        let json = serde_json::to_string_pretty(&all)?;
        std::fs::write(path, &json)?;
        println!("\nResults written to {}", path);
    }

    Ok(())
}
