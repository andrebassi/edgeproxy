---
sidebar_position: 3
---

# Phase 3: Active Health Checks

**Goal**: Proactive health monitoring instead of reactive failure detection.

## Current State (Passive)

```rust
// Only detect failure when connection fails
match TcpStream::connect(backend).await {
    Ok(stream) => use_backend(stream),
    Err(_) => mark_unhealthy(backend), // Too late!
}
```

## Target State (Active + Passive)

```rust
// Background health checker
async fn health_checker(backends: Vec<Backend>) {
    loop {
        for backend in &backends {
            let health = check_health(backend).await;
            update_health_status(backend, health);
        }
        sleep(Duration::from_secs(5)).await;
    }
}

async fn check_health(backend: &Backend) -> HealthStatus {
    // TCP check
    let tcp_ok = tcp_connect(backend, timeout).await.is_ok();

    // HTTP check (if applicable)
    let http_ok = http_get(backend, "/health").await
        .map(|r| r.status().is_success())
        .unwrap_or(false);

    // RTT measurement
    let rtt = measure_rtt(backend).await;

    HealthStatus { tcp_ok, http_ok, rtt }
}
```

## Health Check Types

| Type | Protocol | Check | Frequency |
|------|----------|-------|-----------|
| **TCP** | L4 | Port open | 5s |
| **HTTP** | L7 | GET /health returns 2xx | 10s |
| **gRPC** | L7 | grpc.health.v1.Health | 10s |
| **Custom** | Any | User-defined script | Configurable |

## Benefits

- **Proactive detection**: Know before users complain
- **Gradual degradation**: Soft limit before hard failure
- **RTT-based routing**: Route to fastest backend
- **Alerting integration**: Notify on health changes

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 2: Anycast BGP](./phase-2-anycast-bgp)
