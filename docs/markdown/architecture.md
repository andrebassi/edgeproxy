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

## Hexagonal Architecture (Ports & Adapters)

edgeProxy uses **Hexagonal Architecture** to separate business logic from infrastructure concerns.

### Why Hexagonal?

1. **Testability**: The load balancing algorithm is a pure function - no SQLite, DashMap, or external dependencies. Can be unit tested with mock data.

2. **Flexibility**: Want to switch from SQLite to PostgreSQL? Just create a new adapter implementing `BackendRepository`. The domain doesn't change.

3. **Separation of Concerns**:
   - **Domain**: Pure business rules (scoring, affinity logic)
   - **Application**: Orchestration (coordinates domain + adapters)
   - **Adapters**: Infrastructure details (SQLite, DashMap, MaxMind)

4. **Dependency Inversion**: Domain defines interfaces (ports/traits), adapters implement them. Domain never imports infrastructure code.

### Project Structure

```
src/
├── main.rs                 # Composition root
├── config.rs               # Environment configuration
├── domain/                 # Core business logic (no external deps)
│   ├── entities.rs         # Backend, Binding, ClientKey, GeoInfo
│   ├── value_objects.rs    # RegionCode
│   ├── ports/              # Interfaces (traits)
│   │   ├── backend_repository.rs
│   │   ├── binding_repository.rs
│   │   ├── geo_resolver.rs
│   │   └── metrics_store.rs
│   └── services/
│       └── load_balancer.rs  # Scoring algorithm (pure)
├── application/            # Use cases / orchestration
│   └── proxy_service.rs
└── adapters/               # Infrastructure implementations
    ├── inbound/
    │   └── tcp_server.rs     # TCP listener
    └── outbound/
        ├── sqlite_backend_repo.rs    # BackendRepository impl
        ├── dashmap_binding_repo.rs   # BindingRepository impl
        ├── maxmind_geo_resolver.rs   # GeoResolver impl
        └── dashmap_metrics_store.rs  # MetricsStore impl
```

### Layer Diagram

```
┌─────────────────────────────────────────────────────────────────────┐
│                        INBOUND ADAPTERS                             │
│                      (driving adapters)                             │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  tcp_server.rs - accepts TCP connections, calls ProxyService│   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                         APPLICATION                                  │
│                    (orchestration / use cases)                       │
│  ┌─────────────────────────────────────────────────────────────┐   │
│  │  ProxyService - resolves backend, manages bindings          │   │
│  └─────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
┌─────────────────────────────────────────────────────────────────────┐
│                           DOMAIN                                     │
│                (pure business rules - ZERO external deps)            │
│  ┌──────────────┐  ┌──────────────┐  ┌──────────────────────────┐  │
│  │  entities    │  │ value_objects│  │  services/LoadBalancer   │  │
│  │  - Backend   │  │ - RegionCode │  │  - pick_backend()        │  │
│  │  - Binding   │  │              │  │  - calculate_geo_score() │  │
│  │  - ClientKey │  │              │  │                          │  │
│  └──────────────┘  └──────────────┘  └──────────────────────────┘  │
│                                                                      │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  ports (traits) - interfaces the domain EXPECTS              │  │
│  │  - BackendRepository    - BindingRepository                  │  │
│  │  - GeoResolver          - MetricsStore                       │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
                                  ▲
                                  │ implements
┌─────────────────────────────────────────────────────────────────────┐
│                       OUTBOUND ADAPTERS                              │
│                   (infrastructure / driven adapters)                 │
│  ┌─────────────────┐  ┌─────────────────┐  ┌─────────────────────┐ │
│  │ SqliteBackend   │  │ DashMapBinding  │  │ MaxMindGeoResolver  │ │
│  │ Repository      │  │ Repository      │  │                     │ │
│  └─────────────────┘  └─────────────────┘  └─────────────────────┘ │
│  ┌─────────────────┐                                                │
│  │ DashMapMetrics  │                                                │
│  │ Store           │                                                │
│  └─────────────────┘                                                │
└─────────────────────────────────────────────────────────────────────┘
```

### Ports (Traits)

Ports define what the domain needs, without knowing HOW it will be implemented:

```rust
// domain/ports/backend_repository.rs
#[async_trait]
pub trait BackendRepository: Send + Sync {
    async fn get_all(&self) -> Vec<Backend>;
    async fn get_by_id(&self, id: &str) -> Option<Backend>;
    async fn get_healthy(&self) -> Vec<Backend>;
}

// domain/ports/geo_resolver.rs
pub trait GeoResolver: Send + Sync {
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo>;
}

// domain/ports/metrics_store.rs
pub trait MetricsStore: Send + Sync {
    fn get_connection_count(&self, backend_id: &str) -> usize;
    fn increment_connections(&self, backend_id: &str);
    fn decrement_connections(&self, backend_id: &str);
    fn record_rtt(&self, backend_id: &str, rtt_ms: u64);
}
```

### Adapters (Implementations)

Adapters implement ports with specific technologies:

```rust
// adapters/outbound/sqlite_backend_repo.rs
#[async_trait]
impl BackendRepository for SqliteBackendRepository {
    async fn get_healthy(&self) -> Vec<Backend> {
        self.backends.read().await
            .iter()
            .filter(|b| b.healthy)
            .cloned()
            .collect()
    }
}

// adapters/outbound/maxmind_geo_resolver.rs
impl GeoResolver for MaxMindGeoResolver {
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo> {
        let resp: CountryResp = self.reader.lookup(ip).ok()?;
        let iso = resp.country?.iso_code?;
        let region = RegionCode::from_country(&iso);
        Some(GeoInfo::new(iso, region))
    }
}
```

### Composition Root (main.rs)

The `main.rs` is the only place that knows ALL concrete implementations:

```rust
// main.rs - Composition Root
let backend_repo = Arc::new(SqliteBackendRepository::new());
let binding_repo = Arc::new(DashMapBindingRepository::new());
let geo_resolver = Arc::new(MaxMindGeoResolver::embedded()?);
let metrics = Arc::new(DashMapMetricsStore::new());

let proxy_service = Arc::new(ProxyService::new(
    backend_repo,    // trait BackendRepository
    binding_repo,    // trait BindingRepository
    geo_resolver,    // trait GeoResolver
    metrics,         // trait MetricsStore
    RegionCode::from_str(&cfg.region),
));

let server = TcpServer::new(proxy_service, cfg.listen_addr);
server.run().await
```

### Practical Benefits

| Scenario | Without Hexagonal | With Hexagonal |
|----------|-------------------|----------------|
| Test LoadBalancer | Needs SQLite running | Simple mock of trait |
| Switch SQLite→Postgres | Refactor all code | Create new adapter |
| Add Redis cache | Modify state.rs | Create adapter implementing port |
| Understand domain | Read mixed code | Just look at `domain/` |

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

### 2. Routing Database (`adapters/outbound/sqlite_backend_repo.rs`)

SQLite database containing backend definitions:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Unique backend identifier
    app TEXT,                 -- Application name
    region TEXT,              -- Geographic region (sa, us, eu)
    country TEXT,             -- Country code (BR, US, FR)
    wg_ip TEXT,               -- WireGuard overlay IP
    port INTEGER,             -- Backend port
    healthy INTEGER,          -- Health status (0/1)
    weight INTEGER,           -- Load balancing weight
    soft_limit INTEGER,       -- Preferred max connections
    hard_limit INTEGER,       -- Absolute max connections
    deleted INTEGER           -- Soft delete flag
);
```

The database is reloaded periodically (default: 5 seconds) via a background Tokio task:

```rust
impl SqliteBackendRepository {
    pub fn start_sync(&self, db_path: String, interval_secs: u64) {
        let backends = self.backends.clone();
        tokio::spawn(async move {
            loop {
                let new_backends = Self::load_from_sqlite(&db_path)?;
                *backends.write().await = new_backends;
                sleep(Duration::from_secs(interval_secs)).await;
            }
        });
    }
}
```

### 3. Load Balancer (`domain/services/load_balancer.rs`)

Pure function with NO external dependencies. Receives a closure to get connection counts:

```rust
impl LoadBalancer {
    pub fn pick_backend<F>(
        backends: &[Backend],
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
        get_conn_count: F,  // Injected closure - doesn't know about DashMap
    ) -> Option<Backend>
    where
        F: Fn(&str) -> usize,
    {
        // Pure scoring algorithm
        // geo_score * 100 + (load_factor / weight)
    }
}
```

**Scoring:**

```
score = geo_score * 100 + (load_factor / weight)

where:
  geo_score = 0 (same country as client - best)
            = 1 (same region as client)
            = 2 (same region as local POP)
            = 3 (fallback - cross region)

  load_factor = current_connections / soft_limit
  weight = backend weight (higher = preferred)
```

### 4. Application Service (`application/proxy_service.rs`)

Orchestrates domain logic and coordinates adapters:

```rust
pub struct ProxyService {
    backend_repo: Arc<dyn BackendRepository>,
    binding_repo: Arc<dyn BindingRepository>,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    metrics: Arc<dyn MetricsStore>,
    local_region: RegionCode,
}

impl ProxyService {
    pub async fn resolve_backend(&self, client_ip: IpAddr) -> Option<Backend> {
        // 1. Check existing binding
        // 2. Resolve client geo
        // 3. Call LoadBalancer with injected metrics closure
        // 4. Create new binding
    }
}
```

### 5. TCP Server (`adapters/inbound/tcp_server.rs`)

Inbound adapter that accepts connections and calls the application service:

```rust
impl TcpServer {
    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.listen_addr).await?;

        loop {
            let (stream, addr) = listener.accept().await?;
            let service = self.proxy_service.clone();

            tokio::spawn(async move {
                // Resolve backend via ProxyService
                let backend = service.resolve_backend(addr.ip()).await?;

                // Connect to backend, record metrics
                // Bidirectional TCP copy
            });
        }
    }
}
```

## Connection Flow

The request flow shows the complete lifecycle of a TCP connection through edgeProxy:

![Request Flow](/img/request-flow.svg)

```
1. Client TCP connection arrives at TcpServer (inbound adapter)
2. TcpServer calls ProxyService.resolve_backend()
3. ProxyService checks BindingRepository for existing binding
4. If no binding: ProxyService resolves geo via GeoResolver
5. ProxyService calls LoadBalancer.pick_backend() with metrics closure
6. LoadBalancer returns best backend (pure domain logic)
7. ProxyService creates binding via BindingRepository
8. TcpServer connects to backend, records metrics via MetricsStore
9. Bidirectional TCP copy (L4 passthrough)
10. On disconnect: TcpServer decrements connection count
```

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

### Why Hexagonal Architecture?

- **Testability**: Domain logic can be tested without infrastructure
- **Flexibility**: Easy to swap implementations (SQLite→PostgreSQL)
- **Maintainability**: Clear separation of concerns
- **Onboarding**: New developers can understand domain by reading `domain/` only

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

## Adding New Adapters

To add a new adapter (e.g., PostgreSQL for backends):

1. Create `adapters/outbound/postgres_backend_repo.rs`
2. Implement `BackendRepository` trait
3. Update `main.rs` composition root to use new adapter

```rust
// adapters/outbound/postgres_backend_repo.rs
pub struct PostgresBackendRepository {
    pool: PgPool,
}

#[async_trait]
impl BackendRepository for PostgresBackendRepository {
    async fn get_healthy(&self) -> Vec<Backend> {
        sqlx::query_as!(Backend, "SELECT * FROM backends WHERE healthy = true")
            .fetch_all(&self.pool)
            .await
            .unwrap_or_default()
    }
}

// main.rs - just change the composition
let backend_repo = Arc::new(PostgresBackendRepository::new(pool));
// rest of the code stays the same!
```

## Next Steps

- [Configuration](./configuration) - All available options
- [Load Balancer Internals](./internals/load-balancer) - Detailed scoring algorithm
- [Docker Deployment](./deployment/docker) - Container setup
