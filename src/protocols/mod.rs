// 通訊協定抽象層
// 定義統一的基準測試傳輸介面，所有協定實作此介面

pub mod grpc;
pub mod http;
pub mod zenoh_common;

use anyhow::Result;

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
pub async fn create_server(protocol: &Protocol) -> Result<BenchServerImpl> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let server = zenoh_common::ZenohServer::new(protocol).await?;
            Ok(BenchServerImpl::Zenoh(server))
        }
        Protocol::Http => Ok(BenchServerImpl::Http(http::HttpServer)),
        Protocol::Grpc => Ok(BenchServerImpl::Grpc(grpc::GrpcServer)),
    }
}

/// 根據協定建立客戶端
pub async fn create_client(protocol: &Protocol) -> Result<BenchClient> {
    match protocol {
        Protocol::ZenohShm
        | Protocol::ZenohUnix
        | Protocol::ZenohTcp
        | Protocol::ZenohTls
        | Protocol::ZenohQuic
        | Protocol::ZenohWs => {
            let client = zenoh_common::ZenohClient::new(protocol).await?;
            Ok(BenchClient::Zenoh(client))
        }
        Protocol::Http => {
            let client = http::HttpClient::new().await?;
            Ok(BenchClient::Http(client))
        }
        Protocol::Grpc => {
            let client = grpc::GrpcClient::new().await?;
            Ok(BenchClient::Grpc(client))
        }
    }
}
