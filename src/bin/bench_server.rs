// 基準測試伺服器
// 根據指定的協定和模式啟動對應的伺服器

use anyhow::Result;
use clap::Parser;

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
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!(
        "Starting bench server: protocol={}, mode={}",
        args.protocol, args.mode
    );

    match args.mode {
        BenchMode::RequestReply => {
            let server = protocols::create_server(&args.protocol).await?;
            server.serve().await
        }
        BenchMode::PubSub => {
            let publisher = protocols::create_publisher(&args.protocol).await?;
            if let Some(duration) = args.duration {
                publisher.start_timed(args.payload_size, duration, args.publishers).await
            } else {
                publisher.start(args.payload_size, args.count, args.publishers).await
            }
        }
    }
}
