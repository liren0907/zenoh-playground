// HTTP 基準測試傳輸層
// 使用 axum 作為伺服器，reqwest 作為客戶端

use anyhow::Result;
use axum::{Router, routing::post};
use tokio::net::TcpListener;

const HTTP_ADDR: &str = "0.0.0.0:8080";
const HTTP_URL: &str = "http://127.0.0.1:8080/echo";

/// HTTP 基準測試伺服器
pub struct HttpServer;

impl HttpServer {
    pub async fn serve(&self) -> Result<()> {
        let app = Router::new().route("/echo", post(echo_handler));
        let listener = TcpListener::bind(HTTP_ADDR).await?;

        println!("[Server] Listening on http://{}/echo", HTTP_ADDR);
        println!("Press Ctrl+C to stop...");

        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// 回傳收到的 body 原封不動
async fn echo_handler(body: axum::body::Bytes) -> axum::body::Bytes {
    body
}

/// HTTP 基準測試客戶端
pub struct HttpClient {
    client: reqwest::Client,
}

impl HttpClient {
    pub async fn new() -> Result<Self> {
        let client = reqwest::Client::new();

        // 等待伺服器就緒
        println!("[Client] Waiting for server...");
        let mut delay = tokio::time::Duration::from_millis(100);
        for attempt in 1..=20 {
            match client.post(HTTP_URL).body(vec![0u8; 1]).send().await {
                Ok(_) => {
                    println!("[Client] Server is ready (attempt {})", attempt);
                    return Ok(Self { client });
                }
                Err(_) => {
                    if attempt < 20 {
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(tokio::time::Duration::from_secs(5));
                    }
                }
            }
        }

        anyhow::bail!("HTTP server not reachable after 20 attempts")
    }

    pub async fn send(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let resp = self
            .client
            .post(HTTP_URL)
            .body(payload.to_vec())
            .send()
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }
}
