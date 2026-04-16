// 基準測試客戶端
// 連線到伺服器並執行延遲與吞吐量測試

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

use zenoh_playground::bench;
use zenoh_playground::config::{BenchConfig, CliOverrides};
use zenoh_playground::protocols::{self, BenchMode, Protocol};

#[derive(Parser)]
#[command(name = "bench-client", about = "Benchmark client")]
struct Args {
    /// Communication protocol to use
    #[arg(long, value_enum, default_value_t = Protocol::ZenohTcp)]
    protocol: Protocol,

    /// Benchmark mode
    #[arg(long, value_enum, default_value_t = BenchMode::RequestReply)]
    mode: BenchMode,

    /// Payload size in bytes per request
    #[arg(long, default_value_t = 1024)]
    payload_size: usize,

    /// Number of measured iterations
    #[arg(long, default_value_t = 1000)]
    iterations: usize,

    /// Number of warmup iterations (not counted in stats)
    #[arg(long, default_value_t = 100)]
    warmup: usize,

    /// Number of concurrent publishers (pub/sub mode, for display)
    #[arg(long, default_value_t = 1)]
    publishers: usize,

    /// Duration in seconds for sustained load mode (overrides --iterations)
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

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let mut cfg = BenchConfig::load(args.config.as_deref())?;
    cfg.apply_cli(CliOverrides {
        port_offset: args.port_offset,
        http_port: args.http_port,
        grpc_port: args.grpc_port,
        key_prefix: args.key_prefix,
    });
    let cfg = Arc::new(cfg);

    println!(
        "Starting benchmark: protocol={}, mode={}, payload={}B",
        args.protocol, args.mode, args.payload_size
    );

    match args.mode {
        BenchMode::RequestReply => {
            let client = protocols::create_client(&args.protocol, cfg).await?;
            let result = bench::run_benchmark(
                &client,
                &args.protocol.to_string(),
                args.payload_size,
                args.iterations,
                args.warmup,
            )
            .await?;

            bench::print_report(&result);

            if let Some(path) = &args.json_output {
                let json = serde_json::to_string_pretty(&result)?;
                std::fs::write(path, &json)?;
                println!("Results written to {}", path);
            }
        }
        BenchMode::PubSub => {
            let subscriber = protocols::create_subscriber(&args.protocol, cfg).await?;

            if let Some(duration) = args.duration {
                // Sustained load 模式
                let result = bench::run_sustained_benchmark(
                    &subscriber,
                    &args.protocol.to_string(),
                    args.payload_size,
                    duration,
                    args.window,
                    args.publishers,
                )
                .await?;

                bench::print_sustained_report(&result);

                if let Some(path) = &args.json_output {
                    let json = serde_json::to_string_pretty(&result)?;
                    std::fs::write(path, &json)?;
                    println!("Results written to {}", path);
                }
            } else {
                // 一般 count-based 模式
                let result = bench::run_pubsub_benchmark(
                    &subscriber,
                    &args.protocol.to_string(),
                    args.payload_size,
                    args.iterations,
                    args.warmup,
                    args.publishers,
                )
                .await?;

                bench::print_pubsub_report(&result);

                if let Some(path) = &args.json_output {
                    let json = serde_json::to_string_pretty(&result)?;
                    std::fs::write(path, &json)?;
                    println!("Results written to {}", path);
                }
            }
        }
    }

    Ok(())
}
