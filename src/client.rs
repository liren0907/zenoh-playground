use anyhow::Result;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use zenoh::Session;

/// 啟動客戶端定期查詢任務
/// 展示 Zenoh 的請求-回應模式，定期查詢 echo 和 convert 服務
pub async fn spawn(session: &Session) -> Result<JoinHandle<()>> {
    let session = session.clone();

    let handle = tokio::spawn(async move {
        // 等待服務啟動完成，再發送查詢
        sleep(Duration::from_secs(5)).await;

        let mut counter = 0;
        loop {
            counter += 1;
            println!("[Client] Sending query #{}", counter);

            // 查詢 echo 服務
            match session
                .get("service/echo")
                .payload(format!("Hello Zenoh! #{}", counter))
                .await
            {
                Ok(replies) => {
                    while let Ok(reply) = replies.recv_async().await {
                        if let Ok(sample) = reply.result() {
                            println!(
                                "[Client] Echo reply: {}",
                                sample.payload().try_to_string().unwrap_or_default()
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Client] Echo query failed: {}", e);
                }
            }

            // 遞增整數並查詢 convert 服務
            let test_value = 42 + counter;
            match session
                .get("service/convert")
                .payload(test_value.to_string())
                .await
            {
                Ok(replies) => {
                    while let Ok(reply) = replies.recv_async().await {
                        if let Ok(sample) = reply.result() {
                            println!(
                                "[Client] Convert reply: {}",
                                sample.payload().try_to_string().unwrap_or_default()
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[Client] Convert query failed: {}", e);
                }
            }

            // 等待 3 秒後再發送下一批查詢
            sleep(Duration::from_secs(3)).await;
        }
    });

    Ok(handle)
}
