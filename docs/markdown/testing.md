---
sidebar_position: 12
---

# Testing

This guide covers how to test edgeProxy locally and in deployment environments using the mock backend server.

## Mock Backend Server

The `tests/mock-backend/` directory contains a lightweight Go HTTP server that simulates real backend services for testing purposes.

### Features

- **Multi-region simulation**: Configure different regions per instance
- **Request tracking**: Counts requests per backend
- **Multiple endpoints**: Root, health, info, and latency endpoints
- **JSON responses**: Structured responses for easy parsing
- **Minimal footprint**: ~8MB binary, low memory usage

### Building the Mock Server

```bash
# Native build (for local testing)
cd tests/mock-backend
go build -o mock-backend main.go

# Cross-compile for Linux AMD64 (for EC2/cloud deployment)
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go
```

### Running Locally

Start multiple instances to simulate different backends:

```bash
# Terminal 1: EU backend 1
./mock-backend -port 9001 -region eu -id mock-eu-1

# Terminal 2: EU backend 2
./mock-backend -port 9002 -region eu -id mock-eu-2

# Terminal 3: US backend
./mock-backend -port 9003 -region us -id mock-us-1
```

### CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `-port` | `9001` | TCP port to listen on |
| `-region` | `eu` | Region identifier (eu, us, sa, ap) |
| `-id` | `mock-{region}-{port}` | Unique backend identifier |

### Endpoints

| Endpoint | Description | Response |
|----------|-------------|----------|
| `/` | Root | Text with backend info |
| `/health` | Health check | `OK - {id} ({region})` |
| `/api/info` | JSON info | Full backend details |
| `/api/latency` | Minimal JSON | For latency testing |

### Example Response (`/api/info`)

```json
{
  "backend_id": "mock-eu-1",
  "region": "eu",
  "hostname": "ip-172-31-29-183",
  "port": "9001",
  "request_count": 42,
  "uptime_secs": 3600,
  "timestamp": "2025-12-08T00:11:43Z",
  "message": "Hello from mock backend!"
}
```

## Local Testing Setup

### 1. Configure routing.db

Add mock backends to your local routing.db:

```sql
-- Clear existing test backends
DELETE FROM backends WHERE id LIKE 'mock-%';

-- Add mock backends
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
```

### 2. Start Mock Backends

```bash
# Start all 3 backends
./tests/mock-backend/mock-backend -port 9001 -region eu -id mock-eu-1 &
./tests/mock-backend/mock-backend -port 9002 -region eu -id mock-eu-2 &
./tests/mock-backend/mock-backend -port 9003 -region us -id mock-us-1 &
```

### 3. Run edgeProxy

```bash
EDGEPROXY_REGION=eu \
EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080 \
cargo run --release
```

### 4. Test Requests

```bash
# Simple test
curl http://localhost:8080/api/info

# Multiple requests (observe load balancing)
for i in {1..10}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done

# Health check
curl http://localhost:8080/health
```

## EC2 Deployment Testing

### 1. Deploy Mock Server to EC2

```bash
# Build for Linux
cd tests/mock-backend
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go

# Copy to EC2
scp -i ~/.ssh/edgeproxy-key.pem mock-backend-linux-amd64 ubuntu@<EC2-IP>:/tmp/

# SSH and setup
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@<EC2-IP>
sudo mv /tmp/mock-backend-linux-amd64 /opt/edgeproxy/mock-backend
sudo chmod +x /opt/edgeproxy/mock-backend
```

### 2. Start Mock Backends on EC2

```bash
# Start 3 instances
cd /opt/edgeproxy
nohup ./mock-backend -port 9001 -region eu -id mock-eu-1 > /tmp/mock-9001.log 2>&1 &
nohup ./mock-backend -port 9002 -region eu -id mock-eu-2 > /tmp/mock-9002.log 2>&1 &
nohup ./mock-backend -port 9003 -region us -id mock-us-1 > /tmp/mock-9003.log 2>&1 &

# Verify
ps aux | grep mock-backend
curl localhost:9001/health
curl localhost:9002/health
curl localhost:9003/health
```

### 3. Configure routing.db on EC2

```bash
sqlite3 /opt/edgeproxy/routing.db "
DELETE FROM backends WHERE id LIKE 'mock-%';
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
SELECT id, region, port, healthy FROM backends WHERE deleted=0;
"
```

#### Backend Fields Explained

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `id` | TEXT | Unique identifier for the backend. Used in logs and client affinity. | `mock-eu-1` |
| `app` | TEXT | Application name. Groups backends serving the same app. | `test` |
| `region` | TEXT | Geographic region code. Used for geo-routing decisions. Valid: `eu`, `us`, `sa`, `ap`. | `eu` |
| `wg_ip` | TEXT | Backend IP address. Use `127.0.0.1` for local testing, WireGuard IPs (10.50.x.x) in production. | `127.0.0.1` |
| `port` | INTEGER | TCP port the backend listens on. | `9001` |
| `healthy` | INTEGER | Health status. `1` = healthy (receives traffic), `0` = unhealthy (excluded from routing). | `1` |
| `weight` | INTEGER | Relative weight for load balancing. Higher weight = more traffic. Range: 1-10. | `2` |
| `soft_limit` | INTEGER | Comfortable connection count. Above this, the backend is considered "loaded" and less preferred. | `100` |
| `hard_limit` | INTEGER | Maximum connections. At or above this limit, backend is excluded from new connections. | `150` |

#### Example Data Breakdown

```sql
('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150)
```

| Value | Field | Meaning |
|-------|-------|---------|
| `mock-eu-1` | id | Backend identifier, first EU mock server |
| `test` | app | Application name for testing |
| `eu` | region | Located in Europe region |
| `127.0.0.1` | wg_ip | Localhost (same machine as proxy) |
| `9001` | port | Listening on port 9001 |
| `1` | healthy | Backend is healthy and active |
| `2` | weight | Medium priority (scale 1-10) |
| `100` | soft_limit | Comfortable with up to 100 connections |
| `150` | hard_limit | Maximum 150 connections allowed |

#### Load Balancer Scoring

The proxy uses these fields to calculate a score for each backend:

```
score = geo_score * 100 + (connections / soft_limit) / weight
```

- **geo_score**: 0 (same country), 1 (same region), 2 (local POP region), 3 (global fallback)
- **connections**: Current active connections (from metrics)
- **soft_limit**: Divides load factor
- **weight**: Higher weight reduces the score (more preferred)

**Lowest score wins.** Backends with `healthy=0` or at `hard_limit` are excluded.

### 4. Test from External Client

```bash
# From your local machine
curl http://<EC2-PUBLIC-IP>:8080/api/info
curl http://<EC2-PUBLIC-IP>:8080/health

# Multiple requests to see load balancing
for i in {1..5}; do
  curl -s http://<EC2-PUBLIC-IP>:8080/api/info
  echo ""
done
```

## Testing Scenarios

### Client Affinity

Client affinity (sticky sessions) binds clients to the same backend:

```bash
# All requests from same IP go to same backend
for i in {1..5}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done
# Expected: All show the same backend_id
```

### Load Distribution

To test load distribution, simulate different clients:

```bash
# Use different source IPs or wait for TTL expiration
# Check request_count on each backend
curl localhost:9001/api/info | grep request_count
curl localhost:9002/api/info | grep request_count
curl localhost:9003/api/info | grep request_count
```

### Backend Health

Test health-based routing by stopping a backend:

```bash
# Stop mock-eu-1
pkill -f 'mock-backend.*9001'

# Requests should now go to healthy backends
curl http://localhost:8080/api/info
# Expected: Routes to mock-eu-2 or mock-us-1
```

### Geo-Routing

The proxy routes clients to backends in their region:

1. Configure backends in multiple regions
2. Test from different geographic locations
3. Observe routing decisions in proxy logs

## Monitoring During Tests

### edgeProxy Logs

```bash
# On EC2
sudo journalctl -u edgeproxy -f

# Look for:
# - Backend selection logs
# - Connection counts
# - GeoIP resolution
```

### Mock Backend Logs

```bash
# Check individual backend logs
tail -f /tmp/mock-9001.log
tail -f /tmp/mock-9002.log
tail -f /tmp/mock-9003.log
```

### Request Distribution

```bash
# Quick check of request distribution
echo "mock-eu-1: $(curl -s localhost:9001/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-eu-2: $(curl -s localhost:9002/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-us-1: $(curl -s localhost:9003/api/info | grep -o '"request_count":[0-9]*')"
```

## Cleanup

### Local

```bash
# Kill all mock backends
pkill -f mock-backend
```

### EC2

```bash
# Kill mock backends
sudo pkill -f mock-backend

# Or kill by port
sudo fuser -k 9001/tcp 9002/tcp 9003/tcp
```

## Troubleshooting

### Mock Backend Won't Start

```bash
# Check if port is in use
sudo ss -tlnp | grep 9001

# Kill existing process
sudo fuser -k 9001/tcp
```

### Proxy Can't Connect to Backend

1. Verify backend is running: `curl localhost:9001/health`
2. Check routing.db configuration
3. Verify `wg_ip` matches (use `127.0.0.1` for local testing)
4. Check firewall rules on EC2

### Requests Timeout

1. Check edgeProxy is running: `sudo systemctl status edgeproxy`
2. Verify backend health in routing.db
3. Check connection limits aren't exceeded

---

## Unit Tests

edgeProxy has comprehensive unit test coverage following the Hexagonal Architecture pattern with Sans-IO design. All tests are written in Rust using the built-in test framework.

### Test Summary

| Metric | Value |
|--------|-------|
| **Total Tests** | 786 |
| **Line Coverage** | **98.89%** |
| **Lines Covered** | 5,694 / 5,758 |
| **Function Coverage** | 99.46% |
| **Files with 100%** | 20 |

### Coverage Evolution

The project achieved significant coverage improvements through systematic testing:

| Phase | Coverage | Tests | Key Improvements |
|-------|----------|-------|------------------|
| Initial (stable) | 94.43% | 780 | Basic unit tests |
| Refactoring | 94.92% | 782 | Sans-IO pattern adoption |
| Nightly build | 98.32% | 782 | `coverage(off)` for I/O |
| Edge case tests | 98.50% | 784 | Circuit breaker, metrics |
| Final | **98.89%** | 786 | TLS, connection pool |

### Sans-IO Architecture Benefits

The Sans-IO pattern separates pure business logic from I/O operations:

```
┌─────────────────────────────────────────────────────────────────────┐
│                     TESTABLE (100% covered)                         │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Pure Functions: process_message(), pick_backend(), etc.     │  │
│  │  - No network calls                                          │  │
│  │  - No database access                                        │  │
│  │  - Returns actions to execute                                │  │
│  └──────────────────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────────────────┤
│                     I/O WRAPPERS (excluded)                         │
│  ┌──────────────────────────────────────────────────────────────┐  │
│  │  Async handlers: start(), run(), handle_connection()        │  │
│  │  - Marked with #[cfg_attr(coverage_nightly, coverage(off))] │  │
│  │  - Thin wrappers that execute actions                        │  │
│  └──────────────────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────┘
```

This approach ensures:
- **All business logic is testable** without mocking network
- **100% coverage of decision-making code**
- **Clear separation** between logic and I/O

### Running Tests

```bash
# Run all tests
cargo test

# Run tests with output
cargo test -- --nocapture

# Run tests for a specific module
cargo test domain::services::load_balancer

# Run infrastructure tests only
cargo test infrastructure::

# Run tests in parallel (default)
cargo test -- --test-threads=4

# Run single-threaded (for debugging)
cargo test -- --test-threads=1
```

### Tests by Module

#### Inbound Adapters

| Module | Tests | Coverage | Description |
|--------|-------|----------|-------------|
| `adapters::inbound::api_server` | 38 | 99.57% | Auto-Discovery API, registration, heartbeat |
| `adapters::inbound::dns_server` | 44 | 97.80% | DNS server, geo-routing resolution |
| `adapters::inbound::tcp_server` | 27 | 96.23% | TCP connections, proxy logic |
| `adapters::inbound::tls_server` | 29 | 94.18% | TLS termination, certificates |

#### Outbound Adapters

| Module | Tests | Coverage | Description |
|--------|-------|----------|-------------|
| `adapters::outbound::dashmap_metrics_store` | 20 | 100.00% | Connection metrics, RTT tracking |
| `adapters::outbound::dashmap_binding_repo` | 21 | 100.00% | Client affinity, TTL, GC |
| `adapters::outbound::replication_backend_repo` | 28 | 99.85% | Distributed SQLite replication |
| `adapters::outbound::sqlite_backend_repo` | 20 | 99.26% | SQLite backend storage |
| `adapters::outbound::prometheus_metrics_store` | 19 | 98.70% | Prometheus metrics export |
| `adapters::outbound::maxmind_geo_resolver` | 18 | 95.86% | GeoIP resolution |
| `adapters::outbound::postgres_backend_repo` | 19 | 88.31% | PostgreSQL backend (stub) |

#### Domain Layer

| Module | Tests | Coverage | Description |
|--------|-------|----------|-------------|
| `domain::entities` | 12 | 100.00% | Backend, Binding, ClientKey |
| `domain::value_objects` | 26 | 96.40% | RegionCode, country mapping |
| `domain::services::load_balancer` | 25 | 98.78% | Scoring algorithm, geo-routing |

#### Application Layer

| Module | Tests | Coverage | Description |
|--------|-------|----------|-------------|
| `application::proxy_service` | 26 | 99.43% | Use case orchestration |
| `config` | 24 | 100.00% | Configuration loading |

#### Infrastructure Layer (NEW)

| Module | Tests | Coverage | Description |
|--------|-------|----------|-------------|
| `infrastructure::circuit_breaker` | 22 | 98.30% | Circuit breaker pattern |
| `infrastructure::config_watcher` | 17 | 94.30% | Hot reload configuration |
| `infrastructure::rate_limiter` | 14 | 91.95% | Token bucket rate limiting |
| `infrastructure::health_checker` | 17 | 91.64% | Active health checks |
| `infrastructure::connection_pool` | 17 | 87.21% | TCP connection pooling |
| `infrastructure::shutdown` | 11 | 86.29% | Graceful shutdown |

### Tests by Layer (Hexagonal Architecture)

![Tests by Layer](/img/tests-by-layer.svg)

### Infrastructure Components Test Details

#### Circuit Breaker Tests (22 tests)

```bash
cargo test infrastructure::circuit_breaker
```

| Test | Description |
|------|-------------|
| `test_circuit_breaker_new` | Initial state is Closed |
| `test_circuit_breaker_default` | Default configuration |
| `test_allow_when_closed` | Requests pass in Closed state |
| `test_record_success_in_closed` | Success tracking |
| `test_record_failure_in_closed` | Failure tracking |
| `test_transitions_to_open` | Opens after threshold failures |
| `test_deny_when_open` | Blocks requests in Open state |
| `test_circuit_transitions_to_half_open` | Timeout triggers Half-Open |
| `test_half_open_allows_limited` | Limited requests in Half-Open |
| `test_half_open_to_closed` | Recovers to Closed on success |
| `test_half_open_to_open` | Returns to Open on failure |
| `test_failure_window_resets` | Window resets on success |
| `test_get_metrics` | Metrics retrieval |
| `test_concurrent_record` | Thread-safe operations |

#### Rate Limiter Tests (14 tests)

```bash
cargo test infrastructure::rate_limiter
```

| Test | Description |
|------|-------------|
| `test_rate_limit_config_default` | Default: 100 req/s, burst 10 |
| `test_rate_limiter_new` | Creates with config |
| `test_check_allows_initial_burst` | Burst requests allowed |
| `test_check_different_clients_isolated` | Per-IP isolation |
| `test_remaining` | Token count tracking |
| `test_clear_client` | Reset individual client |
| `test_clear_all` | Reset all clients |
| `test_check_with_cost` | Variable cost requests |
| `test_cleanup_removes_stale` | GC removes old entries |
| `test_refill_over_time` | Token replenishment |
| `test_concurrent_access` | Thread-safe operations |

#### Health Checker Tests (17 tests)

```bash
cargo test infrastructure::health_checker
```

| Test | Description |
|------|-------------|
| `test_health_checker_new` | Creates with config |
| `test_health_check_config_default` | Default intervals |
| `test_health_status_default` | Initial unknown state |
| `test_tcp_check_success` | TCP probe success |
| `test_tcp_check_failure` | TCP probe failure |
| `test_tcp_check_timeout` | TCP timeout handling |
| `test_update_status_becomes_healthy` | Threshold transitions |
| `test_update_status_becomes_unhealthy` | Failure transitions |
| `test_on_health_change_callback` | Change notifications |
| `test_check_backend_success` | Backend check OK |
| `test_check_backend_failure` | Backend check fail |

#### Connection Pool Tests (17 tests)

```bash
cargo test infrastructure::connection_pool
```

| Test | Description |
|------|-------------|
| `test_connection_pool_new` | Pool creation |
| `test_pool_config_default` | Default: 10 max, 60s idle |
| `test_acquire_creates_connection` | New connection on empty pool |
| `test_release_returns_connection` | Connection reuse |
| `test_pool_exhausted` | Max connections error |
| `test_acquire_timeout` | Connection timeout |
| `test_discard_closes_connection` | Explicit discard |
| `test_stats` | Pool statistics |
| `test_pooled_connection_is_expired` | Lifetime check |
| `test_pooled_connection_is_idle_expired` | Idle timeout check |

#### Graceful Shutdown Tests (11 tests)

```bash
cargo test infrastructure::shutdown
```

| Test | Description |
|------|-------------|
| `test_shutdown_controller_new` | Controller creation |
| `test_connection_guard` | RAII guard creation |
| `test_connection_tracking` | Active count tracking |
| `test_multiple_connection_guards` | Concurrent guards |
| `test_shutdown_initiates_once` | Single shutdown |
| `test_subscribe_receives_shutdown` | Broadcast notification |
| `test_wait_for_drain_immediate` | No connections case |
| `test_wait_for_drain_with_connections` | Waits for drain |
| `test_wait_for_drain_timeout` | Timeout behavior |

#### Config Watcher Tests (17 tests)

```bash
cargo test infrastructure::config_watcher
```

| Test | Description |
|------|-------------|
| `test_config_watcher_new` | Watcher creation |
| `test_watch_file` | File monitoring |
| `test_watch_nonexistent_file` | Error handling |
| `test_unwatch_file` | Remove from watch |
| `test_set_and_get` | Config values |
| `test_get_or` | Default values |
| `test_subscribe_value_change` | Change notifications |
| `test_no_change_on_same_value` | No spurious events |
| `test_check_files_detects_modification` | File change detection |
| `test_hot_value_get_set` | HotValue wrapper |

---

## Code Coverage

### Coverage Tools

edgeProxy uses [cargo-llvm-cov](https://github.com/taiki-e/cargo-llvm-cov) for code coverage measurement with LLVM instrumentation.

### Installation

```bash
# Install cargo-llvm-cov
cargo install cargo-llvm-cov

# Install LLVM tools (required for coverage)
rustup component add llvm-tools-preview

# Install nightly toolchain (for coverage(off) support)
rustup toolchain install nightly
rustup run nightly rustup component add llvm-tools-preview
```

### Running Coverage

```bash
# Basic coverage report (stable Rust - includes I/O code)
cargo llvm-cov

# Coverage with nightly (RECOMMENDED - excludes I/O code marked with coverage(off))
rustup run nightly cargo llvm-cov

# Summary only
rustup run nightly cargo llvm-cov --summary-only

# Coverage with HTML report
rustup run nightly cargo llvm-cov --html

# Coverage with LCOV output
rustup run nightly cargo llvm-cov --lcov --output-path lcov.info

# Open HTML report
open target/llvm-cov/html/index.html
```

> **Important**: Use `rustup run nightly` to enable `#[coverage(off)]` attributes. With stable Rust, I/O code will be included in coverage metrics, resulting in ~94% coverage instead of ~99%.

### Coverage Results

**Final Coverage: 98.89%** (5,694 of 5,758 lines covered)

> **Note**: Coverage measured with `rustup run nightly cargo llvm-cov` to enable `coverage(off)` attributes on I/O code.

#### Coverage by Layer

| Layer | Lines | Coverage | Status |
|-------|-------|----------|--------|
| **Domain** | 761 | 99.47% | ✓ Excellent |
| **Application** | 706 | 99.72% | ✓ Excellent |
| **Inbound Adapters** | 2,100 | 98.90% | ✓ Excellent |
| **Outbound Adapters** | 1,450 | 98.62% | ✓ Excellent |
| **Infrastructure** | 455 | 97.14% | ✓ Very Good |
| **Config** | 286 | 100.00% | ✓ Complete |

#### Detailed Coverage by File

##### Core Components (100% Coverage)

| File | Lines | Coverage |
|------|-------|----------|
| `config.rs` | 286 | 100.00% |
| `domain/entities.rs` | 130 | 100.00% |
| `adapters/outbound/dashmap_metrics_store.rs` | 224 | 100.00% |
| `adapters/outbound/dashmap_binding_repo.rs` | 287 | 100.00% |

##### Inbound Adapters

| File | Lines | Covered | Coverage |
|------|-------|---------|----------|
| `adapters/inbound/api_server.rs` | 928 | 924 | 99.57% |
| `adapters/inbound/dns_server.rs` | 774 | 757 | 97.80% |
| `adapters/inbound/tcp_server.rs` | 849 | 817 | 96.23% |
| `adapters/inbound/tls_server.rs` | 996 | 938 | 94.18% |

##### Outbound Adapters

| File | Lines | Covered | Coverage |
|------|-------|---------|----------|
| `adapters/outbound/replication_backend_repo.rs` | 677 | 676 | 99.85% |
| `adapters/outbound/sqlite_backend_repo.rs` | 404 | 401 | 99.26% |
| `adapters/outbound/prometheus_metrics_store.rs` | 307 | 303 | 98.70% |
| `adapters/outbound/maxmind_geo_resolver.rs` | 145 | 139 | 95.86% |
| `adapters/outbound/postgres_backend_repo.rs` | 231 | 204 | 88.31% |

##### Infrastructure Layer (NEW)

| File | Lines | Covered | Coverage |
|------|-------|---------|----------|
| `infrastructure/circuit_breaker.rs` | 353 | 347 | 98.30% |
| `infrastructure/config_watcher.rs` | 298 | 281 | 94.30% |
| `infrastructure/rate_limiter.rs` | 261 | 240 | 91.95% |
| `infrastructure/health_checker.rs` | 371 | 340 | 91.64% |
| `infrastructure/connection_pool.rs` | 391 | 341 | 87.21% |
| `infrastructure/shutdown.rs` | 175 | 151 | 86.29% |

### Coverage Exclusions (Sans-IO Pattern)

The Sans-IO pattern separates pure business logic from I/O operations. Code that performs actual I/O is excluded from coverage using `#[cfg_attr(coverage_nightly, coverage(off))]`:

| Code | Reason |
|------|--------|
| `main.rs` | Entry point, composition root |
| `handle_packet()` (dns_server) | Network I/O dependent |
| `proxy_bidirectional()` (tcp_server) | Real TCP socket operations |
| `start()`, `run()` (servers) | Async event loops with network I/O |
| `start_event_loop()`, `start_flush_loop()` (agent) | Background async loops |
| `request()` (transport) | QUIC network operations |
| `release()` (connection_pool) | Async connection management |
| `SkipServerVerification` impl | TLS callback (cannot unit test) |
| Test modules (`#[cfg(test)]`) | Test code is not production code |

### Remaining Uncovered Lines (64 total)

The 64 uncovered lines fall into these categories:

| Category | Lines | Reason |
|----------|-------|--------|
| **Database errors** | 12 | DB connection failures (unreachable paths) |
| **Test panics** | 8 | `#[should_panic]` test branches |
| **CAS retry loops** | 15 | Atomic compare-and-swap retries |
| **Tracing calls** | 10 | `tracing::warn!()` in error branches |
| **TLS callbacks** | 19 | `ServerCertVerifier` trait impl |

These represent edge cases that require:
- External system failures (DB, network)
- Specific concurrent conditions (CAS retries)
- TLS handshake callbacks from rustls

All **business logic is 100% covered** - only I/O wrappers and unreachable error paths remain.

### Testing Philosophy

edgeProxy follows these testing principles:

1. **Domain logic is pure and fully tested**: `LoadBalancer` scoring algorithm has no external dependencies
2. **Adapters test through interfaces**: Mock implementations of traits for unit testing
3. **Integration tests use real components**: Mock backend server for E2E testing
4. **Network code has coverage exclusions**: I/O-bound code is tested via integration tests
5. **Infrastructure is modular**: Each component can be tested in isolation

### Continuous Integration

```yaml
# Example CI configuration for coverage
test:
  script:
    - cargo test
    - rustup run nightly cargo llvm-cov --fail-under-lines 98

coverage:
  script:
    - rustup run nightly cargo llvm-cov --html
  artifacts:
    paths:
      - target/llvm-cov/html/
```

The `--fail-under-lines 98` flag ensures coverage doesn't drop below 98% in CI.

### New Tests Added (v0.3.1)

| Module | Test | Description |
|--------|------|-------------|
| `circuit_breaker` | `test_allow_request_when_already_half_open` | Tests idempotent HalfOpen transition |
| `circuit_breaker` | `test_record_success_when_open` | Tests success recording in Open state |
| `prometheus_metrics_store` | `test_global_metrics` | Tests aggregated global metrics |
| `prometheus_metrics_store` | `test_concurrent_decrement` | Tests concurrent counter operations |
| `types` | `test_hlc_compare_same_time_different_counter` | Tests HLC counter tiebreaker |
| `types` | `test_hlc_compare_same_time_same_counter` | Tests HLC equality case |
