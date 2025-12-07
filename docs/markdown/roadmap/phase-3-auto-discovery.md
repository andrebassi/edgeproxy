---
sidebar_position: 3
---

# Phase 3: Auto-Discovery & Registration

**Goal**: Backends automatically register/deregister with the control plane.

## Current State

```sql
-- Manual SQL insert
INSERT INTO backends (id, app, region, wg_ip, port, healthy)
VALUES ('nrt-1', 'echo', 'ap', '10.50.4.1', 8080, 1);
```

## Target State

```rust
// Backend auto-registers on startup
async fn register_backend(control_plane: &ControlPlane) {
    control_plane.register(Backend {
        id: generate_id(),
        app: env::var("APP_NAME"),
        region: detect_region(),
        wg_ip: get_wireguard_ip(),
        port: env::var("PORT"),
        metadata: collect_metadata(),
    }).await;
}

// Heartbeat keeps registration alive
loop {
    control_plane.heartbeat().await;
    sleep(Duration::from_secs(10)).await;
}
```

## Registration Flow

![Registration Flow](/img/roadmap/phase-3-auto-discovery.svg)

## Benefits

- **Zero configuration**: Backends just start
- **Automatic scaling**: New instances appear automatically
- **Graceful shutdown**: Clean deregistration
- **Health integration**: Unhealthy = deregistered

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 2: Distributed Control Plane](./phase-2-corrosion)
- [Phase 4: IPv6 Private Network](./phase-4-ipv6)
