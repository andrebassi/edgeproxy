---
sidebar_position: 2
---

# Client Affinity

Client affinity (sticky sessions) ensures that repeated connections from the same client IP are routed to the same backend. This is critical for stateful protocols and session-based applications.

## Overview

edgeProxy maintains a binding table that maps client IPs to backend IDs. The following diagram shows the binding lifecycle:

![Client Affinity](/img/client-affinity.svg)

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Binding lifetime (10 minutes) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Cleanup interval |

## Data Structures

### ClientKey

```rust
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ClientKey {
    pub client_ip: IpAddr,
}
```

### Binding

```rust
#[derive(Clone, Debug)]
pub struct Binding {
    pub backend_id: String,
    pub created_at: Instant,
    pub last_seen: Instant,
}
```

### Storage

Bindings are stored in a lock-free `DashMap`:

```rust
pub struct RcProxyState {
    // ...
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    // ...
}
```

## Lifecycle

### 1. New Connection

When a client connects for the first time:

```rust
// No existing binding - use load balancer
let backend = pick_backend(&backends, local_region, client_region);

// Create new binding
state.bindings.insert(
    ClientKey { client_ip },
    Binding {
        backend_id: backend.id.clone(),
        created_at: Instant::now(),
        last_seen: Instant::now(),
    },
);
```

### 2. Subsequent Connections

When the same client reconnects:

```rust
// Check for existing binding
if let Some(mut entry) = state.bindings.get_mut(&client_key) {
    // Update last_seen timestamp
    entry.last_seen = Instant::now();

    // Use existing backend
    chosen_backend_id = Some(entry.backend_id.clone());
}
```

### 3. Binding Expiration

Bindings expire after `BINDING_TTL_SECS` of inactivity:

```rust
pub fn start_binding_gc(
    bindings: Arc<DashMap<ClientKey, Binding>>,
    ttl: Duration,
    interval: Duration,
) {
    tokio::spawn(async move {
        loop {
            sleep(interval).await;

            let now = Instant::now();
            bindings.retain(|_, binding| {
                now.duration_since(binding.last_seen) < ttl
            });
        }
    });
}
```

### 4. Backend Failure

If the bound backend becomes unhealthy:

```rust
// Lookup binding's backend
let backend = rt.backends
    .iter()
    .find(|b| b.id == backend_id && b.healthy)
    .cloned();

// If not found or unhealthy
if backend.is_none() {
    // Remove stale binding
    state.bindings.remove(&client_key);

    // Fall back to load balancer
    return pick_backend(&backends, ...);
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
