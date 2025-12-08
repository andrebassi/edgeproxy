---
sidebar_position: 7
---

# Infrastructure Components

edgeProxy includes production-ready infrastructure components for reliability and observability.

## Graceful Shutdown

Handles SIGTERM and Ctrl+C signals, allowing in-flight connections to complete before shutdown.

```rust
use edgeproxy::infrastructure::{ShutdownController, shutdown_signal};

// Create controller
let shutdown = ShutdownController::new();

// Track active connections with RAII guards
let _guard = shutdown.connection_guard();
// Connection is automatically decremented when guard is dropped

// Wait for shutdown signal
shutdown_signal().await;

// Drain connections with timeout
shutdown.wait_for_drain(Duration::from_secs(30)).await;
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_SHUTDOWN_TIMEOUT_SECS` | `30` | Max time to wait for connections to drain |

### How It Works

![Graceful Shutdown Flow](/img/graceful-shutdown.svg)

1. Signal received (SIGTERM/Ctrl+C)
2. Stop accepting new connections
3. Wait for active connections to complete (up to timeout)
4. Force close remaining connections
5. Exit cleanly

---

## Rate Limiting

Token bucket rate limiting per client IP to prevent abuse.

```rust
use edgeproxy::infrastructure::{RateLimiter, RateLimitConfig};

let limiter = RateLimiter::new(RateLimitConfig {
    max_requests: 100,        // Requests per window
    window: Duration::from_secs(1),
    burst_size: 10,           // Initial burst allowed
});

// Check if request is allowed
if limiter.check(client_ip) {
    // Process request
} else {
    // Return 429 Too Many Requests
}

// Check remaining tokens
let remaining = limiter.remaining(client_ip);
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_RATE_LIMIT_ENABLED` | `false` | Enable rate limiting |
| `EDGEPROXY_RATE_LIMIT_MAX_REQUESTS` | `100` | Max requests per window |
| `EDGEPROXY_RATE_LIMIT_WINDOW_SECS` | `1` | Time window in seconds |
| `EDGEPROXY_RATE_LIMIT_BURST` | `10` | Burst size (token bucket) |

### Algorithm

```
Token Bucket Algorithm:
- Each client starts with `burst_size` tokens
- Tokens refill at `max_requests / window` rate
- Each request consumes 1 token
- Request denied if no tokens available
```

---

## Circuit Breaker

Prevents cascade failures by temporarily blocking requests to failing backends.

```rust
use edgeproxy::infrastructure::{CircuitBreaker, CircuitBreakerConfig};

let breaker = CircuitBreaker::new(CircuitBreakerConfig {
    failure_threshold: 5,     // Failures before opening
    success_threshold: 3,     // Successes to close
    timeout: Duration::from_secs(30),  // Time in open state
});

// Check if request allowed
if breaker.allow() {
    match backend_request().await {
        Ok(_) => breaker.record_success(),
        Err(_) => breaker.record_failure(),
    }
} else {
    // Circuit is open, fail fast
}
```

### States

| State | Description | Behavior |
|-------|-------------|----------|
| **Closed** | Normal operation | All requests pass through |
| **Open** | Backend failing | All requests fail fast |
| **Half-Open** | Testing recovery | Limited requests to test backend |

### State Transitions

```
         failures >= threshold
Closed ─────────────────────────► Open
   ▲                                │
   │ successes >= threshold         │ timeout expires
   │                                ▼
   └──────────────────────────── Half-Open
                                    │
                                    │ failure
                                    ▼
                                  Open
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_CIRCUIT_BREAKER_ENABLED` | `false` | Enable circuit breaker |
| `EDGEPROXY_CIRCUIT_FAILURE_THRESHOLD` | `5` | Failures to open circuit |
| `EDGEPROXY_CIRCUIT_SUCCESS_THRESHOLD` | `3` | Successes to close circuit |
| `EDGEPROXY_CIRCUIT_TIMEOUT_SECS` | `30` | Timeout in open state |

---

## Active Health Checks

Proactively monitors backend health with TCP or HTTP probes.

```rust
use edgeproxy::infrastructure::{HealthChecker, HealthCheckConfig, HealthCheckType};

let checker = HealthChecker::new(
    "backend-1".to_string(),
    "10.50.1.1:8080".to_string(),
    HealthCheckConfig {
        check_type: HealthCheckType::Http {
            path: "/health".to_string(),
            expected_status: 200,
        },
        interval: Duration::from_secs(5),
        timeout: Duration::from_secs(2),
        healthy_threshold: 2,
        unhealthy_threshold: 3,
    },
);

// Start background health checks
checker.start();

// Get current status
let status = checker.status();
println!("Backend healthy: {}", status.is_healthy);
```

### Check Types

| Type | Description | Use Case |
|------|-------------|----------|
| **TCP** | Simple connection check | Basic connectivity |
| **HTTP** | HTTP GET with status check | Application health |

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_HEALTH_CHECK_ENABLED` | `false` | Enable active health checks |
| `EDGEPROXY_HEALTH_CHECK_INTERVAL_SECS` | `5` | Check interval |
| `EDGEPROXY_HEALTH_CHECK_TIMEOUT_SECS` | `2` | Check timeout |
| `EDGEPROXY_HEALTH_CHECK_TYPE` | `tcp` | Check type: `tcp` or `http` |
| `EDGEPROXY_HEALTH_CHECK_PATH` | `/health` | HTTP check path |
| `EDGEPROXY_HEALTH_HEALTHY_THRESHOLD` | `2` | Successes to become healthy |
| `EDGEPROXY_HEALTH_UNHEALTHY_THRESHOLD` | `3` | Failures to become unhealthy |

---

## Connection Pooling

Reuses TCP connections to backends for improved performance.

```rust
use edgeproxy::infrastructure::{ConnectionPool, PoolConfig};

let pool = ConnectionPool::new(PoolConfig {
    max_connections_per_backend: 10,
    idle_timeout: Duration::from_secs(60),
    max_lifetime: Duration::from_secs(300),
    connect_timeout: Duration::from_secs(5),
});

// Acquire a connection (reuses existing or creates new)
let conn = pool.acquire("backend-1", "10.50.1.1:8080").await?;

// Use connection...
// Connection is returned to pool when dropped
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_POOL_ENABLED` | `false` | Enable connection pooling |
| `EDGEPROXY_POOL_MAX_PER_BACKEND` | `10` | Max connections per backend |
| `EDGEPROXY_POOL_IDLE_TIMEOUT_SECS` | `60` | Idle connection timeout |
| `EDGEPROXY_POOL_MAX_LIFETIME_SECS` | `300` | Max connection lifetime |
| `EDGEPROXY_POOL_CONNECT_TIMEOUT_SECS` | `5` | Connection timeout |

### Benefits

- Reduced latency (no TCP handshake)
- Reduced backend load (fewer connections)
- Better resource utilization

---

## Prometheus Metrics

Export metrics in Prometheus format for monitoring and alerting.

```rust
use edgeproxy::adapters::outbound::PrometheusMetricsStore;

let metrics = PrometheusMetricsStore::new();

// Record connection
metrics.record_connection("backend-1");

// Record bytes
metrics.record_bytes_sent("backend-1", 1024);
metrics.record_bytes_received("backend-1", 2048);

// Export Prometheus format
let output = metrics.export_prometheus();
```

### Exposed Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `edgeproxy_connections_total` | Counter | Total connections |
| `edgeproxy_connections_active` | Gauge | Active connections |
| `edgeproxy_bytes_sent_total` | Counter | Total bytes sent |
| `edgeproxy_bytes_received_total` | Counter | Total bytes received |
| `edgeproxy_backend_connections_total` | Counter | Connections per backend |
| `edgeproxy_backend_connections_active` | Gauge | Active connections per backend |
| `edgeproxy_backend_errors_total` | Counter | Errors per backend |
| `edgeproxy_backend_rtt_seconds` | Histogram | RTT per backend |

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_METRICS_ENABLED` | `false` | Enable Prometheus metrics |
| `EDGEPROXY_METRICS_LISTEN_ADDR` | `0.0.0.0:9090` | Metrics endpoint address |
| `EDGEPROXY_METRICS_PATH` | `/metrics` | Metrics endpoint path |

### Prometheus Scrape Config

```yaml
scrape_configs:
  - job_name: 'edgeproxy'
    static_configs:
      - targets: ['edgeproxy:9090']
    metrics_path: '/metrics'
```

---

## Hot Reload Configuration

Watch configuration files for changes and reload without restart.

```rust
use edgeproxy::infrastructure::{ConfigWatcher, ConfigChange};

let watcher = ConfigWatcher::new(Duration::from_secs(5));

// Watch a configuration file
watcher.watch_file("/etc/edgeproxy/config.toml").await?;

// Subscribe to changes
let mut rx = watcher.subscribe();

// React to changes
tokio::spawn(async move {
    while let Ok(change) = rx.recv().await {
        match change {
            ConfigChange::FileModified(path) => {
                println!("Config file changed: {:?}", path);
                // Reload configuration
            }
            ConfigChange::ValueChanged { key, new_value, .. } => {
                println!("Config {} changed to {}", key, new_value);
            }
            ConfigChange::FullReload => {
                println!("Full config reload");
            }
        }
    }
});
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_CONFIG_WATCH_ENABLED` | `false` | Enable config file watching |
| `EDGEPROXY_CONFIG_WATCH_INTERVAL_SECS` | `5` | File check interval |

### Reloadable Settings

The following can be changed without restart:

- Backend weights and limits
- Health check parameters
- Rate limit thresholds
- Circuit breaker settings

---

## PostgreSQL Backend Repository

Production-ready backend storage using PostgreSQL.

```rust
use edgeproxy::adapters::outbound::{PostgresBackendRepository, PostgresConfig};

let repo = PostgresBackendRepository::new(PostgresConfig {
    url: "postgres://user:pass@localhost:5432/edgeproxy".to_string(),
    max_connections: 10,
    min_connections: 2,
    connect_timeout: Duration::from_secs(5),
    query_timeout: Duration::from_secs(10),
    reload_interval: Duration::from_secs(5),
});

// Initialize (creates tables if needed)
repo.initialize().await?;

// Start background sync
repo.start_sync();

// Use as BackendRepository trait
let backends = repo.get_healthy().await;
```

### Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_POSTGRES_ENABLED` | `false` | Use PostgreSQL for backends |
| `EDGEPROXY_POSTGRES_URL` | *(required)* | PostgreSQL connection URL |
| `EDGEPROXY_POSTGRES_MAX_CONNECTIONS` | `10` | Max pool connections |
| `EDGEPROXY_POSTGRES_MIN_CONNECTIONS` | `2` | Min pool connections |
| `EDGEPROXY_POSTGRES_CONNECT_TIMEOUT_SECS` | `5` | Connection timeout |
| `EDGEPROXY_POSTGRES_QUERY_TIMEOUT_SECS` | `10` | Query timeout |
| `EDGEPROXY_POSTGRES_RELOAD_SECS` | `5` | Cache reload interval |

### Schema

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,
    app TEXT NOT NULL,
    region TEXT NOT NULL,
    country TEXT NOT NULL,
    wg_ip TEXT NOT NULL,
    port INTEGER NOT NULL,
    healthy INTEGER NOT NULL DEFAULT 1,
    weight INTEGER NOT NULL DEFAULT 1,
    soft_limit INTEGER NOT NULL DEFAULT 100,
    hard_limit INTEGER NOT NULL DEFAULT 150,
    deleted INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX idx_backends_healthy ON backends(healthy) WHERE deleted = 0;
CREATE INDEX idx_backends_region ON backends(region) WHERE deleted = 0;
```

---

## Production Configuration Example

Complete example with all infrastructure components enabled:

```bash
# Core
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"

# PostgreSQL Backend
export EDGEPROXY_POSTGRES_ENABLED=true
export EDGEPROXY_POSTGRES_URL="postgres://edgeproxy:secret@postgres:5432/edgeproxy"

# TLS
export EDGEPROXY_TLS_ENABLED=true
export EDGEPROXY_TLS_LISTEN_ADDR="0.0.0.0:8443"
export EDGEPROXY_TLS_CERT="/etc/ssl/edgeproxy.crt"
export EDGEPROXY_TLS_KEY="/etc/ssl/edgeproxy.key"

# API
export EDGEPROXY_API_ENABLED=true
export EDGEPROXY_API_LISTEN_ADDR="0.0.0.0:8081"

# Rate Limiting
export EDGEPROXY_RATE_LIMIT_ENABLED=true
export EDGEPROXY_RATE_LIMIT_MAX_REQUESTS=1000
export EDGEPROXY_RATE_LIMIT_BURST=50

# Circuit Breaker
export EDGEPROXY_CIRCUIT_BREAKER_ENABLED=true
export EDGEPROXY_CIRCUIT_FAILURE_THRESHOLD=5
export EDGEPROXY_CIRCUIT_TIMEOUT_SECS=30

# Health Checks
export EDGEPROXY_HEALTH_CHECK_ENABLED=true
export EDGEPROXY_HEALTH_CHECK_TYPE=http
export EDGEPROXY_HEALTH_CHECK_PATH=/health

# Connection Pooling
export EDGEPROXY_POOL_ENABLED=true
export EDGEPROXY_POOL_MAX_PER_BACKEND=20

# Prometheus Metrics
export EDGEPROXY_METRICS_ENABLED=true
export EDGEPROXY_METRICS_LISTEN_ADDR="0.0.0.0:9090"

# Graceful Shutdown
export EDGEPROXY_SHUTDOWN_TIMEOUT_SECS=60

./edge-proxy
```
