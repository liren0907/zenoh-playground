// HTTP 基準測試傳輸層
// 使用 axum 作為伺服器，reqwest 作為客戶端

use crate::config::BenchConfig;
use anyhow::Result;
use axum::extract::Query;
use axum::response::IntoResponse;
use axum::{Router, routing::{get, post}};
use std::sync::Arc;
use tokio::net::TcpListener;

/// HTTP 基準測試伺服器
pub struct HttpServer {
    pub cfg: Arc<BenchConfig>,
}

impl HttpServer {
    pub async fn serve(&self) -> Result<()> {
        let app = Router::new().route("/echo", post(echo_handler));
        let addr = self.cfg.http_addr();
        let listener = TcpListener::bind(&addr).await?;

        println!("[Server] Listening on http://{}/echo", addr);
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
    echo_url: String,
}

impl HttpClient {
    pub async fn new(cfg: &BenchConfig) -> Result<Self> {
        let client = reqwest::Client::new();
        let echo_url = cfg.http_echo_url();

        // 等待伺服器就緒
        println!("[Client] Waiting for server...");
        let mut delay = tokio::time::Duration::from_millis(100);
        for attempt in 1..=20 {
            match client.post(&echo_url).body(vec![0u8; 1]).send().await {
                Ok(_) => {
                    println!("[Client] Server is ready (attempt {})", attempt);
                    return Ok(Self { client, echo_url });
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
            .post(&self.echo_url)
            .body(payload.to_vec())
            .send()
            .await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

// ── Pub/Sub 模式（chunked streaming，純吞吐量） ──

#[derive(serde::Deserialize)]
struct StreamParams {
    payload_size: usize,
    count: usize,
}

/// HTTP 串流伺服器（同時提供 echo 和 stream 端點）
pub struct HttpStreamServer {
    pub cfg: Arc<BenchConfig>,
}

impl HttpStreamServer {
    pub async fn start(&self, _payload_size: usize, _count: usize) -> Result<()> {
        let app = Router::new()
            .route("/echo", post(echo_handler))
            .route("/stream", get(stream_handler));
        let addr = self.cfg.http_addr();
        let listener = TcpListener::bind(&addr).await?;

        println!("[Server] Listening on http://{}/stream", addr);
        println!("Press Ctrl+C to stop...");

        axum::serve(listener, app).await?;
        Ok(())
    }
}

/// 串流端點：產生 count 個長度前綴的二進位訊框
async fn stream_handler(Query(params): Query<StreamParams>) -> impl IntoResponse {
    let payload_size = params.payload_size;
    let count = params.count;

    let stream = async_stream::stream! {
        let payload: Vec<u8> = (0..payload_size).map(|i| i as u8).collect();

        for _ in 0..count {
            // 訊框格式：[4 bytes frame_len][payload]
            let frame_len = (payload_size as u32).to_le_bytes();
            let mut frame = Vec::with_capacity(4 + payload_size);
            frame.extend_from_slice(&frame_len);
            frame.extend_from_slice(&payload);

            yield Ok::<_, std::io::Error>(frame);
        }
    };

    axum::body::Body::from_stream(stream)
}

/// HTTP 串流客戶端
pub struct HttpStreamClient {
    stream_url: String,
}

impl HttpStreamClient {
    pub async fn new(cfg: &BenchConfig) -> Result<Self> {
        let client = reqwest::Client::new();
        let echo_url = cfg.http_echo_url();
        let stream_url = cfg.http_stream_url();

        println!("[Client] Waiting for server...");
        let mut delay = tokio::time::Duration::from_millis(100);
        for attempt in 1..=20 {
            match client.post(&echo_url).body(vec![0u8; 1]).send().await {
                Ok(_) => {
                    println!("[Client] Server is ready (attempt {})", attempt);
                    return Ok(Self { stream_url });
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

    pub async fn subscribe(&self, count: usize, payload_size: usize, publishers: usize) -> Result<usize> {
        if publishers <= 1 {
            Self::subscribe_single_stream(&self.stream_url, count, payload_size).await
        } else {
            // 多串流併發：每個 stream 接收 count/publishers 條訊息
            let base_count = count / publishers;
            let remainder = count % publishers;

            let mut handles = Vec::with_capacity(publishers);
            for i in 0..publishers {
                let stream_count = base_count + if i < remainder { 1 } else { 0 };
                let url = self.stream_url.clone();
                handles.push(tokio::spawn(async move {
                    Self::subscribe_single_stream(&url, stream_count, payload_size).await
                }));
            }

            let mut total = 0;
            for handle in handles {
                total += handle.await??;
            }
            Ok(total)
        }
    }

    async fn subscribe_single_stream(stream_url: &str, count: usize, payload_size: usize) -> Result<usize> {
        use futures::StreamExt;

        let url = format!("{}?payload_size={}&count={}", stream_url, payload_size, count);
        let resp = reqwest::get(&url).await?;
        let mut stream = resp.bytes_stream();

        let mut received = 0;
        let mut buffer: Vec<u8> = Vec::new();

        while received < count {
            match stream.next().await {
                Some(chunk_result) => {
                    let chunk = chunk_result?;
                    buffer.extend_from_slice(&chunk);

                    while buffer.len() >= 4 {
                        let frame_len =
                            u32::from_le_bytes(buffer[..4].try_into().unwrap()) as usize;
                        if buffer.len() < 4 + frame_len {
                            break;
                        }
                        buffer.drain(..4 + frame_len);
                        received += 1;
                    }
                }
                None => break,
            }
        }

        Ok(received)
    }
}
