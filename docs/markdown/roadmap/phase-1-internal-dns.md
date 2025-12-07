---
sidebar_position: 1
---

# Phase 1: Internal DNS (.internal)

**Goal**: Abstract backend IPs with DNS names for easier management and migration.

## Current State

```rust
// Hardcoded IPs in routing.db
backend.wg_ip = "10.50.4.1"  // NRT
backend.wg_ip = "10.50.4.2"  // SIN
```

## Target State

```rust
// DNS resolution
backend.address = "nrt.backends.internal"  // Resolves to 10.50.4.1
backend.address = "sin.backends.internal"  // Resolves to 10.50.4.2
```

## Implementation

![Internal DNS Service](/img/roadmap/phase-1-internal-dns.svg)

## DNS Schema

| Domain | Resolves To | Example |
|--------|-------------|---------|
| `<region>.backends.internal` | Backend WG IP | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | POP WG IP | `hkg.pops.internal` → `10.50.5.1` |
| `<app>.<region>.services.internal` | App endpoint | `api.nrt.services.internal` → `10.50.4.1:8080` |

## Benefits

- **Abstraction**: Change IPs without updating configs
- **Migration**: Move backends without downtime
- **Multi-tenancy**: Namespace per organization

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 2: Distributed Control Plane](./phase-2-corrosion)
