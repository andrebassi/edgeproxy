---
sidebar_position: 3
---

# Architecture

This document provides a deep dive into edgeProxy's internal architecture, data flow, and design decisions.

## System Overview

edgeProxy is designed as a stateless L4 (TCP) proxy that can be deployed at multiple Points of Presence (POPs) worldwide. Each POP instance:

1. Accepts client TCP connections
2. Determines optimal backend using geo + load scoring
3. Maintains client affinity (sticky sessions)
4. Proxies bidirectional traffic transparently

![Architecture Overview](/img/architecture-overview.svg)

## Multi-Region Deployment

edgeProxy is designed for multi-region deployments with WireGuard mesh networking and distributed SQLite replication via Corrosion:

![Multi-Region Architecture](/img/multi-region.svg)

## Core Components

### 1. Configuration (`config.rs`)

Loads all settings from environment variables at startup:

```rust
pub struct Config {
    pub listen_addr: String,           // TCP listen address
    pub db_path: String,               // SQLite database path
    pub region: String,                // Local POP region
    pub db_reload_secs: u64,           // Routing reload interval
    pub geoip_path: Option<String>,    // MaxMind database path
    pub binding_ttl_secs: u64,         // Client affinity TTL
    pub binding_gc_interval_secs: u64, // Cleanup interval
    pub debug: bool,                   // Verbose logging
}
```

### 2. Routing Database (`db.rs`)

SQLite database containing backend definitions:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Unique backend identifier
    app TEXT,                 -- Application name
    region TEXT,              -- Geographic region (sa, us, eu)
    wg_ip TEXT,               -- WireGuard overlay IP
    port INTEGER,             -- Backend port
    healthy INTEGER,          -- Health status (0/1)
    weight INTEGER,           -- Load balancing weight
    soft_limit INTEGER,       -- Preferred max connections
    hard_limit INTEGER,       -- Absolute max connections
    deleted INTEGER           -- Soft delete flag
);
```

The database is reloaded periodically (default: 5 seconds) to pick up changes without restart:

```rust
pub async fn start_routing_sync_sqlite(
    routing: Arc<RwLock<RoutingState>>,
    db_path: String,
    interval_secs: u64,
) -> Result<()> {
    loop {
        // Load backends from SQLite
        let new_state = load_routing_state_from_sqlite(&db_path)?;

        // Atomic update
        let mut guard = routing.write().await;
        *guard = new_state;

        sleep(Duration::from_secs(interval_secs)).await;
    }
}
```

### 3. Load Balancer (`lb.rs`)

The load balancer uses a scoring system to select the optimal backend:

```
score = region_score * 100 + (load_factor / weight)

where:
  region_score = 0 (client region matches backend)
               = 1 (local POP region)
               = 2 (fallback/other regions)

  load_factor = current_connections / soft_limit

  weight = configured backend weight (higher = preferred)
```

**Algorithm:**

1. Filter backends: `healthy = true` AND `connections < hard_limit`
2. Calculate score for each backend
3. Select backend with lowest score

```rust
pub fn pick_backend(
    backends: &[Backend],
    local_region: &str,
    client_region: Option<&str>,
    metrics: &DashMap<String, BackendMetrics>,
) -> Option<Backend> {
    let mut best: Option<(Backend, f64)> = None;

    for b in backends {
        if !b.healthy { continue; }

        let conns = metrics.get(&b.id)
            .map(|m| m.current_conns.load(Ordering::Relaxed))
            .unwrap_or(0);

        if conns >= b.hard_limit as u64 { continue; }

        // Calculate region score
        let region_score = match client_region {
            Some(cr) if cr == b.region => 0,
            _ if b.region == local_region => 1,
            _ => 2,
        };

        // Calculate load factor
        let load_factor = conns as f64 / b.soft_limit as f64;

        // Final score (lower is better)
        let score = (region_score * 100) as f64 + (load_factor / b.weight as f64);

        // Update best if this is better
        if best.is_none() || score < best.as_ref().unwrap().1 {
            best = Some((b.clone(), score));
        }
    }

    best.map(|(b, _)| b)
}
```

### 4. Shared State (`state.rs`)

Global state shared across all connections:

```rust
pub struct RcProxyState {
    pub local_region: String,
    pub routing: Arc<RwLock<RoutingState>>,
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    pub metrics: Arc<DashMap<String, BackendMetrics>>,
    pub geo: Option<GeoDb>,
}
```

**Key structures:**

- `RoutingState`: Current backend list (refreshed periodically)
- `DashMap<ClientKey, Binding>`: Lock-free client affinity map
- `DashMap<String, BackendMetrics>`: Per-backend connection counts and RTT

### 5. GeoIP Resolution (`state.rs`)

Maps client IPs to regions using MaxMind GeoLite2:

```rust
impl GeoDb {
    pub fn region_for_ip(&self, ip: IpAddr) -> Option<String> {
        let country: geoip2::Country = self.reader.lookup(ip).ok()?;
        let iso_code = country.country?.iso_code?;

        // Map country to region
        match iso_code {
            "BR" | "AR" | "CL" | "PE" | "CO" => Some("sa".to_string()),
            "US" | "CA" | "MX" => Some("us".to_string()),
            "PT" | "ES" | "FR" | "DE" | "GB" => Some("eu".to_string()),
            _ => Some("us".to_string()), // Default fallback
        }
    }
}
```

### 6. TCP Proxy (`proxy.rs`)

The core proxy logic handles bidirectional TCP streaming:

```rust
async fn handle_connection(
    state: RcProxyState,
    client_stream: TcpStream,
    client_addr: SocketAddr,
) -> Result<()> {
    // 1. Check for existing binding (affinity)
    let client_key = ClientKey { client_ip: client_addr.ip() };

    // 2. Resolve backend
    let backend = if let Some(binding) = state.bindings.get(&client_key) {
        // Use existing binding
        find_backend_by_id(&binding.backend_id)
    } else {
        // Pick new backend using load balancer
        let client_region = state.geo.as_ref()
            .and_then(|g| g.region_for_ip(client_addr.ip()));
        pick_backend(&backends, &state.local_region, client_region.as_deref())
    };

    // 3. Connect to backend
    let backend_stream = TcpStream::connect(&backend_addr).await?;

    // 4. Update metrics
    state.metrics.entry(backend.id.clone())
        .or_insert_with(BackendMetrics::new)
        .current_conns.fetch_add(1, Ordering::Relaxed);

    // 5. Bidirectional copy with proper half-close handling
    let (client_read, client_write) = client_stream.into_split();
    let (backend_read, backend_write) = backend_stream.into_split();

    let c2b = tokio::spawn(async move {
        io::copy(&mut client_read, &mut backend_write).await?;
        backend_write.shutdown().await
    });

    let b2c = tokio::spawn(async move {
        io::copy(&mut backend_read, &mut client_write).await
    });

    tokio::join!(c2b, b2c);

    // 6. Cleanup metrics
    state.metrics.get(&backend.id)
        .map(|m| m.current_conns.fetch_sub(1, Ordering::Relaxed));

    Ok(())
}
```

## Connection Flow

The request flow shows the complete lifecycle of a TCP connection through edgeProxy:

![Request Flow](/img/request-flow.svg)

## Design Decisions

### Why Rust?

- **Predictable Latency**: No garbage collection pauses
- **Memory Safety**: Zero-cost abstractions without runtime overhead
- **Async I/O**: Tokio provides efficient event-driven networking
- **Performance**: Competitive with C/C++ implementations

### Why DashMap?

- **Lock-Free**: Concurrent reads without blocking
- **Sharded**: Distributed locking for writes
- **Drop-in**: Similar API to `HashMap`

### Why SQLite?

- **Simplicity**: Single file, no server required
- **Replication**: Works with Corrosion for distributed sync
- **Transactions**: ACID guarantees for routing updates

### Why WireGuard?

- **Encryption**: Secure overlay between POPs
- **Performance**: Kernel-level encryption with minimal overhead
- **Simplicity**: Point-to-point configuration

## Performance Considerations

### Connection Handling

- Each connection spawns two Tokio tasks (client→backend, backend→client)
- `io::copy` uses optimized kernel splice when available
- Half-close properly handled with `shutdown()`

### Memory Usage

- Bindings stored in DashMap with TTL expiration
- Periodic garbage collection removes expired entries
- Backend list refreshed atomically without memory spikes

### Scalability

- Horizontal: Deploy multiple edgeProxy instances behind DNS/Anycast
- Vertical: Tokio scales to available CPU cores automatically

## Next Steps

- [Configuration](./configuration) - All available options
- [Load Balancer Internals](./internals/load-balancer) - Detailed scoring algorithm
- [Docker Deployment](./deployment/docker) - Container setup
