// 基準測試伺服器
// 根據指定的協定和模式啟動對應的伺服器

use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;

use zenoh_playground::config::{BenchConfig, CliOverrides};
use zenoh_playground::protocols::{self, BenchMode, Protocol};

#[derive(Parser)]
#[command(name = "bench-server", about = "Benchmark echo server")]
struct Args {
    /// Communication protocol to use
    #[arg(long, value_enum, default_value_t = Protocol::ZenohTcp)]
    protocol: Protocol,

    /// Benchmark mode
    #[arg(long, value_enum, default_value_t = BenchMode::RequestReply)]
    mode: BenchMode,

    /// Payload size in bytes (pub/sub mode)
    #[arg(long, default_value_t = 1024)]
    payload_size: usize,

    /// Number of messages to publish (pub/sub mode)
    #[arg(long, default_value_t = 1000)]
    count: usize,

    /// Number of concurrent publishers (pub/sub mode)
    #[arg(long, default_value_t = 1)]
    publishers: usize,

    /// Duration in seconds for sustained load mode (overrides --count)
    #[arg(long)]
    duration: Option<u64>,

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
        "Starting bench server: protocol={}, mode={}",
        args.protocol, args.mode
    );

    match args.mode {
        BenchMode::RequestReply => {
            let server = protocols::create_server(&args.protocol, cfg).await?;
            server.serve().await
        }
        BenchMode::PubSub => {
            let publisher = protocols::create_publisher(&args.protocol, cfg).await?;
            if let Some(duration) = args.duration {
                publisher.start_timed(args.payload_size, duration, args.publishers).await
            } else {
                publisher.start(args.payload_size, args.count, args.publishers).await
            }
        }
    }
}
