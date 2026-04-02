// gRPC 基準測試傳輸層
// 使用 tonic 作為伺服器和客戶端

use anyhow::Result;
use tonic::{Request, Response, Status};

pub mod pb {
    tonic::include_proto!("bench");
}

use pb::echo_client::EchoClient;
use pb::echo_server::{Echo, EchoServer};
use pb::{EchoRequest, EchoResponse};

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

/// gRPC 基準測試伺服器
pub struct GrpcServer;

impl GrpcServer {
    pub async fn serve(&self) -> Result<()> {
        let addr = GRPC_ADDR.parse()?;

        println!("[Server] Listening on grpc://{}", GRPC_ADDR);
        println!("Press Ctrl+C to stop...");

        tonic::transport::Server::builder()
            .add_service(EchoServer::new(EchoService))
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
        // clone client 因為 tonic client 的 send 需要 &mut self
        let mut client = self.client.clone();
        let response = client.send_bytes(request).await?;
        Ok(response.into_inner().data)
    }
}
