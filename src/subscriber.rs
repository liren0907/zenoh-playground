use anyhow::Result;
use tokio::task::JoinHandle;
use zenoh::Session;

/// 啟動感測器訂閱者
/// 使用萬用字元 "sensor/**" 監聽所有感測器主題
pub async fn spawn(session: &Session) -> Result<JoinHandle<()>> {
    let subscriber = session
        .declare_subscriber("sensor/**")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to declare subscriber: {}", e))?;

    let handle = tokio::spawn(async move {
        // 持續監聽感測器主題的發佈資料
        while let Ok(sample) = subscriber.recv_async().await {
            // 取出 payload 並顯示對應的主題 key
            let payload: String = sample
                .payload()
                .try_to_string()
                .unwrap_or_default()
                .into_owned();
            println!("[Subscriber] '{}' -> {}", sample.key_expr(), payload);
        }
    });

    Ok(handle)
}
