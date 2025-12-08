---
sidebar_position: 2
---

# Affinity

Client affinity (sticky sessions) ensures that repeated connections from the same client IP are routed to the same backend. This is critical for stateful protocols and session-based applications.

## Overview

edgeProxy maintains a binding table that maps client IPs to backend IDs. The following diagram shows the binding lifecycle:

![Client Affinity](/img/client-affinity.svg)

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Binding lifetime (10 minutes) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Cleanup interval |

## Hexagonal Architecture

Client affinity is managed through **ports and adapters**:

```
domain/entities.rs           → ClientKey, Binding (entities)
domain/ports/binding_repository.rs → BindingRepository trait (port)
adapters/outbound/dashmap_binding_repo.rs → DashMapBindingRepository (adapter)
```

The domain defines WHAT we need (the trait), the adapter provides HOW (DashMap).

## Data Structures

### ClientKey (`domain/entities.rs`)

```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClientKey {
    pub client_ip: IpAddr,
}

impl ClientKey {
    pub fn new(client_ip: IpAddr) -> Self {
        Self { client_ip }
    }
}
```

### Binding (`domain/entities.rs`)

```rust
#[derive(Clone, Debug)]
pub struct Binding {
    pub backend_id: String,
    pub created_at: Instant,
    pub last_seen: Instant,
}

impl Binding {
    pub fn new(backend_id: String) -> Self {
        let now = Instant::now();
        Self { backend_id, created_at: now, last_seen: now }
    }
}
```

### Port (Interface) - `domain/ports/binding_repository.rs`

```rust
#[async_trait]
pub trait BindingRepository: Send + Sync {
    async fn get(&self, key: &ClientKey) -> Option<Binding>;
    async fn set(&self, key: ClientKey, binding: Binding);
    async fn remove(&self, key: &ClientKey);
    async fn touch(&self, key: &ClientKey);  // Update last_seen
    async fn cleanup_expired(&self, ttl: Duration) -> usize;
}
```

### Adapter (Implementation) - `adapters/outbound/dashmap_binding_repo.rs`

```rust
pub struct DashMapBindingRepository {
    bindings: Arc<DashMap<ClientKey, Binding>>,
}

#[async_trait]
impl BindingRepository for DashMapBindingRepository {
    async fn get(&self, key: &ClientKey) -> Option<Binding> {
        self.bindings.get(key).map(|e| e.value().clone())
    }

    async fn set(&self, key: ClientKey, binding: Binding) {
        self.bindings.insert(key, binding);
    }

    async fn touch(&self, key: &ClientKey) {
        if let Some(mut entry) = self.bindings.get_mut(key) {
            entry.last_seen = Instant::now();
        }
    }
    // ...
}
```

## Lifecycle

All lifecycle operations go through the `ProxyService` (application layer), which uses the `BindingRepository` trait.

### 1. New Connection

When a client connects for the first time:

```rust
// application/proxy_service.rs
pub async fn resolve_backend(&self, client_ip: IpAddr) -> Option<Backend> {
    let client_key = ClientKey::new(client_ip);

    // 1. Check for existing binding via repository trait
    if let Some(binding) = self.binding_repo.get(&client_key).await {
        // ... use existing binding
    }

    // 2. No binding - use LoadBalancer (pure domain logic)
    let backend = LoadBalancer::pick_backend(
        &backends,
        &self.local_region,
        client_geo.as_ref(),
        |id| self.metrics.get_connection_count(id),
    )?;

    // 3. Create new binding via repository trait
    self.binding_repo.set(
        client_key,
        Binding::new(backend.id.clone()),
    ).await;

    Some(backend)
}
```

### 2. Subsequent Connections

When the same client reconnects:

```rust
// application/proxy_service.rs
if let Some(binding) = self.binding_repo.get(&client_key).await {
    // Update last_seen via repository
    self.binding_repo.touch(&client_key).await;

    // Verify backend is still healthy
    if let Some(backend) = self.backend_repo.get_by_id(&binding.backend_id).await {
        if backend.healthy {
            return Some(backend);
        }
    }

    // Backend unhealthy - remove stale binding
    self.binding_repo.remove(&client_key).await;
}
```

### 3. Binding Expiration (GC)

The adapter handles garbage collection:

```rust
// adapters/outbound/dashmap_binding_repo.rs
impl DashMapBindingRepository {
    pub fn start_gc(&self, ttl: Duration, interval: Duration) {
        let bindings = self.bindings.clone();
        tokio::spawn(async move {
            loop {
                let now = Instant::now();
                bindings.retain(|_, binding| {
                    now.duration_since(binding.last_seen) <= ttl
                });
                tokio::time::sleep(interval).await;
            }
        });
    }
}
```

### 4. Backend Failure

If the bound backend becomes unhealthy:

```rust
// application/proxy_service.rs
if let Some(binding) = self.binding_repo.get(&client_key).await {
    // Check backend health via repository
    if let Some(backend) = self.backend_repo.get_by_id(&binding.backend_id).await {
        if backend.healthy {
            return Some(backend);
        }
    }

    // Backend unhealthy or gone - remove binding
    self.binding_repo.remove(&client_key).await;
    // Fall through to LoadBalancer...
}
```

## Flow Diagram

![Client Affinity Flow](/img/client-affinity-flow.svg)

## Use Cases

### 1. Stateful Applications

Games, chat servers, or any application maintaining connection state:

```
Client A ──▶ game-server-1 (player state)
Client A ──▶ game-server-1 (same server, state preserved)
```

### 2. Session-Based Protocols

Applications using session cookies or tokens:

```
Client B ──▶ web-server-2 (session created)
Client B ──▶ web-server-2 (session retrieved)
```

### 3. Connection Pooling

Database connections or persistent HTTP connections:

```
Client C ──▶ db-replica-1 (connection 1)
Client C ──▶ db-replica-1 (connection 2, same replica)
```

## Performance

### Memory Usage

Each binding uses approximately:

```
ClientKey: 16 bytes (IPv4) or 40 bytes (IPv6)
Binding: ~80 bytes (String + 2 Instants)
DashMap overhead: ~64 bytes per entry

Total: ~160 bytes per client
```

For 1 million clients: ~160 MB

### Garbage Collection

GC runs every `BINDING_GC_INTERVAL_SECS`:

```rust
// Iterate all bindings
bindings.retain(|_, binding| {
    now.duration_since(binding.last_seen) < ttl
});
```

Time complexity: O(n) where n = total bindings

### Concurrency

DashMap provides lock-free reads and sharded writes:

- Read (binding lookup): No blocking
- Write (binding create/update): Per-shard locking
- GC (retain): Brief per-shard locks

## Tuning

### High-Frequency Connections

For clients making many short connections:

```bash
# Shorter TTL to free memory faster
export EDGEPROXY_BINDING_TTL_SECS=60

# More frequent GC
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=10
```

### Long-Lived Sessions

For persistent connections or infrequent reconnects:

```bash
# Longer TTL to maintain affinity
export EDGEPROXY_BINDING_TTL_SECS=3600  # 1 hour

# Less frequent GC (lower CPU)
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=300
```

### High Client Volume

For millions of unique clients:

```bash
# Aggressive TTL to bound memory
export EDGEPROXY_BINDING_TTL_SECS=300

# Frequent GC
export EDGEPROXY_BINDING_GC_INTERVAL_SECS=30
```

## Limitations

### 1. IP-Based Only

Affinity is based on client IP, not:
- HTTP cookies
- TLS session tickets
- Application tokens

**Implication:** Clients behind NAT share affinity.

### 2. No Cross-POP Sync

Bindings are local to each POP instance:

```
Client → POP-SA → sa-node-1 (binding created)
Client → POP-US → us-node-1 (different binding!)
```

**Solution:** Use DNS geo-routing to ensure clients hit consistent POPs.

### 3. Backend Changes

If a backend is removed from routing.db:

1. Existing bindings remain until TTL
2. Next connection fails backend health check
3. Binding removed, new backend selected

## Monitoring

### Binding Count

```bash
# Check active bindings (requires debug endpoint)
curl http://localhost:8080/debug/bindings/count
```

### GC Activity

With `DEBUG=1`:

```
DEBUG edge_proxy::state: binding GC removed 150 expired entries
```

### Memory Usage

Monitor process RSS to track binding memory:

```bash
ps -o rss= -p $(pgrep edge-proxy)
```

## Future Improvements

1. **Distributed bindings**: Sync across POPs via Redis/Corrosion
2. **Configurable keys**: Support for headers, cookies
3. **Weighted affinity**: Probability-based stickiness
4. **Metrics export**: Prometheus counters for bindings

## Next Steps

- [Load Balancer](./load-balancer) - Backend selection algorithm
- [Architecture](../architecture) - System overview
- [Configuration](../configuration) - All options
