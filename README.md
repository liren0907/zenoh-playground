# Zenoh Playground

A Rust playground for exploring and demonstrating Zenoh communication patterns — queryable services, pub/sub, and request/reply — all running as async tasks in a single process with shared memory (SHM) zero-copy transport.

## Prerequisites

- Rust (edition 2024)
- No external Zenoh router required — uses default peer-to-peer config

## Quick Start

```bash
cargo run
```

This starts all services and the client in one process. Press `Ctrl+C` to gracefully shut down.

## Architecture

```
src/
  main.rs              — startup, config (SHM enabled), graceful shutdown
  services/
    mod.rs             — re-exports service modules
    echo.rs            — Echo queryable service
    convert.rs         — Binary convert queryable service
  publisher.rs         — Temperature publisher (SHM zero-copy)
  subscriber.rs        — Wildcard sensor subscriber
  client.rs            — Periodic query client
```

All services share a single `zenoh::Session` and run as spawned tokio tasks. The publisher uses Zenoh's shared memory (SHM) transport for zero-copy data transfer on the same machine, with automatic fallback to regular transport if SHM is unavailable.

## Zenoh Patterns

### Queryable (Request/Reply)

- **Echo Service** (`service/echo`) — Returns the received message with an "Echo: " prefix.
- **Binary Convert Service** (`service/convert`) — Accepts an integer string and returns its binary representation.

### Pub/Sub

- **Temperature Publisher** (`sensor/temperature`) — Simulates a temperature sensor using a sine wave with random perturbation, publishing every 2 seconds via SHM buffer.
- **Sensor Subscriber** (`sensor/**`) — Listens to all topics under `sensor/` using wildcard matching.

### Client

After a 5-second startup delay, periodically queries both the echo and convert services every 3 seconds.

## Example Output

```
Zenoh 多服務節點啟動！（已啟用共享記憶體）
[Publisher] 已啟用共享記憶體模式
按下 Ctrl+C 以關閉...
[Publisher] 發佈: Temp = 24.8
[Subscriber] 'sensor/temperature' -> Temp = 24.8
[Client] 發送查詢 #1
[Echo 服務] 收到: Hello Zenoh! #1
[Client] Echo 回覆: Echo: Hello Zenoh! #1
[Binary Convert 服務] 收到: 43
[Client] Convert 回覆: 43 的二進位格式為 0b101011
^C
正在關閉 Zenoh session...
Zenoh session 已關閉。
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| zenoh | 1.8.0 | Peer-to-peer data-centric communication (with `shared-memory` and `unstable` features) |
| tokio | 1.50.0 | Async runtime |
| anyhow | 1 | Application-level error handling |
| rand | 0.9 | Temperature simulation randomness |
