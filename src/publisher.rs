use anyhow::Result;
use rand::rngs::SmallRng;
use rand::Rng;
use rand::SeedableRng;
use tokio::task::JoinHandle;
use tokio::time::{sleep, Duration};
use zenoh::shm::{
    BlockOn, GarbageCollect, PosixShmProviderBackend, ShmProviderBuilder,
};
use zenoh::{Session, Wait};

/// 啟動溫度感測器發佈者（使用共享記憶體零拷貝）
/// 以正弦波模擬溫度變化，並透過 SHM buffer 發佈到 "sensor/temperature"
pub async fn spawn(session: &Session) -> Result<JoinHandle<()>> {
    let publisher = session
        .declare_publisher("sensor/temperature")
        .await
        .map_err(|e| anyhow::anyhow!("無法宣告溫度發佈者: {}", e))?;

    let handle = tokio::spawn(async move {
        // 建立 SHM 提供者（65KB 共享記憶體區段）
        let backend = match PosixShmProviderBackend::builder(65536).wait() {
            Ok(b) => b,
            Err(e) => {
                eprintln!("[Publisher] 無法建立 SHM 後端: {}，將使用一般發佈模式", e);
                run_without_shm(publisher).await;
                return;
            }
        };
        let shm_provider = ShmProviderBuilder::backend(backend).wait();

        let layout = match shm_provider.alloc_layout(256) {
            Ok(l) => l,
            Err(e) => {
                eprintln!("[Publisher] 無法建立 SHM 配置佈局: {}，將使用一般發佈模式", e);
                run_without_shm(publisher).await;
                return;
            }
        };

        println!("[Publisher] 已啟用共享記憶體模式");
        let mut rng = SmallRng::from_os_rng();
        let mut tick: f64 = 0.0;

        loop {
            // 使用正弦波模擬溫度變化，加上隨機擾動
            let value = 25.0 + 5.0 * (tick * 0.1).sin() + rng.random_range(-0.5_f64..0.5_f64);
            let msg = format!("Temp = {:.1}", value);
            println!("[Publisher] 發佈: {}", msg);

            // 配置 SHM 緩衝區並寫入資料
            match layout
                .alloc()
                .with_policy::<BlockOn<GarbageCollect>>()
                .await
            {
                Ok(mut sbuf) => {
                    let bytes = msg.as_bytes();
                    sbuf[..bytes.len()].copy_from_slice(bytes);
                    if let Err(e) = publisher.put(sbuf).await {
                        eprintln!("[Publisher] 發佈失敗: {}", e);
                    }
                }
                Err(e) => {
                    eprintln!("[Publisher] SHM 配置失敗: {:?}，改用一般發佈", e);
                    if let Err(e) = publisher.put(msg).await {
                        eprintln!("[Publisher] 發佈失敗: {}", e);
                    }
                }
            }

            tick += 1.0;
            sleep(Duration::from_secs(2)).await;
        }
    });

    Ok(handle)
}

/// 不使用 SHM 的一般發佈迴圈（作為 fallback）
async fn run_without_shm(publisher: zenoh::pubsub::Publisher<'static>) {
    let mut rng = SmallRng::from_os_rng();
    let mut tick: f64 = 0.0;

    loop {
        let value = 25.0 + 5.0 * (tick * 0.1).sin() + rng.random_range(-0.5_f64..0.5_f64);
        let msg = format!("Temp = {:.1}", value);
        println!("[Publisher] 發佈: {}", msg);

        if let Err(e) = publisher.put(msg).await {
            eprintln!("[Publisher] 發佈失敗: {}", e);
        }

        tick += 1.0;
        sleep(Duration::from_secs(2)).await;
    }
}
