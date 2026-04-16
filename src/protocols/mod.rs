// 通訊協定抽象層
// 定義統一的基準測試傳輸介面，所有協定實作此介面

pub mod grpc;
pub mod http;
pub mod zenoh_common;

use crate::config::BenchConfig;
use anyhow::Result;
use std::sync::Arc;

/// 基準測試模式
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum BenchMode {
    RequestReply,
    PubSub,
}

impl std::fmt::Display for BenchMode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            BenchMode::RequestReply => write!(f, "request-reply"),
            BenchMode::PubSub => write!(f, "pubsub"),
        }
    }
}

/// 支援的通訊協定
#[derive(Debug, Clone, clap::ValueEnum)]
pub enum Protocol {
    ZenohShm,
    ZenohUnix,
    ZenohTcp,
    ZenohTls,
    ZenohQuic,
    ZenohWs,
    Http,
    Grpc,
}

/// 所有協定的列表（供 run-all 模式使用）
pub const ALL_PROTOCOLS: &[Protocol] = &[
    Protocol::ZenohShm,
    Protocol::ZenohUnix,
    Protocol::ZenohTcp,
    Protocol::ZenohTls,
    Protocol::ZenohQuic,
    Protocol::ZenohWs,
    Protocol::Http,
    Protocol::Grpc,
];

impl std::fmt::Display for Protocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Protocol::ZenohShm => write!(f, "zenoh-shm"),
            Protocol::ZenohUnix => write!(f, "zenoh-unix"),
            Protocol::ZenohTcp => write!(f, "zenoh-tcp"),
            Protocol::ZenohTls => write!(f, "zenoh-tls"),
            Protocol::ZenohQuic => write!(f, "zenoh-quic"),
            Protocol::ZenohWs => write!(f, "zenoh-ws"),
            Protocol::Http => write!(f, "http"),
            Protocol::Grpc => write!(f, "grpc"),
        }
    }
}

/// 統一的基準測試傳輸介面（客戶端）
/// 使用 enum dispatch 避免 async trait 的 dyn 不相容問題
pub enum BenchClient {
    Zenoh(zenoh_common::ZenohClient),
    Http(http::HttpClient),
    Grpc(grpc::GrpcClient),
}

impl BenchClient {
    pub async fn send(&self, payload: &[u8]) -> Result<Vec<u8>> {
        match self {
            BenchClient::Zenoh(c) => c.send(payload).await,
            BenchClient::Http(c) => c.send(payload).await,
            BenchClient::Grpc(c) => c.send(payload).await,
        }
    }
}

/// 伺服器端 enum dispatch
pub enum BenchServerImpl {
    Zenoh(zenoh_common::ZenohServer),
    Http(http::HttpServer),
    Grpc(grpc::GrpcServer),
}

impl BenchServerImpl {
    pub async fn serve(&self) -> Result<()> {
        match self {
            BenchServerImpl::Zenoh(s) => s.serve().await,
            BenchServerImpl::Http(s) => s.serve().await,
            BenchServerImpl::Grpc(s) => s.serve().await,
        }
    }
}

/// 根據協定建立伺服器
pub async fn create_server(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<BenchServerImpl> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let server = zenoh_common::ZenohServer::new(protocol, cfg).await?;
            Ok(BenchServerImpl::Zenoh(server))
        }
        Protocol::Http => Ok(BenchServerImpl::Http(http::HttpServer { cfg })),
        Protocol::Grpc => Ok(BenchServerImpl::Grpc(grpc::GrpcServer { cfg })),
    }
}

/// 根據協定建立客戶端
pub async fn create_client(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<BenchClient> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let client = zenoh_common::ZenohClient::new(protocol, cfg).await?;
            Ok(BenchClient::Zenoh(client))
        }
        Protocol::Http => {
            let client = http::HttpClient::new(&cfg).await?;
            Ok(BenchClient::Http(client))
        }
        Protocol::Grpc => {
            let client = grpc::GrpcClient::new(&cfg).await?;
            Ok(BenchClient::Grpc(client))
        }
    }
}

// ── Pub/Sub 模式 ──

/// 發佈者 enum dispatch
pub enum BenchPublisher {
    Zenoh(zenoh_common::ZenohBenchPublisher),
    Http(http::HttpStreamServer),
    Grpc(grpc::GrpcStreamServer),
}

impl BenchPublisher {
    pub async fn start(&self, payload_size: usize, count: usize, publishers: usize) -> Result<()> {
        match self {
            BenchPublisher::Zenoh(p) => p.start(payload_size, count, publishers).await,
            BenchPublisher::Http(p) => p.start(payload_size, count).await,
            BenchPublisher::Grpc(p) => p.start(payload_size, count).await,
        }
    }

    /// 時間限制模式：持續發送直到 duration 結束
    pub async fn start_timed(&self, payload_size: usize, duration_secs: u64, publishers: usize) -> Result<()> {
        match self {
            BenchPublisher::Zenoh(p) => p.start_timed(payload_size, duration_secs, publishers).await,
            BenchPublisher::Http(p) => {
                // HTTP: 使用一個很大的 count 讓串流持續，client 端會在 duration 後斷開
                let large_count = 10_000_000;
                p.start(payload_size, large_count).await
            }
            BenchPublisher::Grpc(p) => {
                let large_count = 10_000_000;
                p.start(payload_size, large_count).await
            }
        }
    }
}

/// 訂閱者 enum dispatch
pub enum BenchSubscriber {
    Zenoh(zenoh_common::ZenohBenchSubscriber),
    Http(http::HttpStreamClient),
    Grpc(grpc::GrpcStreamClient),
}

impl BenchSubscriber {
    pub async fn subscribe(&self, count: usize, payload_size: usize, publishers: usize) -> Result<usize> {
        match self {
            BenchSubscriber::Zenoh(s) => s.subscribe(count).await,
            BenchSubscriber::Http(s) => s.subscribe(count, payload_size, publishers).await,
            BenchSubscriber::Grpc(s) => s.subscribe(count, payload_size, publishers).await,
        }
    }
}

/// 根據協定建立發佈者
pub async fn create_publisher(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<BenchPublisher> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let publisher = zenoh_common::ZenohBenchPublisher::new(protocol, cfg).await?;
            Ok(BenchPublisher::Zenoh(publisher))
        }
        Protocol::Http => Ok(BenchPublisher::Http(http::HttpStreamServer { cfg })),
        Protocol::Grpc => Ok(BenchPublisher::Grpc(grpc::GrpcStreamServer { cfg })),
    }
}

/// 根據協定建立訂閱者
pub async fn create_subscriber(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<BenchSubscriber> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let subscriber = zenoh_common::ZenohBenchSubscriber::new(protocol, cfg).await?;
            Ok(BenchSubscriber::Zenoh(subscriber))
        }
        Protocol::Http => {
            let subscriber = http::HttpStreamClient::new(&cfg).await?;
            Ok(BenchSubscriber::Http(subscriber))
        }
        Protocol::Grpc => {
            let subscriber = grpc::GrpcStreamClient::new(&cfg).await?;
            Ok(BenchSubscriber::Grpc(subscriber))
        }
    }
}
