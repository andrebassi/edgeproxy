---
sidebar_position: 3
---

# Database Schema

The `routing.db` SQLite database contains the backend configuration.

## Table Structure

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- Unique identifier (e.g., "sa-node-1")
    app TEXT,                 -- Application name (e.g., "myapp")
    region TEXT,              -- Region code: "sa", "us", "eu"
    wg_ip TEXT,               -- Backend IP address
    port INTEGER,             -- Backend port
    healthy INTEGER,          -- 1 = healthy, 0 = unhealthy
    weight INTEGER,           -- Load balancing weight (higher = more traffic)
    soft_limit INTEGER,       -- Preferred max connections
    hard_limit INTEGER,       -- Absolute max connections
    deleted INTEGER DEFAULT 0 -- Soft delete flag
);
```

## Example Data

```sql
INSERT INTO backends VALUES
    ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
    ('sa-node-2', 'myapp', 'sa', '10.50.1.2', 8080, 1, 1, 50, 100, 0),
    ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
    ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

## Field Descriptions

### `region`

Geographic region identifier. Standard values:

| Code | Description |
|------|-------------|
| `sa` | South America (Brazil, Argentina, Chile, etc.) |
| `us` | North America (USA, Canada, Mexico) |
| `eu` | Europe (Germany, France, UK, etc.) |
| `ap` | Asia Pacific (Japan, Singapore, Australia) |

### `weight`

Relative weight for load balancing. Higher values receive more traffic:

- `weight=2`: Receives 2x more traffic than weight=1
- `weight=1`: Standard traffic share
- `weight=0`: Effectively disabled (not recommended, use `healthy=0`)

### `soft_limit` vs `hard_limit`

- **soft_limit**: Target connection count. Beyond this, the backend is considered "loaded" and receives a higher score.
- **hard_limit**: Absolute maximum. Connections are refused beyond this limit.

```
connections < soft_limit  → Low score (preferred)
soft_limit ≤ connections < hard_limit → Higher score (less preferred)
connections ≥ hard_limit → Backend excluded
```

## Database Management

### View All Backends

```bash
sqlite3 routing.db "SELECT * FROM backends WHERE deleted=0"
```

### Add a Backend

```bash
sqlite3 routing.db "INSERT INTO backends VALUES ('eu-node-2', 'myapp', 'eu', '10.50.3.2', 8080, 1, 2, 50, 100, 0)"
```

### Mark Backend Unhealthy

```bash
sqlite3 routing.db "UPDATE backends SET healthy=0 WHERE id='sa-node-1'"
```

### Soft Delete Backend

```bash
sqlite3 routing.db "UPDATE backends SET deleted=1 WHERE id='sa-node-1'"
```

### Adjust Weight

```bash
sqlite3 routing.db "UPDATE backends SET weight=3 WHERE id='us-node-1'"
```

## Automatic Reload

Changes are automatically picked up based on `EDGEPROXY_DB_RELOAD_SECS` (default: 5 seconds). No restart required.
