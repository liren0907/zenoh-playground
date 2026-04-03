// gRPC 基準測試傳輸層
// 使用 tonic 作為伺服器和客戶端

use anyhow::Result;
use tokio_stream::StreamExt;
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("bench");
}

use pb::echo_client::EchoClient;
use pb::echo_server::{Echo, EchoServer};
use pb::bench_stream_client::BenchStreamClient;
use pb::bench_stream_server::{BenchStream, BenchStreamServer};
use pb::{EchoRequest, EchoResponse, StreamRequest, StreamResponse};

const GRPC_ADDR: &str = "0.0.0.0:50052";
const GRPC_URL: &str = "http://127.0.0.1:50052";

/// gRPC Echo 服務實作
#[derive(Default)]
struct EchoService;

#[tonic::async_trait]
impl Echo for EchoService {
    async fn send_bytes(
        &self,
        request: Request<EchoRequest>,
    ) -> Result<Response<EchoResponse>, Status> {
        let data = request.into_inner().data;
        Ok(Response::new(EchoResponse { data }))
    }
}

/// gRPC 串流服務實作
#[derive(Default)]
struct StreamService;

#[tonic::async_trait]
impl BenchStream for StreamService {
    type StreamDataStream =
        tokio_stream::wrappers::ReceiverStream<Result<StreamResponse, Status>>;

    async fn stream_data(
        &self,
        request: Request<StreamRequest>,
    ) -> Result<Response<Self::StreamDataStream>, Status> {
        let req = request.into_inner();
        let payload_size = req.payload_size as usize;
        let count = req.count as usize;

        // 使用 bounded(1) 增加背壓，確保時間戳更接近實際送出時間
        let (tx, rx) = tokio::sync::mpsc::channel(1);

        tokio::spawn(async move {
            let data: Vec<u8> = (0..payload_size).map(|i| i as u8).collect();

            for _ in 0..count {
                let resp = StreamResponse { data: data.clone() };
                if tx.send(Ok(resp)).await.is_err() {
                    break;
                }
            }
        });

        Ok(Response::new(tokio_stream::wrappers::ReceiverStream::new(rx)))
    }
}

/// gRPC 基準測試伺服器
pub struct GrpcServer;

impl GrpcServer {
    pub async fn serve(&self) -> Result<()> {
        let addr = GRPC_ADDR.parse()?;

        println!("[Server] Listening on grpc://{}", GRPC_ADDR);
        println!("Press Ctrl+C to stop...");

        tonic::transport::Server::builder()
            .add_service(EchoServer::new(EchoService))
            .add_service(BenchStreamServer::new(StreamService))
            .serve(addr)
            .await?;

        Ok(())
    }
}

/// gRPC 基準測試客戶端
pub struct GrpcClient {
    client: EchoClient<tonic::transport::Channel>,
}

impl GrpcClient {
    pub async fn new() -> Result<Self> {
        println!("[Client] Waiting for server...");
        let mut delay = tokio::time::Duration::from_millis(100);

        for attempt in 1..=20 {
            match EchoClient::connect(GRPC_URL).await {
                Ok(client) => {
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

        anyhow::bail!("gRPC server not reachable after 20 attempts")
    }

    pub async fn send(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let request = EchoRequest {
            data: payload.to_vec(),
        };
        let mut client = self.client.clone();
        let response = client.send_bytes(request).await?;
        Ok(response.into_inner().data)
    }
}

// ── Pub/Sub 模式（server streaming） ──

/// gRPC 串流伺服器（同時提供 Echo 和 BenchStream 服務）
pub struct GrpcStreamServer;

impl GrpcStreamServer {
    pub async fn start(&self, _payload_size: usize, _count: usize) -> Result<()> {
        let addr = GRPC_ADDR.parse()?;

        println!("[Server] Listening on grpc://{} (streaming)", GRPC_ADDR);
        println!("Press Ctrl+C to stop...");

        tonic::transport::Server::builder()
            .add_service(EchoServer::new(EchoService))
            .add_service(BenchStreamServer::new(StreamService))
            .serve(addr)
            .await?;

        Ok(())
    }
}

/// gRPC 串流客戶端
pub struct GrpcStreamClient;

impl GrpcStreamClient {
    pub async fn new() -> Result<Self> {
        println!("[Client] Waiting for server...");
        let mut delay = tokio::time::Duration::from_millis(100);

        for attempt in 1..=20 {
            match BenchStreamClient::connect(GRPC_URL).await {
                Ok(_) => {
                    println!("[Client] Server is ready (attempt {})", attempt);
                    return Ok(Self);
                }
                Err(_) => {
                    if attempt < 20 {
                        tokio::time::sleep(delay).await;
                        delay = (delay * 2).min(tokio::time::Duration::from_secs(5));
                    }
                }
            }
        }

        anyhow::bail!("gRPC server not reachable after 20 attempts")
    }

    pub async fn subscribe(&self, count: usize, payload_size: usize, publishers: usize) -> Result<usize> {
        if publishers <= 1 {
            Self::subscribe_single_stream(count, payload_size).await
        } else {
            let base_count = count / publishers;
            let remainder = count % publishers;

            let mut handles = Vec::with_capacity(publishers);
            for i in 0..publishers {
                let stream_count = base_count + if i < remainder { 1 } else { 0 };
                handles.push(tokio::spawn(
                    Self::subscribe_single_stream(stream_count, payload_size),
                ));
            }

            let mut total = 0;
            for handle in handles {
                total += handle.await??;
            }
            Ok(total)
        }
    }

    async fn subscribe_single_stream(count: usize, payload_size: usize) -> Result<usize> {
        let mut client = BenchStreamClient::connect(GRPC_URL).await?;

        let request = StreamRequest {
            payload_size: payload_size as u32,
            count: count as u32,
        };

        let mut stream = client.stream_data(request).await?.into_inner();
        let mut received = 0;

        while let Some(resp) = stream.next().await {
            match resp {
                Ok(_) => {
                    received += 1;
                    if received >= count {
                        break;
                    }
                }
                Err(e) => {
                    eprintln!("[gRPC Client] Stream error: {}", e);
                    break;
                }
            }
        }

        Ok(received)
    }
}
