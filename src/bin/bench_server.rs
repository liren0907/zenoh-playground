// 基準測試伺服器
// 根據指定的協定啟動回聲伺服器

use anyhow::Result;
use clap::Parser;

// 引用 library crate 的模組
use zenoh_playground::protocols::{self, Protocol};

#[derive(Parser)]
#[command(name = "bench-server", about = "Benchmark echo server")]
struct Args {
    /// Communication protocol to use
    #[arg(long, value_enum, default_value_t = Protocol::ZenohTcp)]
    protocol: Protocol,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!("Starting bench server with protocol: {}", args.protocol);

    let server = protocols::create_server(&args.protocol).await?;
    server.serve().await
}
