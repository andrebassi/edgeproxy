---
sidebar_position: 0
---

# Future Architecture Roadmap

This document outlines the planned evolution of edgeProxy towards a fully distributed, self-healing edge computing platform.

:::tip Current Version: 0.2.0
edgeProxy now includes **TLS termination**, **Auto-Discovery API**, **Internal DNS**, and **Corrosion integration**. See [Configuration](../configuration) for details.
:::

## Design Principles

edgeProxy follows proven patterns from production edge platforms:

- **WireGuard as Foundation**: All internal communication flows over WireGuard mesh. It provides the **backhaul** between POPs - the internal network that carries traffic between datacenters. When a user connects to the nearest edge server but their backend runs in a different region, the proxy transparently routes through low-latency WireGuard tunnels instead of going back through the public internet.

![WireGuard Backhaul](/img/roadmap/backhaul-diagram.svg)

- **Rust + Tokio for Performance**: Critical path components built in Rust using Tokio async runtime for predictable latency without GC pauses.
- **6PN (IPv6 Private Network)**: Internal connectivity uses IPv6 private addressing, enabling unlimited address space for backends and services.
- **Anycast Global Network**: Single IP address announced from multiple locations, with BGP handling optimal routing.

---

## Architecture Comparison

### Current vs Target Architecture

![Future Architecture](/img/architecture-future.svg)

| Component | v1 (Current) | v2 (Target) |
|-----------|--------------|-------------|
| **Traffic Routing** | GeoDNS | Anycast BGP |
| **Edge Proxy** | edgeProxy (Rust) | edgeProxy (Rust) |
| **Control Plane** | routing.db (local) | Corrosion (replicated) |
| **Private Network** | WireGuard IPv4 | WireGuard IPv6 (6PN) |
| **Service Discovery** | Static (manual) | Dynamic (auto-register) |
| **Internal DNS** | None | .internal domains |
| **Health Checks** | Passive | Active + Passive |

---

## Completed Features (v0.2.0)

The following features have been implemented and are documented in [Configuration](../configuration):

| Feature | Description | Documentation |
|---------|-------------|---------------|
| **TLS Termination** | HTTPS support with auto-generated or custom certificates | [Configuration](../configuration#tls-settings) |
| **Internal DNS** | Geo-aware `.internal` domain resolution | [Configuration](../configuration#internal-dns-server) |
| **Auto-Discovery API** | Dynamic backend registration/deregistration | [Configuration](../configuration#auto-discovery-api) |
| **Corrosion Integration** | Distributed SQLite replication across POPs | [Configuration](../configuration#distributed-control-plane-corrosion) |
| **358 Unit Tests** | Comprehensive test coverage (99.38%) | [Testing](../testing#unit-tests) |

---

## Implementation Phases

| Phase | Description | Status |
|-------|-------------|--------|
| [Phase 1: IPv6 (6PN)](./phase-1-ipv6) | IPv6 private network | Planned |
| [Phase 2: Anycast BGP](./phase-2-anycast-bgp) | BGP-based traffic routing | Planned |
| [Phase 3: Health Checks](./phase-3-health-checks) | Active health monitoring | Planned |

---

## Related Documentation

- [Architecture](../architecture) - Current architecture
- [Configuration](../configuration) - All environment variables and features
- [WireGuard](../wireguard) - Network overlay details
- [Benchmarks](../benchmark) - Performance measurements
