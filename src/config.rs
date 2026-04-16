// 基準測試框架的階層式設定系統
// 優先順序：內建預設值 < bench.yaml < CLI 覆寫

use crate::protocols::Protocol;
use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::Path;

/// 完整設定結構
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct BenchConfig {
    pub http: HttpConfig,
    pub grpc: GrpcConfig,
    pub zenoh: ZenohConfig,
    pub defaults: DefaultsConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct HttpConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct GrpcConfig {
    pub port: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ZenohConfig {
    pub key_prefix: String,
    pub socket_path: String,
    pub cert_dir: String,
    pub ports: ZenohPorts,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct ZenohPorts {
    pub shm: u16,
    pub tcp: u16,
    pub tls: u16,
    pub quic: u16,
    pub ws: u16,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct DefaultsConfig {
    pub iterations: usize,
    pub warmup: usize,
    pub window_secs: u64,
    pub payload_sizes: Vec<usize>,
}

// ── Default impls (built-in defaults layer) ──

impl Default for BenchConfig {
    fn default() -> Self {
        Self {
            http: HttpConfig::default(),
            grpc: GrpcConfig::default(),
            zenoh: ZenohConfig::default(),
            defaults: DefaultsConfig::default(),
        }
    }
}

impl Default for HttpConfig {
    fn default() -> Self {
        Self { port: 8080 }
    }
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self { port: 50052 }
    }
}

impl Default for ZenohConfig {
    fn default() -> Self {
        Self {
            key_prefix: "bench".to_string(),
            socket_path: "/tmp/zenoh-bench.sock".to_string(),
            cert_dir: "/tmp/zenoh-bench-certs".to_string(),
            ports: ZenohPorts::default(),
        }
    }
}

impl Default for ZenohPorts {
    fn default() -> Self {
        Self {
            shm: 7447,
            tcp: 7448,
            tls: 7449,
            quic: 7450,
            ws: 7451,
        }
    }
}

impl Default for DefaultsConfig {
    fn default() -> Self {
        Self {
            iterations: 1000,
            warmup: 50,
            window_secs: 5,
            payload_sizes: vec![64, 1024, 4096, 65536, 1048576],
        }
    }
}

// ── CLI 覆寫（最高優先權）──

#[derive(Debug, Clone, Default)]
pub struct CliOverrides {
    pub port_offset: Option<u16>,
    pub http_port: Option<u16>,
    pub grpc_port: Option<u16>,
    pub key_prefix: Option<String>,
}

impl BenchConfig {
    /// 載入設定：指定路徑時讀取檔案；未指定時若 ./bench.yaml 存在則讀取，否則用預設值
    pub fn load(path: Option<&Path>) -> Result<Self> {
        let resolved_path = match path {
            Some(p) => Some(p.to_path_buf()),
            None => {
                let default = Path::new("bench.yaml");
                if default.exists() {
                    Some(default.to_path_buf())
                } else {
                    None
                }
            }
        };

        match resolved_path {
            Some(p) => {
                let content = std::fs::read_to_string(&p)
                    .with_context(|| format!("Failed to read config file: {}", p.display()))?;
                let cfg: Self = serde_yml::from_str(&content)
                    .with_context(|| format!("Failed to parse YAML: {}", p.display()))?;
                Ok(cfg)
            }
            None => Ok(Self::default()),
        }
    }

    /// 套用 CLI 覆寫（依序：port_offset → 各協定 port → key_prefix）
    pub fn apply_cli(&mut self, ov: CliOverrides) {
        if let Some(offset) = ov.port_offset {
            self.http.port = self.http.port.saturating_add(offset);
            self.grpc.port = self.grpc.port.saturating_add(offset);
            self.zenoh.ports.shm = self.zenoh.ports.shm.saturating_add(offset);
            self.zenoh.ports.tcp = self.zenoh.ports.tcp.saturating_add(offset);
            self.zenoh.ports.tls = self.zenoh.ports.tls.saturating_add(offset);
            self.zenoh.ports.quic = self.zenoh.ports.quic.saturating_add(offset);
            self.zenoh.ports.ws = self.zenoh.ports.ws.saturating_add(offset);
        }
        if let Some(p) = ov.http_port {
            self.http.port = p;
        }
        if let Some(p) = ov.grpc_port {
            self.grpc.port = p;
        }
        if let Some(pref) = ov.key_prefix {
            self.zenoh.key_prefix = pref;
        }
    }

    // ── HTTP accessors ──

    pub fn http_addr(&self) -> String {
        format!("0.0.0.0:{}", self.http.port)
    }
    pub fn http_echo_url(&self) -> String {
        format!("http://127.0.0.1:{}/echo", self.http.port)
    }
    pub fn http_stream_url(&self) -> String {
        format!("http://127.0.0.1:{}/stream", self.http.port)
    }

    // ── gRPC accessors ──

    pub fn grpc_addr(&self) -> String {
        format!("0.0.0.0:{}", self.grpc.port)
    }
    pub fn grpc_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.grpc.port)
    }

    // ── Zenoh accessors ──

    /// Zenoh 伺服器端 endpoint（listen 用）
    pub fn zenoh_listen(&self, p: &Protocol) -> String {
        match p {
            Protocol::ZenohShm => format!("tcp/0.0.0.0:{}", self.zenoh.ports.shm),
            Protocol::ZenohUnix => format!("unixsock-stream/{}", self.zenoh.socket_path),
            Protocol::ZenohTcp => format!("tcp/0.0.0.0:{}", self.zenoh.ports.tcp),
            Protocol::ZenohTls => format!("tls/0.0.0.0:{}", self.zenoh.ports.tls),
            Protocol::ZenohQuic => format!("quic/0.0.0.0:{}", self.zenoh.ports.quic),
            Protocol::ZenohWs => format!("ws/0.0.0.0:{}", self.zenoh.ports.ws),
            Protocol::Http | Protocol::Grpc => String::new(),
        }
    }

    /// Zenoh 客戶端 endpoint（connect 用）
    pub fn zenoh_connect(&self, p: &Protocol) -> String {
        match p {
            Protocol::ZenohShm => format!("tcp/127.0.0.1:{}", self.zenoh.ports.shm),
            Protocol::ZenohUnix => format!("unixsock-stream/{}", self.zenoh.socket_path),
            Protocol::ZenohTcp => format!("tcp/127.0.0.1:{}", self.zenoh.ports.tcp),
            Protocol::ZenohTls => format!("tls/127.0.0.1:{}", self.zenoh.ports.tls),
            Protocol::ZenohQuic => format!("quic/127.0.0.1:{}", self.zenoh.ports.quic),
            Protocol::ZenohWs => format!("ws/127.0.0.1:{}", self.zenoh.ports.ws),
            Protocol::Http | Protocol::Grpc => String::new(),
        }
    }

    pub fn zenoh_echo_key(&self) -> String {
        format!("{}/echo", self.zenoh.key_prefix)
    }
    pub fn zenoh_stream_key(&self) -> String {
        format!("{}/stream", self.zenoh.key_prefix)
    }
    pub fn zenoh_ready_key(&self) -> String {
        format!("{}/stream/ready", self.zenoh.key_prefix)
    }
}
