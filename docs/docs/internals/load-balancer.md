---
sidebar_position: 1
---

# Load Balancer Internals

This document provides a deep technical dive into edgeProxy's load balancing algorithm, scoring system, and implementation details.

## Overview

edgeProxy uses a **weighted scoring algorithm** that considers:

1. Geographic proximity (region matching)
2. Current backend load (connection count)
3. Backend capacity (soft/hard limits)
4. Configured weights

The goal is to route traffic to the "best" backend where best = lowest score.

## Scoring Algorithm

### Formula

```
score = region_score * 100 + (load_factor / weight)

where:
  region_score = 0 | 1 | 2 (lower is better)
  load_factor = current_connections / soft_limit
  weight = backend weight (1-10, higher receives more traffic)
```

### Region Score

| Condition | Score | Description |
|-----------|-------|-------------|
| Client region == Backend region | 0 | Best match - same region |
| Backend region == Local POP region | 1 | Good match - local region |
| Other | 2 | Fallback - cross-region |

**Example:**

```
Client from Brazil connecting to SA POP:
├── sa-node-1 (region=sa) → region_score = 0 (client match)
├── sa-node-2 (region=sa) → region_score = 0 (client match)
├── us-node-1 (region=us) → region_score = 2 (fallback)
└── eu-node-1 (region=eu) → region_score = 2 (fallback)
```

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

### Source Code (`lb.rs`)

```rust
use crate::model::Backend;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Default)]
pub struct BackendMetrics {
    pub current_conns: AtomicU64,
    pub last_rtt_ms: AtomicU64,
}

impl BackendMetrics {
    pub fn new() -> Self {
        Self::default()
    }
}

pub fn pick_backend(
    backends: &[Backend],
    local_region: &str,
    client_region: Option<&str>,
    metrics: &DashMap<String, BackendMetrics>,
) -> Option<Backend> {
    let mut best: Option<(Backend, f64)> = None;

    for b in backends {
        // Skip unhealthy backends
        if !b.healthy {
            continue;
        }

        // Get current connection count
        let conns = metrics
            .get(&b.id)
            .map(|m| m.current_conns.load(Ordering::Relaxed))
            .unwrap_or(0);

        // Skip backends at hard limit
        if conns >= b.hard_limit as u64 {
            continue;
        }

        // Calculate region score
        let region_score: u64 = match client_region {
            Some(cr) if cr == b.region => 0,  // Client region match
            _ if b.region == local_region => 1, // Local POP region
            _ => 2,                            // Fallback
        };

        // Calculate load factor
        let load_factor = conns as f64 / b.soft_limit as f64;

        // Final score (lower is better)
        let score = (region_score * 100) as f64 + (load_factor / b.weight as f64);

        // Update best if this is better
        match &best {
            Some((_, best_score)) => {
                if score < *best_score {
                    best = Some((b.clone(), score));
                }
            }
            None => {
                best = Some((b.clone(), score));
            }
        }
    }

    best.map(|(b, _)| b)
}
```

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

## Future Improvements

1. **Latency-based routing**: Include RTT in score
2. **Adaptive weights**: Auto-adjust based on error rates
3. **Circuit breaker**: Temporary exclusion on failures
4. **Consistent hashing**: For stateful backends

## Next Steps

- [Architecture](../architecture) - System overview
- [Configuration](../configuration) - Tuning options
- [Client Affinity](./client-affinity) - Sticky sessions
