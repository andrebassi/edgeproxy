---
sidebar_position: 1
---

# Load Balancer

This document provides a deep technical dive into edgeProxy's load balancing algorithm, scoring system, and implementation details.

## Overview

edgeProxy uses a **weighted scoring algorithm** that considers:

1. **Country-based routing** (exact country match - highest priority)
2. **Region-based routing** (continental region matching)
3. Current backend load (connection count)
4. Backend capacity (soft/hard limits)
5. Configured weights

The goal is to route traffic to the "best" backend where best = lowest score.

## GeoIP Database

The MaxMind GeoLite2-Country database is **embedded directly into the binary** at compile time using Rust's `include_bytes!` macro. This means:

- No external database file needed at runtime
- Single binary deployment
- Automatic geo-routing without configuration
- Optional override via `EDGEPROXY_GEOIP_PATH` environment variable

## Scoring Algorithm

### Formula

```
score = region_score * 100 + (load_factor / weight)

where:
  region_score = 0 | 1 | 2 (lower is better)
  load_factor = current_connections / soft_limit
  weight = backend weight (1-10, higher receives more traffic)
```

### Geo Score (Country + Region)

The geo scoring system prioritizes **country** first, then **region**, ensuring users are routed to the geographically closest backend:

| Condition | Score | Description |
|-----------|-------|-------------|
| Client country == Backend country | 0 | Best match - same country (e.g., FR → CDG) |
| Client region == Backend region | 1 | Good match - same region (e.g., FR → any EU) |
| Backend region == Local POP region | 2 | Local POP region |
| Other | 3 | Fallback - cross-region |

**Example:**

```
Client from France (country=FR, region=eu) connecting:
├── fly-cdg-1 (country=FR, region=eu) → geo_score = 0 (country match!)
├── fly-fra-1 (country=DE, region=eu) → geo_score = 1 (region match)
├── fly-lhr-1 (country=GB, region=eu) → geo_score = 1 (region match)
├── fly-iad-1 (country=US, region=us) → geo_score = 3 (fallback)
└── fly-nrt-1 (country=JP, region=ap) → geo_score = 3 (fallback)
```

### Country to Region Mapping

The following countries are mapped to regions:

| Region | Countries |
|--------|-----------|
| **sa** (South America) | BR, AR, CL, PE, CO, UY, PY, BO, EC |
| **us** (North America) | US, CA, MX |
| **eu** (Europe) | PT, ES, FR, DE, NL, IT, GB, IE, BE, CH, AT, PL, CZ, SE, NO, DK, FI |
| **ap** (Asia Pacific) | JP, KR, TW, HK, SG, MY, TH, VN, ID, PH, AU, NZ |
| **us** (Fallback) | All other countries |

### Load Factor

```rust
load_factor = current_connections as f64 / soft_limit as f64
```

| Connections | Soft Limit | Load Factor |
|-------------|------------|-------------|
| 0 | 50 | 0.0 |
| 25 | 50 | 0.5 |
| 50 | 50 | 1.0 |
| 75 | 50 | 1.5 |

**Note:** Backends exceeding `hard_limit` are excluded entirely.

### Weight Impact

Higher weight = lower score = more traffic:

```
score contribution = load_factor / weight

weight=1: load_factor contributes 100%
weight=2: load_factor contributes 50%
weight=3: load_factor contributes 33%
```

## Complete Scoring Example

The following diagram shows how the load balancer scores and selects backends based on region matching, current load, and weight configuration:

![Load Balancer Scoring](/img/load-balancer-scoring.svg)

## Implementation

### Hexagonal Architecture

The load balancer is implemented as a **pure domain service** with NO external dependencies. It doesn't know about DashMap, SQLite, or any infrastructure.

```
src/domain/services/load_balancer.rs  ← Pure business logic
```

### Source Code (`domain/services/load_balancer.rs`)

```rust
use crate::domain::entities::{Backend, GeoInfo};
use crate::domain::value_objects::RegionCode;

/// Load balancer service - PURE function, no external dependencies
pub struct LoadBalancer;

impl LoadBalancer {
    /// Select the best backend for a client.
    ///
    /// Note: `get_conn_count` is a closure - the LoadBalancer doesn't know
    /// about DashMap or any specific metrics implementation.
    pub fn pick_backend<F>(
        backends: &[Backend],
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
        get_conn_count: F,  // Injected dependency via closure
    ) -> Option<Backend>
    where
        F: Fn(&str) -> usize,
    {
        let mut best: Option<(Backend, f64)> = None;

        for backend in backends.iter().filter(|b| b.healthy) {
            let current = get_conn_count(&backend.id) as f64;

            let soft = if backend.soft_limit == 0 { 1.0 } else { backend.soft_limit as f64 };
            let hard = if backend.hard_limit == 0 { f64::MAX } else { backend.hard_limit as f64 };

            // Skip if at hard limit
            if current >= hard {
                continue;
            }

            // Calculate geo score (0-3 scale, lower is better)
            let geo_score = Self::calculate_geo_score(backend, local_region, client_geo);

            // Calculate load factor
            let load_factor = current / soft;

            // Weight factor
            let weight = if backend.weight == 0 { 1.0 } else { backend.weight as f64 };

            // Final score
            let score = geo_score * 100.0 + (load_factor / weight);

            match &best {
                Some((_, best_score)) if score < *best_score => {
                    best = Some((backend.clone(), score));
                }
                None => {
                    best = Some((backend.clone(), score));
                }
                _ => {}
            }
        }

        best.map(|(backend, _)| backend)
    }

    fn calculate_geo_score(
        backend: &Backend,
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
    ) -> f64 {
        match client_geo {
            Some(geo) if backend.country == geo.country => 0.0,  // Same country
            Some(geo) if backend.region == geo.region => 1.0,    // Same region
            _ if backend.region == *local_region => 2.0,         // Local POP region
            _ => 3.0,                                             // Fallback
        }
    }
}
```

### Usage in Application Layer

The `ProxyService` calls the LoadBalancer with an injected closure:

```rust
// application/proxy_service.rs
let metrics = self.metrics.clone();

let backend = LoadBalancer::pick_backend(
    &backends,
    &self.local_region,
    client_geo.as_ref(),
    |id| metrics.get_connection_count(id),  // Closure injected here
)?;
```

### Why This Design?

| Benefit | Explanation |
|---------|-------------|
| **Testable** | Can test with mock data, no DashMap needed |
| **Pure** | Same inputs always produce same outputs |
| **Flexible** | Metrics can come from DashMap, Redis, Prometheus, etc. |
| **Clear** | Business logic is isolated from infrastructure |

### Key Design Decisions

#### 1. Lock-Free Metrics

Using `DashMap` with atomic counters avoids contention:

```rust
// Increment on connection
metrics.entry(backend_id)
    .or_insert_with(BackendMetrics::new)
    .current_conns.fetch_add(1, Ordering::Relaxed);

// Decrement on disconnect
metrics.get(&backend_id)
    .map(|m| m.current_conns.fetch_sub(1, Ordering::Relaxed));
```

#### 2. Region Priority

The `* 100` multiplier ensures region always dominates:

```
Best case (same region):     0 + load_factor
Worst case (different): 200 + load_factor

Even a fully loaded local backend (load_factor=2.0)
beats an empty remote backend (200.0)
```

#### 3. Weight as Divisor

Using weight as a divisor provides intuitive scaling:

```
weight=2 receives 2x traffic of weight=1
weight=3 receives 3x traffic of weight=1
```

## Edge Cases

### No Healthy Backends

```rust
if rt.backends.is_empty() || all_unhealthy {
    tracing::warn!("no healthy backend available");
    return Ok(()); // Connection dropped
}
```

### All Backends at Capacity

When all backends exceed `hard_limit`, the connection is dropped:

```rust
if conns >= b.hard_limit as u64 {
    continue; // Skip this backend
}

// If no backends remain after filtering
best.is_none() → connection dropped
```

### Unknown Client Region

Without GeoIP, falls back to local POP region:

```rust
let client_region = state.geo
    .as_ref()
    .and_then(|g| g.region_for_ip(client_ip));

// If None, region_score uses local_region comparison only
```

## Performance Characteristics

### Time Complexity

- Backend iteration: O(n) where n = backend count
- Metric lookup: O(1) average (DashMap)
- Score calculation: O(1)

**Total:** O(n) per connection

### Space Complexity

- Metrics map: O(n) where n = unique backends
- Per-metric: 16 bytes (two AtomicU64)

### Benchmarks

| Backends | Avg Selection Time |
|----------|-------------------|
| 10 | ~100ns |
| 100 | ~1μs |
| 1000 | ~10μs |

## Tuning Guidelines

### Weight Distribution

| Scenario | Weights |
|----------|---------|
| Equal distribution | All 1 |
| Prefer primary | Primary: 3, Secondary: 1 |
| Gradual rollout | New: 1, Old: 9 |

### Soft/Hard Limits

```
soft_limit = comfortable_connections
hard_limit = absolute_maximum

Recommendation:
  soft_limit = 70% of hard_limit
  hard_limit = max_fd / expected_backends
```

### Region Configuration

Ensure backends match expected client distribution:

```
70% traffic from SA → 70% backends in sa
20% traffic from US → 20% backends in us
10% traffic from EU → 10% backends in eu
```

## Monitoring

### Key Metrics

1. **Connection distribution**: Are backends balanced?
2. **Region routing accuracy**: Are clients hitting local backends?
3. **Capacity utilization**: Soft/hard limit hits?

### Debug Logging

```bash
DEBUG=1 ./edge-proxy
```

Output:

```
DEBUG edge_proxy::proxy: proxying 10.0.0.1 -> sa-node-1 (10.50.1.1:8080)
DEBUG edge_proxy::lb: scores: sa-node-1=0.3, sa-node-2=0.2, selected=sa-node-2
```

## Geo-Routing Benchmark Results

The following benchmark was conducted on **2025-12-07** using VPN connections from multiple countries to validate the geo-routing algorithm with 10 Fly.io backends deployed globally.

### Test Environment

- **edgeProxy version**: 0.1.0
- **GeoIP Database**: MaxMind GeoLite2-Country (embedded)
- **Backends**: 10 nodes across 4 regions (sa, us, eu, ap)
- **Test method**: VPN connection from each country, `curl localhost:8080`

### Results: 9/9 Tests Passed (100%)

| # | VPN Location | Country | Expected Backend | Actual Result | Status |
|---|--------------|---------|------------------|---------------|--------|
| 1 | Paris, France | FR | CDG | CDG | PASS |
| 2 | Frankfurt, Germany | DE | FRA | FRA | PASS |
| 3 | London, UK | GB | LHR | LHR | PASS |
| 4 | Detroit, USA | US | IAD | IAD | PASS |
| 5 | Las Vegas, USA | US | IAD | IAD | PASS |
| 6 | Tokyo, Japan | JP | NRT | NRT | PASS |
| 7 | Singapore | SG | SIN | SIN | PASS |
| 8 | Sydney, Australia | AU | SYD | SYD | PASS |
| 9 | Sao Paulo, Brazil | BR | GRU | GRU | PASS |

### Key Observations

1. **Country-based routing works correctly**: France routes to CDG (Paris), Germany to FRA (Frankfurt), UK to LHR (London)
2. **Region fallback works**: Multiple US locations (Detroit, Las Vegas) correctly fall back to IAD since all US backends have the same country code
3. **VPN change detection**: The proxy automatically detects VPN/country changes and clears client bindings
4. **Embedded GeoIP**: No external database file needed - MaxMind DB is compiled into the binary

### Backend Configuration

```
| Backend ID   | Country | Region | Location           |
|--------------|---------|--------|-------------------|
| fly-gru-1    | BR      | sa     | Sao Paulo, Brazil |
| fly-iad-1    | US      | us     | Virginia, USA     |
| fly-ord-1    | US      | us     | Chicago, USA      |
| fly-lax-1    | US      | us     | Los Angeles, USA  |
| fly-lhr-1    | GB      | eu     | London, UK        |
| fly-fra-1    | DE      | eu     | Frankfurt, Germany|
| fly-cdg-1    | FR      | eu     | Paris, France     |
| fly-nrt-1    | JP      | ap     | Tokyo, Japan      |
| fly-sin-1    | SG      | ap     | Singapore         |
| fly-syd-1    | AU      | ap     | Sydney, Australia |
```

## Future Improvements

1. **Latency-based routing**: Include RTT in score
2. **Adaptive weights**: Auto-adjust based on error rates
3. **Circuit breaker**: Temporary exclusion on failures
4. **Consistent hashing**: For stateful backends
5. **City/State-level routing**: For large countries like US, route to closest regional backend

## Next Steps

- [Architecture](../architecture) - System overview
- [Configuration](../configuration) - Tuning options
- [Client Affinity](./client-affinity) - Sticky sessions
