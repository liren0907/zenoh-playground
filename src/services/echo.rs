use anyhow::Result;
use tokio::task::JoinHandle;
use zenoh::Session;

/// 啟動 Echo 查詢服務
/// 接收 "service/echo" 的查詢，回傳收到的訊息
pub async fn spawn(session: &Session) -> Result<JoinHandle<()>> {
    let queryable = session
        .declare_queryable("service/echo")
        .await
        .map_err(|e| anyhow::anyhow!("Failed to declare Echo queryable: {}", e))?;

    let handle = tokio::spawn(async move {
        // 持續監聽傳入的查詢
        while let Ok(query) = queryable.recv_async().await {
            // 從查詢中提取訊息內容
            let msg = query
                .payload()
                .map(|p| p.try_to_string().unwrap_or_default().to_string())
                .unwrap_or_default();
            println!("[Echo Service] Received: {}", msg);

            // 回應查詢，回傳 echo 後的訊息
            if let Err(e) = query
                .reply(query.key_expr().clone(), format!("Echo: {}", msg))
                .await
            {
                eprintln!("[Echo Service] Reply failed: {}", e);
            }
        }
    });

    Ok(handle)
}
