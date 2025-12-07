---
sidebar_position: 1
sidebar_label: "What is it?"
slug: /
---

# What is edgeProxy?

**edgeProxy** is a high-performance distributed TCP proxy written in Rust, designed to operate at edge Points of Presence (POPs) worldwide. It routes client connections to optimal backends based on geographic proximity, backend health, current load, and capacity limits.

## Key Features

- **Geo-Aware Routing**: Routes clients to the nearest regional backend using MaxMind GeoIP
- **Client Affinity**: Sticky sessions with configurable TTL ensure consistent backend assignment
- **Weighted Load Balancing**: Intelligent scoring based on region, load, and backend weight
- **Soft/Hard Limits**: Graceful degradation with connection limits per backend
- **Dynamic Configuration**: Hot-reload routing database without restart
- **Zero-Copy Proxying**: Efficient bidirectional TCP copy with Tokio
- **WireGuard Ready**: Designed for overlay network connectivity between POPs

## Use Cases

| Scenario | Description |
|----------|-------------|
| **CDN/Edge Computing** | Global POPs serving content from nearest origin |
| **Gaming Servers** | Session affinity for stateful game connections |
| **Multi-Region APIs** | Automatic failover and geo-routing |
| **Database Proxies** | Read replica routing based on client location |

## Architecture Overview

![Architecture Overview](/img/architecture-overview.svg)

## Quick Start

```bash
# Clone the repository
git clone https://github.com/andrebassi/edgeproxy.git
cd edgeproxy

# Build and run
task build
task run

# Or with Docker
task docker-build
task docker-up
task docker-test
```

## Technology Stack

| Component | Technology |
|-----------|------------|
| Language | Rust 2021 Edition |
| Async Runtime | Tokio (full features) |
| Database | SQLite (rusqlite) |
| Concurrency | DashMap (lock-free) |
| GeoIP | MaxMind GeoLite2 |
| Networking | WireGuard overlay |

## Next Steps

- [Getting Started](./getting-started) - Installation and first run
- [Architecture](./architecture) - Deep dive into system design
- [Configuration](./configuration) - Environment variables and options
- [Docker Deployment](./deployment/docker) - Container-based deployment
- [Benchmark Results](./benchmark) - Performance tests across 9 global regions
