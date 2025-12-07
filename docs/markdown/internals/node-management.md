---
sidebar_position: 3
---

# Node Management

This document describes how to manage backend nodes in edgeProxy using the Taskfile commands.

## Overview

edgeProxy discovers backends dynamically from the `routing.db` SQLite database. The database is typically replicated across all POPs via Corrosion (or similar distributed state system). You can manage nodes using the provided Taskfile commands.

## Node Schema

Each backend node has the following attributes:

| Field | Type | Description |
|-------|------|-------------|
| `id` | TEXT | Unique identifier (e.g., `sa-node-1`) |
| `app` | TEXT | Application name (default: `myapp`) |
| `region` | TEXT | Geographic region (`sa`, `us`, `eu`, `ap`) |
| `wg_ip` | TEXT | WireGuard IP address |
| `port` | INTEGER | Backend port (default: 8080) |
| `healthy` | INTEGER | Health status (0=unhealthy, 1=healthy) |
| `weight` | INTEGER | Load balancing weight (higher = more traffic) |
| `soft_limit` | INTEGER | Preferred max connections |
| `hard_limit` | INTEGER | Absolute max connections |
| `deleted` | INTEGER | Soft delete flag (0=active, 1=deleted) |

## Commands

### Add a Node

```bash
task node-add -- <id> <region> <wg_ip> [port] [weight]
```

**Examples:**

```bash
# Add node with defaults (port=8080, weight=1)
task node-add -- sa-node-2 sa 10.50.1.2

# Add node with custom port
task node-add -- us-node-2 us 10.50.2.2 9000

# Add node with custom port and weight
task node-add -- eu-node-2 eu 10.50.3.2 8080 3
```

### Remove a Node (Soft Delete)

Marks the node as deleted without removing it from the database. The node can be restored later.

```bash
task node-remove -- <id>
```

**Example:**

```bash
task node-remove -- sa-node-2
```

### Delete a Node (Permanent)

Permanently removes the node from the database.

```bash
task node-delete -- <id>
```

**Example:**

```bash
task node-delete -- sa-node-2
```

### Enable a Node

Sets `healthy=1` and `deleted=0`, making the node available for traffic.

```bash
task node-enable -- <id>
```

**Example:**

```bash
task node-enable -- sa-node-1
```

### Disable a Node

Sets `healthy=0`, preventing traffic from being routed to this node.

```bash
task node-disable -- <id>
```

**Example:**

```bash
task node-disable -- sa-node-1
```

### Set Node Weight

Adjusts the load balancing weight. Higher weight means the node receives more traffic relative to others.

```bash
task node-weight -- <id> <weight>
```

**Example:**

```bash
# Give this node 3x more traffic
task node-weight -- sa-node-1 3
```

### Set Connection Limits

Configures soft and hard connection limits for a node.

- **soft_limit**: When reached, the load balancer starts preferring other nodes
- **hard_limit**: Absolute maximum connections; new connections are rejected

```bash
task node-limits -- <id> <soft_limit> <hard_limit>
```

**Example:**

```bash
task node-limits -- sa-node-1 200 500
```

## View Commands

### Show All Nodes

```bash
task db-show
```

### Show Healthy Nodes Only

```bash
task db-healthy
```

## Load Balancing Algorithm

When selecting a backend, edgeProxy uses a scoring system:

```
score = region_score * 100 + (load_factor / weight)

where:
  region_score = 0 (client region matches backend region)
               = 1 (backend in same region as POP)
               = 2 (fallback to other regions)

  load_factor = current_connections / soft_limit

  weight = configured backend weight (higher = preferred)
```

The backend with the lowest score is selected, respecting:
- Only healthy backends (`healthy = 1`)
- Backends under hard_limit
- Soft-deleted backends are excluded (`deleted = 0`)

## Dynamic Updates

edgeProxy reloads `routing.db` periodically (default: every 5 seconds). Changes to nodes take effect automatically without restarting the proxy.

To change the reload interval:

```bash
export EDGEPROXY_DB_RELOAD_SECS=10
```

## Best Practices

1. **Use soft delete** (`node-remove`) before permanent deletion to allow recovery
2. **Disable nodes** during maintenance rather than removing them
3. **Set appropriate weights** based on node capacity
4. **Configure limits** to prevent overload during traffic spikes
5. **Monitor healthy nodes** with `task db-healthy` before deployments
