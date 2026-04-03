// 全協定基準測試
// 依序對所有協定執行 benchmark，最後輸出比較表格

use std::process::{Child, Command};
use std::time::Duration;

use anyhow::Result;
use clap::{Parser, ValueEnum};

use zenoh_playground::bench;
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

    /// Payload size in bytes per request
    #[arg(long, default_value_t = 1024)]
    payload_size: usize,

    /// Number of measured iterations per protocol
    #[arg(long, default_value_t = 500)]
    iterations: usize,

    /// Number of warmup iterations per protocol
    #[arg(long, default_value_t = 50)]
    warmup: usize,

    /// Write JSON results to this file
    #[arg(long)]
    json_output: Option<String>,
}

fn server_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("Failed to get current exe path");
    path.set_file_name("bench-server");
    path
}

fn start_server(protocol: &Protocol, mode: &str, payload_size: usize, count: usize) -> Result<Child> {
    let child = Command::new(server_binary_path())
        .args([
            "--protocol",
            &protocol.to_string(),
            "--mode",
            mode,
            "--payload-size",
            &payload_size.to_string(),
            "--count",
            &count.to_string(),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(child)
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== Running all protocol benchmarks ===");
    println!(
        "Mode: {:?}, Payload: {} bytes, Iterations: {}, Warmup: {}",
        args.mode, args.payload_size, args.iterations, args.warmup
    );
    println!();

    let run_rr = matches!(args.mode, RunMode::All | RunMode::RequestReply);
    let run_ps = matches!(args.mode, RunMode::All | RunMode::PubSub);

    let mut rr_results: Vec<bench::BenchResult> = Vec::new();
    let mut ps_results: Vec<bench::PubSubResult> = Vec::new();

    // ── Request/Reply 模式 ──
    if run_rr {
        println!("══════════════════════════════════════════");
        println!("  Request/Reply Benchmark");
        println!("══════════════════════════════════════════");

        for protocol in ALL_PROTOCOLS {
            println!("────────────────────────────────────────");
            println!("Protocol: {}", protocol);
            println!("────────────────────────────────────────");

            let mut server = match start_server(protocol, "request-reply", args.payload_size, 0) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  Failed to start server: {}", e);
                    continue;
                }
            };

            tokio::time::sleep(Duration::from_secs(2)).await;

            let result = async {
                let client = protocols::create_client(protocol).await?;
                let r = bench::run_benchmark(
                    &client,
                    &protocol.to_string(),
                    args.payload_size,
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
        println!();
    }

    // ── Pub/Sub 模式 ──
    if run_ps {
        println!("══════════════════════════════════════════");
        println!("  Pub/Sub Benchmark");
        println!("══════════════════════════════════════════");

        let total_messages = args.warmup + args.iterations;

        for protocol in ALL_PROTOCOLS {
            println!("────────────────────────────────────────");
            println!("Protocol: {}", protocol);
            println!("────────────────────────────────────────");

            let mut server = match start_server(
                protocol,
                "pub-sub",
                args.payload_size,
                total_messages,
            ) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("  Failed to start server: {}", e);
                    continue;
                }
            };

            tokio::time::sleep(Duration::from_secs(2)).await;

            let result = async {
                let subscriber = protocols::create_subscriber(protocol).await?;
                let r = bench::run_pubsub_benchmark(
                    &subscriber,
                    &protocol.to_string(),
                    args.payload_size,
                    args.iterations,
                    args.warmup,
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
    }

    // ── JSON 輸出 ──
    if let Some(path) = &args.json_output {
        #[derive(serde::Serialize)]
        struct AllResults {
            request_reply: Vec<bench::BenchResult>,
            pubsub: Vec<bench::PubSubResult>,
        }
        let all = AllResults {
            request_reply: rr_results,
            pubsub: ps_results,
        };
        let json = serde_json::to_string_pretty(&all)?;
        std::fs::write(path, &json)?;
        println!("\nResults written to {}", path);
    }

    Ok(())
}
