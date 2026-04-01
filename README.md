# zenoh-playground

A Rust playground for exploring and demonstrating Zenoh communication patterns — queryable services, pub/sub, and request/reply — all running as async tasks in a single process.

## Prerequisites

- Rust (edition 2024)
- No external Zenoh router required — uses default peer-to-peer config

## Quick Start

```bash
cargo run
```

This starts all services and the client in one process. You'll see output from each component as they interact.

## Services

### Echo Service (`service/echo`)

Returns the received message back with an "Echo: " prefix.

### Binary Convert Service (`service/convert`)

Accepts an integer string and returns its binary representation. Returns an error message for invalid input.

### Temperature Publisher (`sensor/temperature`)

Simulates a temperature sensor by publishing incrementing values every 2 seconds.

### Sensor Subscriber (`sensor/**`)

Listens to all topics under `sensor/` using wildcard matching and prints received data.

### Client

After a 5-second startup delay, periodically queries both the echo and convert services every 3 seconds.

## Example Output

```
Zenoh 多服務節點啟動！
[Publisher] 發佈: Temp = 25.0
[Subscriber] 'sensor/temperature' -> Temp = 25.0
[Client] 發送查詢 #1
[Echo 服務] 收到: Hello Zenoh! #1
[Client] Echo 回覆: Echo: Hello Zenoh! #1
[Binary Convert 服務] 收到: 43
[Client] Convert 回覆: 43 的二進位格式為 0b101011
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| zenoh | 1.8.0 | Peer-to-peer data-centric communication |
| tokio | 1.50.0 | Async runtime |
