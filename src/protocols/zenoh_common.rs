// Zenoh 共用傳輸層
// 所有 Zenoh 協定（SHM、Unix、TCP、TLS、QUIC、WS）共用相同的應用程式碼
// 僅透過設定切換傳輸方式 — 這是 Zenoh 的核心優勢

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::Result;
use tokio::time::{sleep, Duration};
use zenoh::qos::CongestionControl;
use zenoh::shm::{BlockOn, GarbageCollect, PosixShmProviderBackend, ShmProviderBuilder};
use zenoh::{Config, Session, Wait};

use super::Protocol;
use crate::config::BenchConfig;

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
fn setup_tls_certs(cfg: &BenchConfig) -> Result<TlsCertPaths> {
    let dir = PathBuf::from(&cfg.zenoh.cert_dir);
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

    println!("[TLS] Generated self-signed certificates in {}", cfg.zenoh.cert_dir);
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
fn server_config(protocol: &Protocol, cfg: &BenchConfig) -> Result<Config> {
    let mut config = Config::default();

    let endpoint = cfg.zenoh_listen(protocol);
    insert_json5(&mut config, "listen/endpoints", &format!(r#"["{}"]"#, endpoint))?;
    insert_json5(&mut config, "scouting/multicast/enabled", "false")?;

    if needs_shm(protocol) {
        let _ = config.transport.shared_memory.set_enabled(true);
    }

    if needs_tls(protocol) {
        let certs = setup_tls_certs(cfg)?;
        apply_tls_server_config(&mut config, &certs)?;
    }

    Ok(config)
}

/// 建立客戶端 Zenoh 設定
fn client_config(protocol: &Protocol, cfg: &BenchConfig) -> Result<Config> {
    let mut config = Config::default();

    let endpoint = cfg.zenoh_connect(protocol);
    insert_json5(&mut config, "connect/endpoints", &format!(r#"["{}"]"#, endpoint))?;
    insert_json5(&mut config, "scouting/multicast/enabled", "false")?;

    if needs_shm(protocol) {
        let _ = config.transport.shared_memory.set_enabled(true);
    }

    if needs_tls(protocol) {
        let certs = setup_tls_certs(cfg)?;
        apply_tls_client_config(&mut config, &certs)?;
    }

    Ok(config)
}

/// Zenoh 基準測試伺服器
/// 宣告 queryable 並回傳收到的原始 bytes
pub struct ZenohServer {
    session: Session,
    cfg: Arc<BenchConfig>,
}

impl ZenohServer {
    pub async fn new(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<Self> {
        // Unix socket 清理殘留的 socket 檔案
        if matches!(protocol, Protocol::ZenohUnix) {
            let _ = std::fs::remove_file(&cfg.zenoh.socket_path);
        }

        let config = server_config(protocol, &cfg)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh server session: {}", e))?;

        Ok(Self { session, cfg })
    }

    pub async fn serve(&self) -> Result<()> {
        let echo_key = self.cfg.zenoh_echo_key();
        let queryable = self
            .session
            .declare_queryable(&echo_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare queryable: {}", e))?;

        println!("[Server] Listening on '{}'", echo_key);
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
    cfg: Arc<BenchConfig>,
}

impl ZenohClient {
    pub async fn new(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<Self> {
        let config = client_config(protocol, &cfg)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh client session: {}", e))?;

        // 等待連線建立，用 retry 確認伺服器就緒
        Self::wait_for_server(&session, &cfg.zenoh_echo_key()).await?;

        Ok(Self { session, cfg })
    }

    /// 以指數退避重試確認伺服器已就緒
    async fn wait_for_server(session: &Session, echo_key: &str) -> Result<()> {
        println!("[Client] Waiting for server...");
        let mut delay = Duration::from_millis(100);
        let max_attempts = 20;

        for attempt in 1..=max_attempts {
            match session
                .get(echo_key)
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
            .get(&self.cfg.zenoh_echo_key())
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

// ── Pub/Sub 模式（純吞吐量測量） ──

/// Zenoh Pub/Sub 發佈者（支援 SHM zero-copy）
pub struct ZenohBenchPublisher {
    session: Session,
    use_shm: bool,
    cfg: Arc<BenchConfig>,
}

impl ZenohBenchPublisher {
    pub async fn new(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<Self> {
        if matches!(protocol, Protocol::ZenohUnix) {
            let _ = std::fs::remove_file(&cfg.zenoh.socket_path);
        }

        let use_shm = needs_shm(protocol);
        let config = server_config(protocol, &cfg)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh publisher session: {}", e))?;

        Ok(Self { session, use_shm, cfg })
    }

    pub async fn start(&self, payload_size: usize, count: usize, publishers: usize) -> Result<()> {
        // 就緒信號：等待訂閱者查詢後才開始發佈
        let ready_key = self.cfg.zenoh_ready_key();
        let stream_key = self.cfg.zenoh_stream_key();
        let ready = self
            .session
            .declare_queryable(&ready_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare ready queryable: {}", e))?;

        println!("[Publisher] Waiting for subscriber...");
        if let Ok(query) = ready.recv_async().await {
            let _ = query.reply(query.key_expr().clone(), "ready").await;
        }
        println!(
            "[Publisher] Subscriber connected, publishing {} messages with {} publisher(s)",
            count, publishers
        );

        let payload: Vec<u8> = (0..payload_size).map(|i| i as u8).collect();

        if publishers <= 1 {
            // 單一 publisher（原始路徑）
            let publisher = self
                .session
                .declare_publisher(&stream_key)
                .congestion_control(CongestionControl::Drop)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to declare publisher: {}", e))?;

            if self.use_shm {
                self.publish_shm(&publisher, payload_size, count, &payload).await
            } else {
                self.publish_regular(&publisher, count, &payload).await
            }
        } else {
            // 多 publisher 併發
            self.publish_concurrent(payload_size, count, publishers, &payload, &stream_key).await
        }
    }

    /// 多 publisher 併發發佈
    async fn publish_concurrent(
        &self,
        payload_size: usize,
        count: usize,
        publishers: usize,
        payload: &[u8],
        stream_key: &str,
    ) -> Result<()> {
        let base_count = count / publishers;
        let remainder = count % publishers;
        let payload = payload.to_vec();
        let use_shm = self.use_shm;

        let mut handles = Vec::with_capacity(publishers);

        for i in 0..publishers {
            let task_count = base_count + if i < remainder { 1 } else { 0 };
            let session = self.session.clone();
            let task_payload = payload.clone();
            let key = stream_key.to_string();

            let handle = tokio::spawn(async move {
                let publisher = session
                    .declare_publisher(key)
                    .congestion_control(CongestionControl::Drop)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to declare publisher {}: {}", i, e))?;

                if use_shm {
                    let pool_size = 64;
                    let backend = PosixShmProviderBackend::builder(payload_size * pool_size)
                        .wait()
                        .map_err(|e| anyhow::anyhow!("Failed to create SHM backend: {}", e))?;
                    let shm_provider = ShmProviderBuilder::backend(backend).wait();
                    let layout = shm_provider
                        .alloc_layout(payload_size)
                        .map_err(|e| anyhow::anyhow!("Failed to create SHM layout: {}", e))?;

                    for batch_start in (0..task_count).step_by(pool_size) {
                        let batch_end = (batch_start + pool_size).min(task_count);
                        let batch_count = batch_end - batch_start;

                        let mut batch = Vec::with_capacity(batch_count);
                        for _ in 0..batch_count {
                            let mut sbuf = layout
                                .alloc()
                                .with_policy::<BlockOn<GarbageCollect>>()
                                .await
                                .map_err(|e| anyhow::anyhow!("SHM alloc failed: {:?}", e))?;
                            sbuf[..payload_size].copy_from_slice(&task_payload);
                            batch.push(sbuf);
                        }

                        for sbuf in batch {
                            if let Err(e) = publisher.put(sbuf).await {
                                eprintln!("[Publisher {}] Publish failed: {}", i, e);
                            }
                        }
                    }
                } else {
                    for _ in 0..task_count {
                        if let Err(e) = publisher.put(task_payload.clone()).await {
                            eprintln!("[Publisher {}] Publish failed: {}", i, e);
                        }
                    }
                }

                Ok::<(), anyhow::Error>(())
            });

            handles.push(handle);
        }

        // 等待所有 publisher 完成
        for (i, handle) in handles.into_iter().enumerate() {
            if let Err(e) = handle.await? {
                eprintln!("[Publisher {}] Task failed: {}", i, e);
            }
        }

        println!("[Publisher] Done publishing {} messages across {} publishers", count, publishers);
        sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    /// 時間限制模式：持續發送直到 duration 結束
    pub async fn start_timed(&self, payload_size: usize, duration_secs: u64, publishers: usize) -> Result<()> {
        let ready_key = self.cfg.zenoh_ready_key();
        let stream_key = self.cfg.zenoh_stream_key();

        // 就緒信號
        let ready = self
            .session
            .declare_queryable(&ready_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare ready queryable: {}", e))?;

        println!("[Publisher] Waiting for subscriber...");
        if let Ok(query) = ready.recv_async().await {
            let _ = query.reply(query.key_expr().clone(), "ready").await;
        }
        println!(
            "[Publisher] Subscriber connected, timed mode: {}s, {} publisher(s)",
            duration_secs, publishers
        );

        let payload: Vec<u8> = (0..payload_size).map(|i| i as u8).collect();
        let stop = Arc::new(AtomicBool::new(false));
        let use_shm = self.use_shm;

        let mut handles = Vec::with_capacity(publishers);

        for i in 0..publishers {
            let session = self.session.clone();
            let task_payload = payload.clone();
            let stop = stop.clone();
            let key = stream_key.clone();

            let handle = tokio::spawn(async move {
                let publisher = session
                    .declare_publisher(key)
                    .congestion_control(CongestionControl::Drop)
                    .await
                    .map_err(|e| anyhow::anyhow!("Failed to declare publisher {}: {}", i, e))?;

                if use_shm {
                    let pool_size = 64;
                    let backend = PosixShmProviderBackend::builder(payload_size * pool_size)
                        .wait()
                        .map_err(|e| anyhow::anyhow!("Failed to create SHM backend: {}", e))?;
                    let shm_provider = ShmProviderBuilder::backend(backend).wait();
                    let layout = shm_provider
                        .alloc_layout(payload_size)
                        .map_err(|e| anyhow::anyhow!("Failed to create SHM layout: {}", e))?;

                    while !stop.load(Ordering::Relaxed) {
                        let mut sbuf = layout
                            .alloc()
                            .with_policy::<BlockOn<GarbageCollect>>()
                            .await
                            .map_err(|e| anyhow::anyhow!("SHM alloc failed: {:?}", e))?;
                        sbuf[..payload_size].copy_from_slice(&task_payload);
                        let _ = publisher.put(sbuf).await;
                    }
                } else {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = publisher.put(task_payload.clone()).await;
                    }
                }

                Ok::<(), anyhow::Error>(())
            });

            handles.push(handle);
        }

        // 等待 duration 後設置停止信號
        sleep(Duration::from_secs(duration_secs)).await;
        stop.store(true, Ordering::Relaxed);

        for (i, handle) in handles.into_iter().enumerate() {
            if let Err(e) = handle.await? {
                eprintln!("[Publisher {}] Task failed: {}", i, e);
            }
        }

        println!("[Publisher] Timed mode completed ({}s)", duration_secs);
        sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    /// 使用 SHM buffer 發佈（真正的 zero-copy）
    /// 使用批次預分配 buffer pool，避免逐條分配觸發 GC 阻塞
    async fn publish_shm(
        &self,
        publisher: &zenoh::pubsub::Publisher<'_>,
        payload_size: usize,
        count: usize,
        payload: &[u8],
    ) -> Result<()> {
        let pool_size = 64;
        let backend = PosixShmProviderBackend::builder(payload_size * pool_size)
            .wait()
            .map_err(|e| anyhow::anyhow!("Failed to create SHM backend: {}", e))?;
        let shm_provider = ShmProviderBuilder::backend(backend).wait();
        let layout = shm_provider
            .alloc_layout(payload_size)
            .map_err(|e| anyhow::anyhow!("Failed to create SHM layout: {}", e))?;

        println!("[Publisher] SHM mode enabled (batch pool size: {})", pool_size);

        // 批次預分配：每次分配一批 buffer，填入 payload，然後批次發送
        // 避免交錯分配+發送導致 GC 阻塞
        for batch_start in (0..count).step_by(pool_size) {
            let batch_end = (batch_start + pool_size).min(count);
            let batch_count = batch_end - batch_start;

            // 預分配整批 buffer
            let mut batch = Vec::with_capacity(batch_count);
            for _ in 0..batch_count {
                let mut sbuf = layout
                    .alloc()
                    .with_policy::<BlockOn<GarbageCollect>>()
                    .await
                    .map_err(|e| anyhow::anyhow!("SHM alloc failed: {:?}", e))?;
                sbuf[..payload_size].copy_from_slice(payload);
                batch.push(sbuf);
            }

            // 批次發送
            for sbuf in batch {
                if let Err(e) = publisher.put(sbuf).await {
                    eprintln!("[Publisher] Publish failed: {}", e);
                }
            }
        }

        println!("[Publisher] Done publishing {} messages", count);
        sleep(Duration::from_secs(1)).await;
        Ok(())
    }

    /// 一般發佈（非 SHM）
    async fn publish_regular(
        &self,
        publisher: &zenoh::pubsub::Publisher<'_>,
        count: usize,
        payload: &[u8],
    ) -> Result<()> {
        for _ in 0..count {
            if let Err(e) = publisher.put(payload.to_vec()).await {
                eprintln!("[Publisher] Publish failed: {}", e);
            }
        }

        println!("[Publisher] Done publishing {} messages", count);
        sleep(Duration::from_secs(1)).await;
        Ok(())
    }
}

/// Zenoh Pub/Sub 訂閱者
pub struct ZenohBenchSubscriber {
    session: Session,
    cfg: Arc<BenchConfig>,
}

impl ZenohBenchSubscriber {
    pub async fn new(protocol: &Protocol, cfg: Arc<BenchConfig>) -> Result<Self> {
        let config = client_config(protocol, &cfg)?;
        let session = zenoh::open(config)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to open Zenoh subscriber session: {}", e))?;

        Ok(Self { session, cfg })
    }

    pub async fn subscribe(&self, count: usize) -> Result<usize> {
        let stream_key = self.cfg.zenoh_stream_key();
        let ready_key = self.cfg.zenoh_ready_key();

        let subscriber = self
            .session
            .declare_subscriber(&stream_key)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to declare subscriber: {}", e))?;

        // 通知發佈者開始
        println!("[Subscriber] Signaling publisher to start...");
        let mut delay = Duration::from_millis(100);
        for attempt in 1..=20 {
            match self.session.get(&ready_key).await {
                Ok(replies) => {
                    if replies.recv_async().await.is_ok() {
                        println!("[Subscriber] Publisher is ready (attempt {})", attempt);
                        break;
                    }
                }
                Err(_) => {}
            }
            if attempt == 20 {
                anyhow::bail!("Publisher not reachable after 20 attempts");
            }
            sleep(delay).await;
            delay = (delay * 2).min(Duration::from_secs(5));
        }

        // 接收訊息，只計數
        let mut received = 0;
        for _ in 0..count {
            subscriber
                .recv_async()
                .await
                .map_err(|e| anyhow::anyhow!("Subscriber recv failed: {}", e))?;
            received += 1;
        }

        Ok(received)
    }
}
