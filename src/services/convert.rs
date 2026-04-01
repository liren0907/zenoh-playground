use anyhow::Result;
use tokio::task::JoinHandle;
use zenoh::Session;

/// 啟動二進位轉換查詢服務
/// 接收 "service/convert" 的查詢，將數字轉換為二進位格式
pub async fn spawn(session: &Session) -> Result<JoinHandle<()>> {
    let queryable = session
        .declare_queryable("service/convert")
        .await
        .map_err(|e| anyhow::anyhow!("無法宣告 Convert 查詢服務: {}", e))?;

    let handle = tokio::spawn(async move {
        // 持續監聽傳入的查詢
        while let Ok(query) = queryable.recv_async().await {
            // 從查詢中提取訊息內容
            let msg = query
                .payload()
                .map(|p| p.try_to_string().unwrap_or_default().to_string())
                .unwrap_or_default();
            println!("[Binary Convert 服務] 收到: {}", msg);

            // 嘗試解析訊息為整數，並轉換為二進位
            let reply = msg
                .parse::<i64>()
                .map(|v| format!("{} 的二進位格式為 0b{:b}", v, v))
                .unwrap_or_else(|_| "錯誤：不是有效的整數".into());

            // 回應查詢，傳回二進位轉換結果
            if let Err(e) = query.reply(query.key_expr().clone(), reply).await {
                eprintln!("[Binary Convert 服務] 回覆失敗: {}", e);
            }
        }
    });

    Ok(handle)
}
