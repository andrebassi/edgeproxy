---
sidebar_position: 1
---

# Phase 1: IPv6 Private Network (6PN)

**Goal**: Migrate from IPv4 to IPv6 for unlimited address space and modern networking.

## Current State

```
Network: 10.50.0.0/16
Addresses: ~65,000
Protocol: IPv4 over WireGuard
```

## Target State

```
Network: fd00:edgeproxy::/48
Addresses: 2^80 (unlimited)
Protocol: IPv6 over WireGuard
```

## Address Scheme

```
fd00:edgeproxy:RRRR:BBBB::1

Where:
  fd00:edgeproxy = ULA prefix (Unique Local Address)
  RRRR          = Region code (0001=EU, 0002=US, 0003=SA, 0004=AP)
  BBBB          = Backend ID
  ::1           = Instance number
```

## Examples

| Backend | IPv4 (current) | IPv6 (future) |
|---------|----------------|---------------|
| EC2 Ireland | 10.50.0.1 | fd00:edgeproxy:0001:0001::1 |
| GRU | 10.50.1.1 | fd00:edgeproxy:0003:0001::1 |
| NRT | 10.50.4.1 | fd00:edgeproxy:0004:0001::1 |
| HKG POP | 10.50.5.1 | fd00:edgeproxy:0004:0100::1 |

## Dual-Stack Transition

```
Phase 1a: Add IPv6 alongside IPv4 (dual-stack)
Phase 1b: Prefer IPv6 for new connections
Phase 1c: Deprecate IPv4 internal traffic
```

## Benefits

- **Unlimited scale**: No address exhaustion
- **Modern standard**: IPv6-native applications
- **Simplified routing**: Hierarchical addressing
- **Future-proof**: Ready for next decade

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 2: Anycast BGP](./phase-2-anycast-bgp)
