---
sidebar_position: 5
---

# Configuration

edgeProxy is configured entirely through environment variables. This document covers all available options with examples.

## Environment Variables

### Core Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_LISTEN_ADDR` | `0.0.0.0:8080` | TCP address to listen on |
| `EDGEPROXY_DB_PATH` | `routing.db` | Path to SQLite routing database |
| `EDGEPROXY_REGION` | `sa` | Local POP region identifier |

### Database Sync

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_DB_RELOAD_SECS` | `5` | Interval to reload routing.db (seconds) |

### Client Affinity

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Client binding TTL (10 minutes) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Garbage collection interval |

### Debugging

| Variable | Default | Description |
|----------|---------|-------------|
| `DEBUG` | *(unset)* | Enable debug logging when set |

### TLS Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_TLS_ENABLED` | `false` | Enable TLS server |
| `EDGEPROXY_TLS_LISTEN_ADDR` | `0.0.0.0:8443` | TLS listen address |
| `EDGEPROXY_TLS_CERT` | *(none)* | Path to TLS certificate (PEM) |
| `EDGEPROXY_TLS_KEY` | *(none)* | Path to TLS private key (PEM) |

### Internal DNS Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Enable DNS server |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | DNS listen address |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | DNS domain suffix |

### Auto-Discovery API Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_API_ENABLED` | `false` | Enable Auto-Discovery API |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | API listen address |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | Backend heartbeat TTL |

### Corrosion Settings (Distributed SQLite)

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_CORROSION_ENABLED` | `false` | Enable Corrosion backend |
| `EDGEPROXY_CORROSION_API_URL` | `http://localhost:8080` | Corrosion HTTP API URL |
| `EDGEPROXY_CORROSION_POLL_SECS` | `5` | Polling interval for backend sync |

## Example Configurations

### Development

```bash
export EDGEPROXY_LISTEN_ADDR="127.0.0.1:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="./routing.db"
export EDGEPROXY_BINDING_TTL_SECS="60"
export DEBUG="1"

./target/release/edge-proxy
```

### Production (South America POP)

```bash
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="/data/routing.db"
export EDGEPROXY_DB_RELOAD_SECS="5"
export EDGEPROXY_BINDING_TTL_SECS="600"
export EDGEPROXY_BINDING_GC_INTERVAL_SECS="60"

./edge-proxy
```

### Docker Compose

```yaml
services:
  pop-sa:
    image: edgeproxy:latest
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8080:8080"
    volumes:
      - ./routing.db:/app/routing.db:ro
```

## Routing Database Schema

The `routing.db` SQLite database contains the backend configuration:

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

### Example Data

```sql
INSERT INTO backends VALUES
    ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
    ('sa-node-2', 'myapp', 'sa', '10.50.1.2', 8080, 1, 1, 50, 100, 0),
    ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
    ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

### Field Descriptions

#### `region`

Geographic region identifier. Standard values:

| Code | Description |
|------|-------------|
| `sa` | South America (Brazil, Argentina, Chile, etc.) |
| `us` | North America (USA, Canada, Mexico) |
| `eu` | Europe (Germany, France, UK, etc.) |
| `ap` | Asia Pacific (Japan, Singapore, Australia) |

#### `weight`

Relative weight for load balancing. Higher values receive more traffic:

- `weight=2`: Receives 2x more traffic than weight=1
- `weight=1`: Standard traffic share
- `weight=0`: Effectively disabled (not recommended, use `healthy=0`)

#### `soft_limit` vs `hard_limit`

- **soft_limit**: Target connection count. Beyond this, the backend is considered "loaded" and receives a higher score.
- **hard_limit**: Absolute maximum. Connections are refused beyond this limit.

```
connections < soft_limit  → Low score (preferred)
soft_limit ≤ connections < hard_limit → Higher score (less preferred)
connections ≥ hard_limit → Backend excluded
```

## GeoIP

The MaxMind GeoLite2 database is **embedded in the binary** - no external download or configuration required.

### Country to Region Mapping

Default mapping in `state.rs`:

```rust
match iso_code {
    // South America
    "BR" | "AR" | "CL" | "PE" | "CO" | "UY" | "PY" | "BO" | "EC" => "sa",

    // North America
    "US" | "CA" | "MX" => "us",

    // Europe
    "PT" | "ES" | "FR" | "DE" | "NL" | "IT" | "GB" | "IE" | "BE" | "CH" => "eu",

    // Default fallback
    _ => "us",
}
```

## Hot Reload

The routing database is automatically reloaded every `EDGEPROXY_DB_RELOAD_SECS` seconds. To update configuration:

1. Modify the SQLite database:
   ```bash
   sqlite3 routing.db "UPDATE backends SET healthy=0 WHERE id='sa-node-1'"
   ```

2. Wait for reload (check logs):
   ```
   INFO edge_proxy::db: routing reload ok, version=5 backends=9
   ```

No restart required.

## Logging

### Log Levels

- **INFO** (default): Startup messages, routing reloads
- **DEBUG** (when `DEBUG=1`): Connection details, backend selection

### Sample Output

```
INFO edge_proxy: starting edgeProxy region=sa listen=0.0.0.0:8080
INFO edge_proxy::proxy: edgeProxy listening on 0.0.0.0:8080
INFO edge_proxy::db: routing reload ok, version=1 backends=9
DEBUG edge_proxy::proxy: proxying 10.10.0.100 -> sa-node-1 (10.10.1.1:8080)
```

---

## Internal DNS Server

The DNS server provides geo-aware name resolution for `.internal` domains.

### Usage

```bash
# Enable DNS server
export EDGEPROXY_DNS_ENABLED=true
export EDGEPROXY_DNS_LISTEN_ADDR=0.0.0.0:5353
export EDGEPROXY_DNS_DOMAIN=internal

# Query for best backend IP (geo-aware)
dig @localhost -p 5353 myapp.internal A

# Response: Best backend IP based on client location
;; ANSWER SECTION:
myapp.internal.    300    IN    A    10.50.1.5
```

### DNS Schema

| Domain | Resolves To | Example |
|--------|-------------|---------|
| `<app>.internal` | Best backend IP | `myapp.internal` → `10.50.1.5` |
| `<region>.backends.internal` | Backend WG IP | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | POP WG IP | `hkg.pops.internal` → `10.50.5.1` |

### Benefits

- **Abstraction**: Change IPs without updating configs
- **Migration**: Move backends without downtime
- **Geo-aware**: Returns best backend based on client location

---

## Auto-Discovery API

The API allows backends to automatically register and deregister.

### Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check + version + backend count |
| POST | `/api/v1/register` | Register a new backend |
| POST | `/api/v1/heartbeat/:id` | Update backend heartbeat |
| GET | `/api/v1/backends` | List all registered backends |
| GET | `/api/v1/backends/:id` | Get specific backend details |
| DELETE | `/api/v1/backends/:id` | Deregister a backend |

### Registration Example

```bash
# Enable API
export EDGEPROXY_API_ENABLED=true
export EDGEPROXY_API_LISTEN_ADDR=0.0.0.0:8081
export EDGEPROXY_HEARTBEAT_TTL_SECS=60

# Register a backend
curl -X POST http://localhost:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "backend-eu-1",
    "app": "myapp",
    "region": "eu",
    "ip": "10.50.1.1",
    "port": 8080,
    "weight": 2,
    "soft_limit": 100,
    "hard_limit": 150
  }'

# Send heartbeat (keep alive)
curl -X POST http://localhost:8081/api/v1/heartbeat/backend-eu-1

# List all backends
curl http://localhost:8081/api/v1/backends
```

### Benefits

- **Zero configuration**: Backends just start and register
- **Automatic scaling**: New instances appear automatically
- **Graceful shutdown**: Clean deregistration
- **TTL-based health**: Unhealthy = expired = deregistered

---

## Distributed Control Plane (Corrosion)

Corrosion enables distributed SQLite replication across all POPs.

### Architecture

![Corrosion Architecture](/img/corrosion-architecture.svg)

### How It Works

When `EDGEPROXY_CORROSION_ENABLED=true`, edgeProxy **ignores** the local `EDGEPROXY_DB_PATH` and instead queries the Corrosion HTTP API for backend data. Corrosion handles all replication between POPs automatically.

![Corrosion Data Flow](/img/corrosion-data-flow.svg)

### Configuration

```bash
# Enable Corrosion backend (replaces local SQLite)
export EDGEPROXY_CORROSION_ENABLED=true
export EDGEPROXY_CORROSION_API_URL=http://corrosion:8080
export EDGEPROXY_CORROSION_POLL_SECS=5

# Note: EDGEPROXY_DB_PATH is IGNORED when Corrosion is enabled
./edgeproxy
```

### Corrosion Agent Configuration

The Corrosion agent runs as a sidecar and manages its own replicated database:

```toml
# corrosion.toml (Corrosion agent config, NOT edgeProxy)
[db]
path = "/var/lib/corrosion/state.db"  # Corrosion's internal state

[cluster]
name = "edgeproxy"
bootstrap = ["10.50.0.1:4001", "10.50.5.1:4001"]

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8080"  # edgeProxy connects here
```

### Benefits

- **Real-time sync**: Changes propagate in ~100ms via gossip protocol
- **No manual intervention**: Automatic replication across all POPs
- **Partition tolerance**: Works during network splits (CRDT-based)
- **Single source of truth**: Register backend once, available everywhere

---

## Next Steps

- [Docker Deployment](./deployment/docker) - Container configuration
- [Fly.io Deployment](./deployment/flyio) - Global edge deployment
- [Load Balancer Internals](./internals/load-balancer) - Scoring details
