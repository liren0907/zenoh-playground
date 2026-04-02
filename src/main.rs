// Zenoh 多服務節點應用程式
// 展示 Queryable、Pub/Sub、Client 等通訊模式
// 啟用共享記憶體（SHM）以實現同機器零拷貝傳輸

mod client;
mod publisher;
mod services;
mod subscriber;

use anyhow::{Context, Result};
use zenoh::Config;

#[tokio::main]
async fn main() -> Result<()> {
    // 建立 Zenoh 設定，啟用共享記憶體
    let mut config = Config::default();
    let _ = config.transport.shared_memory.set_enabled(true);

    // 初始化 Zenoh session
    let session = zenoh::open(config)
        .await
        .map_err(|e| anyhow::anyhow!(e))
        .context("Failed to initialize Zenoh session")?;
    println!("Zenoh multi-service node started! (shared memory enabled)");

    // 啟動所有服務，取得各任務的 handle
    let echo_handle = services::echo::spawn(&session).await?;
    let convert_handle = services::convert::spawn(&session).await?;
    let pub_handle = publisher::spawn(&session).await?;
    let sub_handle = subscriber::spawn(&session).await?;
    let client_handle = client::spawn(&session).await?;

    // 等待 Ctrl+C 訊號以優雅地關閉
    println!("Press Ctrl+C to shutdown...");
    tokio::signal::ctrl_c()
        .await
        .context("Failed to listen for Ctrl+C signal")?;
    println!("\nShutting down Zenoh session...");

    // 中止所有任務
    echo_handle.abort();
    convert_handle.abort();
    pub_handle.abort();
    sub_handle.abort();
    client_handle.abort();

    // 關閉 Zenoh session
    let _: () = session
        .close()
        .await
        .map_err(|e| anyhow::anyhow!(e))
        .context("Failed to close session")?;
    println!("Zenoh session closed.");

    Ok(())
}
