---
sidebar_position: 0
---

# Future Architecture Roadmap

This document outlines the planned evolution of edgeProxy towards a fully distributed, self-healing edge computing platform.

:::info Current State
edgeProxy v1 is a functional geo-aware TCP proxy with WireGuard overlay. This roadmap describes the path to v2 and beyond.
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

## Implementation Phases

| Phase | Description | Status |
|-------|-------------|--------|
| [Phase 1: Internal DNS](./phase-1-internal-dns) | Abstract backend IPs with DNS names | Planned |
| [Phase 2: Corrosion](./phase-2-corrosion) | Distributed control plane with SQLite replication | Planned |
| [Phase 3: Auto-Discovery](./phase-3-auto-discovery) | Automatic backend registration | Planned |
| [Phase 4: IPv6 (6PN)](./phase-4-ipv6) | IPv6 private network | Planned |
| [Phase 5: Anycast BGP](./phase-5-anycast-bgp) | BGP-based traffic routing | Planned |
| [Phase 6: Health Checks](./phase-6-health-checks) | Active health monitoring | Planned |

---

## Related Documentation

- [Architecture](../architecture) - Current architecture
- [WireGuard](../wireguard) - Network overlay details
- [Benchmarks](../benchmark) - Performance measurements
