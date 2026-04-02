// 基準測試客戶端
// 連線到伺服器並執行延遲與吞吐量測試

use anyhow::Result;
use clap::Parser;

use zenoh_playground::bench;
use zenoh_playground::protocols::{self, Protocol};

#[derive(Parser)]
#[command(name = "bench-client", about = "Benchmark client")]
struct Args {
    /// Communication protocol to use
    #[arg(long, value_enum, default_value_t = Protocol::ZenohTcp)]
    protocol: Protocol,

    /// Payload size in bytes per request
    #[arg(long, default_value_t = 1024)]
    payload_size: usize,

    /// Number of measured iterations
    #[arg(long, default_value_t = 1000)]
    iterations: usize,

    /// Number of warmup iterations (not counted in stats)
    #[arg(long, default_value_t = 100)]
    warmup: usize,

    /// Write JSON results to this file
    #[arg(long)]
    json_output: Option<String>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    println!(
        "Starting benchmark: protocol={}, payload={}B, iterations={}",
        args.protocol, args.payload_size, args.iterations
    );

    let client = protocols::create_client(&args.protocol).await?;

    let result = bench::run_benchmark(
        &client,
        &args.protocol.to_string(),
        args.payload_size,
        args.iterations,
        args.warmup,
    )
    .await?;

    bench::print_report(&result);

    // 輸出 JSON 結果（如果有指定）
    if let Some(path) = &args.json_output {
        let json = serde_json::to_string_pretty(&result)?;
        std::fs::write(path, &json)?;
        println!("Results written to {}", path);
    }

    Ok(())
}
