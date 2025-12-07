---
sidebar_position: 2
---

# Phase 2: Distributed Control Plane (Corrosion)

**Goal**: Replace local SQLite with replicated SQLite for real-time consistency across all POPs.

## Current State

![Current State - Manual Sync](/img/roadmap/phase-2-corrosion-current.svg)

## Target State

![Target State - Corrosion Cluster](/img/roadmap/phase-2-corrosion-target.svg)

## Corrosion Integration

```toml
# corrosion.toml
[db]
path = "/var/lib/edgeproxy/routing.db"

[cluster]
name = "edgeproxy"
bootstrap = ["10.50.0.1:4001", "10.50.5.1:4001"]

[gossip]
addr = "0.0.0.0:4001"
```

## Benefits

- **Real-time sync**: Changes propagate in ~100ms
- **No manual intervention**: Automatic replication
- **Partition tolerance**: Works during network splits
- **Event-driven**: Subscribe to changes

## Related

- [Roadmap Overview](../roadmap/)
- [Phase 1: Internal DNS](./phase-1-internal-dns)
- [Phase 3: Auto-Discovery](./phase-3-auto-discovery)
