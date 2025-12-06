---
sidebar_position: 4
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

### GeoIP

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_GEOIP_PATH` | *(none)* | Path to GeoLite2-Country.mmdb |

### Debugging

| Variable | Default | Description |
|----------|---------|-------------|
| `DEBUG` | *(unset)* | Enable debug logging when set |

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
export EDGEPROXY_GEOIP_PATH="/data/GeoLite2-Country.mmdb"
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

## GeoIP Setup

edgeProxy uses MaxMind GeoLite2 for IP geolocation.

### 1. Register for Free Account

Visit [MaxMind GeoLite2](https://dev.maxmind.com/geoip/geolite2-free-geolocation-data) and create a free account.

### 2. Download Database

```bash
# Download GeoLite2-Country database
wget "https://download.maxmind.com/app/geoip_download?edition_id=GeoLite2-Country&suffix=tar.gz&license_key=YOUR_KEY" -O GeoLite2-Country.tar.gz

# Extract
tar -xzf GeoLite2-Country.tar.gz
mv GeoLite2-Country_*/GeoLite2-Country.mmdb /data/
```

### 3. Configure edgeProxy

```bash
export EDGEPROXY_GEOIP_PATH="/data/GeoLite2-Country.mmdb"
```

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

## Next Steps

- [Docker Deployment](./deployment/docker) - Container configuration
- [Kubernetes Deployment](./deployment/kubernetes) - K8s manifests
- [Load Balancer Internals](./internals/load-balancer) - Scoring details
