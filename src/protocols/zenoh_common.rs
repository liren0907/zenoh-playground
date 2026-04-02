// Zenoh 共用傳輸層
// 所有 Zenoh 協定（SHM、Unix、TCP、TLS、QUIC、WS）共用相同的應用程式碼
// 僅透過設定切換傳輸方式 — 這是 Zenoh 的核心優勢

use std::path::PathBuf;

use anyhow::Result;
use tokio::time::{sleep, Duration};
use zenoh::{Config, Session};

use super::Protocol;

const BENCH_KEY_EXPR: &str = "bench/echo";
const CERT_DIR: &str = "/tmp/zenoh-bench-certs";

/// 根據協定回傳伺服器端監聽的端點 URL
fn server_endpoint(protocol: &Protocol) -> &'static str {
    match protocol {
        Protocol::ZenohShm => "tcp/0.0.0.0:7447",
        Protocol::ZenohUnix => "unixsock-stream//tmp/zenoh-bench.sock",
        Protocol::ZenohTcp => "tcp/0.0.0.0:7448",
        Protocol::ZenohTls => "tls/0.0.0.0:7449",
        Protocol::ZenohQuic => "quic/0.0.0.0:7450",
        Protocol::ZenohWs => "ws/0.0.0.0:7451",
        _ => unreachable!(),
    }
}

/// 根據協定回傳客戶端連線的端點 URL
fn client_endpoint(protocol: &Protocol) -> &'static str {
    match protocol {
        Protocol::ZenohShm => "tcp/127.0.0.1:7447",
        Protocol::ZenohUnix => "unixsock-stream//tmp/zenoh-bench.sock",
        Protocol::ZenohTcp => "tcp/127.0.0.1:7448",
        Protocol::ZenohTls => "tls/127.0.0.1:7449",
        Protocol::ZenohQuic => "quic/127.0.0.1:7450",
        Protocol::ZenohWs => "ws/127.0.0.1:7451",
        _ => unreachable!(),
    }
}

/// 是否需要啟用 SHM
fn needs_shm(protocol: &Protocol) -> bool {
    matches!(protocol, Protocol::ZenohShm)
}

/// 是否需要 TLS 憑證（TLS 和 QUIC 都需要）
fn needs_tls(protocol: &Protocol) -> bool {
    matches!(protocol, Protocol::ZenohTls | Protocol::ZenohQuic)
}

/// TLS 憑證路徑
struct TlsCertPaths {
    ca_cert: PathBuf,
    server_cert: PathBuf,
    server_key: PathBuf,
}

/// 產生自簽憑證供 TLS/QUIC 基準測試使用
fn setup_tls_certs() -> Result<TlsCertPaths> {
    let dir = PathBuf::from(CERT_DIR);
    std::fs::create_dir_all(&dir)?;

    let paths = TlsCertPaths {
        ca_cert: dir.join("ca.pem"),
        server_cert: dir.join("server.pem"),
        server_key: dir.join("server.key"),
    };

    // 如果憑證已存在就直接使用
    if paths.ca_cert.exists() && paths.server_cert.exists() && paths.server_key.exists() {
        return Ok(paths);
    }

    // 產生 CA 憑證
    let mut ca_params = rcgen::CertificateParams::new(vec!["Zenoh Bench CA".to_string()])?;
    ca_params.is_ca = rcgen::IsCa::Ca(rcgen::BasicConstraints::Unconstrained);
    let ca_key = rcgen::KeyPair::generate()?;
    let ca_cert = ca_params.self_signed(&ca_key)?;

    // 產生伺服器憑證，由 CA 簽發
    let mut server_params = rcgen::CertificateParams::new(vec!["localhost".to_string()])?;
    server_params
        .subject_alt_names
        .push(rcgen::SanType::IpAddress(std::net::IpAddr::V4(
            std::net::Ipv4Addr::LOCALHOST,
        )));
    let server_key = rcgen::KeyPair::generate()?;
    let server_cert = server_params.signed_by(&server_key, &ca_cert, &ca_key)?;

    // 寫入檔案
    std::fs::write(&paths.ca_cert, ca_cert.pem())?;
    std::fs::write(&paths.server_cert, server_cert.pem())?;
    std::fs::write(&paths.server_key, server_key.serialize_pem())?;

    println!("[TLS] Generated self-signed certificates in {}", CERT_DIR);
    Ok(paths)
}

/// 將 TLS 憑證設定寫入 Zenoh config（伺服器端）
fn apply_tls_server_config(config: &mut Config, certs: &TlsCertPaths) -> Result<()> {
    let ca = certs.ca_cert.to_string_lossy();
    let cert = certs.server_cert.to_string_lossy();
    let key = certs.server_key.to_string_lossy();

    insert_json5(config, "transport/link/tls/root_ca_certificate", &format!(r#""{}""#, ca))?;
    insert_json5(config, "transport/link/tls/listen_certificate", &format!(r#""{}""#, cert))?;
    insert_json5(config, "transport/link/tls/listen_private_key", &format!(r#""{}""#, key))?;
    insert_json5(config, "transport/link/tls/enable_mtls", "false")?;
    insert_json5(config, "transport/link/tls/verify_name_on_connect", "false")?;

    Ok(())
}

/// 將 TLS 憑證設定寫入 Zenoh config（客戶端）
fn apply_tls_client_config(config: &mut Config, certs: &TlsCertPaths) -> Result<()> {
    let ca = certs.ca_cert.to_string_lossy();

    insert_json5(config, "transport/link/tls/root_ca_certificate", &format!(r#""{}""#, ca))?;
    insert_json5(config, "transport/link/tls/enable_mtls", "false")?;
    insert_json5(config, "transport/link/tls/verify_name_on_connect", "false")?;

    Ok(())
}

/// 將 insert_json5 的錯誤轉為 anyhow
fn insert_json5(config: &mut Config, key: &str, value: &str) -> Result<()> {
    config
        .insert_json5(key, value)
        .map_err(|e| anyhow::anyhow!("Failed to set config '{}': {}", key, e))
}

/// 建立伺服器端 Zenoh 設定
fn server_config(protocol: &Protocol) -> Result<Config> {
    let mut config = Config::default();

    let endpoint = server_endpoint(protocol);
    insert_json5(&mut config, "listen/endpoints", &format!(r#"["{}"]"#, endpoint))?;
    insert_json5(&mut config, "scouting/multicast/enabled", "false")?;

    if needs_shm(protocol) {
        let _ = config.transport.shared_memory.set_enabled(true);
    }

    if needs_tls(protocol) {
        let certs = setup_tls_certs()?;
        apply_tls_server_config(&mut config, &certs)?;
    }

    Ok(config)
}

/// 建立客戶端 Zenoh 設定
fn client_config(protocol: &Protocol) -> Result<Config> {
    let mut config = Config::default();

    let endpoint = client_endpoint(protocol);
    insert_json5(&mut config, "connect/endpoints", &format!(r#"["{}"]"#, endpoint))?;
    insert_json5(&mut config, "scouting/multicast/enabled", "false")?;

    if needs_shm(protocol) {
        let _ = config.transport.shared_memory.set_enabled(true);
    }

    if needs_tls(protocol) {
        let certs = setup_tls_certs()?;
        apply_tls_client_config(&mut config, &certs)?;
    }

    Ok(config)
}

/// Zenoh 基準測試伺服器
/// 宣告 queryable 並回傳收到的原始 bytes
pub struct ZenohServer {
    session: Session,
}

impl ZenohServer {
    pub async fn new(protocol: &Protocol) -> Result<Self> {
        // Unix socket 清理殘留的 socket 檔案
        if matches!(protocol, Protocol::ZenohUnix) {
            let _ = std::fs::remove_file("/tmp/zenoh-bench.sock");
        }

        let config = server_config(protocol)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh server session: {}", e))?;

        Ok(Self { session })
    }

    pub async fn serve(&self) -> Result<()> {
        let queryable = self
            .session
            .declare_queryable(BENCH_KEY_EXPR)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare queryable: {}", e))?;

        println!("[Server] Listening on '{}'", BENCH_KEY_EXPR);
        println!("Press Ctrl+C to stop...");

        // 持續監聽並回傳原始 bytes
        while let Ok(query) = queryable.recv_async().await {
            let payload = query
                .payload()
                .map(|p| p.to_bytes().to_vec())
                .unwrap_or_default();

            if let Err(e) = query.reply(query.key_expr().clone(), payload).await {
                eprintln!("[Server] Reply failed: {}", e);
            }
        }

        Ok(())
    }
}

/// Zenoh 基準測試客戶端
pub struct ZenohClient {
    session: Session,
}

impl ZenohClient {
    pub async fn new(protocol: &Protocol) -> Result<Self> {
        let config = client_config(protocol)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh client session: {}", e))?;

        // 等待連線建立，用 retry 確認伺服器就緒
        Self::wait_for_server(&session).await?;

        Ok(Self { session })
    }

    /// 以指數退避重試確認伺服器已就緒
    async fn wait_for_server(session: &Session) -> Result<()> {
        println!("[Client] Waiting for server...");
        let mut delay = Duration::from_millis(100);
        let max_attempts = 20;

        for attempt in 1..=max_attempts {
            match session
                .get(BENCH_KEY_EXPR)
                .payload(vec![0u8; 1])
                .await
            {
                Ok(replies) => {
                    if replies.recv_async().await.is_ok() {
                        println!("[Client] Server is ready (attempt {})", attempt);
                        return Ok(());
                    }
                }
                Err(_) => {}
            }

            if attempt < max_attempts {
                sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(5));
            }
        }

        anyhow::bail!("Server not reachable after {} attempts", max_attempts)
    }

    pub async fn send(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let replies = self
            .session
            .get(BENCH_KEY_EXPR)
            .payload(payload.to_vec())
            .await
            .map_err(|e| anyhow::anyhow!("Query failed: {}", e))?;

        let reply = replies
            .recv_async()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to receive reply: {}", e))?;

        let sample = reply
            .result()
            .map_err(|e| anyhow::anyhow!("Reply error: {:?}", e))?;

        Ok(sample.payload().to_bytes().to_vec())
    }
}
