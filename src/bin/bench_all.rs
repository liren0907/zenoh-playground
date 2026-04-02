// 全協定基準測試
// 依序對所有協定執行 benchmark，最後輸出比較表格

use std::process::{Child, Command};
use std::time::Duration;

use anyhow::Result;
use clap::Parser;

use zenoh_playground::bench;
use zenoh_playground::protocols::{self, ALL_PROTOCOLS, Protocol};

#[derive(Parser)]
#[command(name = "bench-all", about = "Run benchmarks for all protocols")]
struct Args {
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

/// 啟動 bench-server 子程序
fn start_server(protocol: &Protocol) -> Result<Child> {
    let child = Command::new(server_binary_path())
        .args(["--protocol", &protocol.to_string()])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;
    Ok(child)
}

/// 取得 bench-server 的路徑（與自身在同一目錄）
fn server_binary_path() -> std::path::PathBuf {
    let mut path = std::env::current_exe().expect("Failed to get current exe path");
    path.set_file_name("bench-server");
    path
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    println!("=== Running all protocol benchmarks ===");
    println!(
        "Payload: {} bytes, Iterations: {}, Warmup: {}",
        args.payload_size, args.iterations, args.warmup
    );
    println!();

    let mut results: Vec<bench::BenchResult> = Vec::new();

    for protocol in ALL_PROTOCOLS {
        println!("────────────────────────────────────────");
        println!("Protocol: {}", protocol);
        println!("────────────────────────────────────────");

        // 啟動伺服器子程序
        let mut server = match start_server(protocol) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("  Failed to start server: {}", e);
                continue;
            }
        };

        // 等待伺服器啟動
        tokio::time::sleep(Duration::from_secs(2)).await;

        // 建立客戶端並執行 benchmark（在獨立 scope 內確保資源釋放）
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
            // 明確 drop 客戶端，釋放 Zenoh session 和網路連線
            drop(client);
            r
        }
        .await;

        // 停止伺服器
        let _ = server.kill();
        let _ = server.wait();

        // 等待端口和資源完全釋放
        tokio::time::sleep(Duration::from_secs(2)).await;

        match result {
            Ok(r) => {
                bench::print_report(&r);
                results.push(r);
            }
            Err(e) => {
                eprintln!("  Benchmark failed for {}: {}", protocol, e);
            }
        }
        println!();
    }

    // 輸出比較表格
    bench::print_comparison(&results);

    // 輸出 JSON
    if let Some(path) = &args.json_output {
        let json = serde_json::to_string_pretty(&results)?;
        std::fs::write(path, &json)?;
        println!("\nResults written to {}", path);
    }

    Ok(())
}
