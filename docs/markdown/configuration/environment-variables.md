---
sidebar_position: 2
---

# Environment Variables

All edgeProxy settings are configured via environment variables.

## Core Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_LISTEN_ADDR` | `0.0.0.0:8080` | TCP address to listen on |
| `EDGEPROXY_DB_PATH` | `routing.db` | Path to SQLite routing database |
| `EDGEPROXY_REGION` | `sa` | Local POP region identifier |

## Database Sync

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_DB_RELOAD_SECS` | `5` | Interval to reload routing.db (seconds) |

## Client Affinity

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Client binding TTL (10 minutes) |
| `EDGEPROXY_BINDING_GC_INTERVAL_SECS` | `60` | Garbage collection interval |

## Debugging

| Variable | Default | Description |
|----------|---------|-------------|
| `DEBUG` | *(unset)* | Enable debug logging when set |

## TLS Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_TLS_ENABLED` | `false` | Enable TLS server |
| `EDGEPROXY_TLS_LISTEN_ADDR` | `0.0.0.0:8443` | TLS listen address |
| `EDGEPROXY_TLS_CERT` | *(none)* | Path to TLS certificate (PEM) |
| `EDGEPROXY_TLS_KEY` | *(none)* | Path to TLS private key (PEM) |

## Internal DNS Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Enable DNS server |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | DNS listen address |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | DNS domain suffix |

## Auto-Discovery API Settings

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_API_ENABLED` | `false` | Enable Auto-Discovery API |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | API listen address |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | Backend heartbeat TTL |

## Corrosion Settings (Distributed SQLite)

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_CORROSION_ENABLED` | `false` | Enable Corrosion backend |
| `EDGEPROXY_CORROSION_API_URL` | `http://localhost:8080` | Corrosion HTTP API URL |
| `EDGEPROXY_CORROSION_POLL_SECS` | `5` | Polling interval for backend sync |

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
