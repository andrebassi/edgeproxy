# edgeProxy

**Distributed TCP Proxy for Geo-Aware Load Balancing**

[![Website](https://img.shields.io/badge/website-edgeproxy.io-orange)](https://edgeproxy.io)
[![Documentation](https://img.shields.io/badge/docs-docs.edgeproxy.io-blue)](https://docs.edgeproxy.io)
[![Release](https://img.shields.io/github/v/release/andrebassi/edgeproxy)](https://github.com/andrebassi/edgeproxy/releases)
[![License](https://img.shields.io/badge/license-MIT-blue)](LICENSE)

**Website:** https://edgeproxy.io | **Documentation:** https://docs.edgeproxy.io

## What is edgeProxy?

edgeProxy is a **distributed TCP proxy** written in Rust, designed to operate at edge Points of Presence (POPs) worldwide. It routes client connections to the optimal backend based on geographic proximity, backend health, current load, and capacity limits.

## Why edgeProxy?

Built with the same patterns used by production edge platforms like Fly.io:

- **WireGuard Backhaul**: All internal communication flows over encrypted WireGuard tunnels
- **Rust + Tokio**: Predictable latency without GC pauses
- **Geo-Aware Routing**: Clients are routed to the nearest healthy backend
- **Client Affinity**: Session stickiness with configurable TTL

## Features

- **Geo-Aware Load Balancing**: Route clients to the nearest region using MaxMind GeoIP
- **Client Affinity**: Session stickiness with configurable TTL (default 10 minutes)
- **Weighted Load Balancing**: Configure backend weights for traffic distribution
- **Hot Reload**: Update routing.db without restarts
- **Connection Limits**: Soft and hard limits per backend
- **WireGuard Overlay**: Secure communication between POPs

## Quick Start

### Prerequisites

- Rust 1.75+
- SQLite 3.x
- WireGuard (for production multi-POP)

### Installation

```bash
# Clone the repository
git clone https://github.com/andrebassi/edgeproxy.git
cd edgeproxy

# Build
cargo build --release

# Run
./target/release/edge-proxy
```

### Docker

```bash
# Build and start multi-region environment
task docker-build
task docker-up

# Run tests
task docker-test

# View logs
task docker-logs
```

### Fly.io (Recommended for Production)

```bash
# Deploy to Fly.io
fly launch
fly deploy
```

See [Fly.io Deployment Guide](https://docs.edgeproxy.io/deployment/flyio) for details.

## Configuration

All configuration via environment variables:

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_LISTEN_ADDR` | `0.0.0.0:8080` | TCP listen address |
| `EDGEPROXY_DB_PATH` | `routing.db` | Path to SQLite routing database |
| `EDGEPROXY_REGION` | `sa` | Local POP region identifier |
| `EDGEPROXY_DB_RELOAD_SECS` | `5` | Interval to reload routing.db |
| `EDGEPROXY_BINDING_TTL_SECS` | `600` | Client binding TTL (10 min) |

### Example

```bash
EDGEPROXY_REGION=us \
EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080 \
./target/release/edge-proxy
```

## Architecture

```
                    ┌──────────────────────────────────────────────────┐
                    │           WireGuard Overlay (10.50.x.x)          │
                    └──────────────────────────────────────────────────┘
                              │            │            │
┌────────┐    ┌───────────────┴──┐    ┌────┴────┐  ┌────┴────┐  ┌─────────┐
│ Client │───▶│   edgeProxy POP  │───▶│Backend  │  │Backend  │  │Backend  │
│ (geo)  │    │   (sa/us/eu)     │    │   SA    │  │   US    │  │   EU    │
└────────┘    └──────────────────┘    │10.50.1.x│  │10.50.2.x│  │10.50.3.x│
                      │               └─────────┘  └─────────┘  └─────────┘
               ┌──────┴──────┐
               │ routing.db  │ ◄── Replicated via Corrosion (v2)
               └─────────────┘
```

### Load Balancer Algorithm

Scoring system (lower = better):

```
score = region_score * 100 + (load_factor / weight)

where:
  region_score = 0 (client region match)
               = 1 (local POP region)
               = 2 (fallback/other)

  load_factor = current_connections / soft_limit
  weight = configured backend weight (higher = preferred)
```

### Request Flow

1. Client TCP connection arrives at edgeProxy
2. Check for existing binding (affinity)
3. If no binding: resolve client region via MaxMind GeoIP
4. Score all healthy backends within capacity
5. Select lowest-score backend
6. Create binding, connect via WireGuard overlay
7. Bidirectional TCP copy (L4 passthrough)

## Routing Database

The `routing.db` SQLite database contains backend configuration:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,      -- "sa-node-1"
    app TEXT,                 -- "myapp"
    region TEXT,              -- "sa", "us", "eu"
    wg_ip TEXT,               -- WireGuard IP
    port INTEGER,             -- Backend port
    healthy INTEGER,          -- 0 or 1
    weight INTEGER,           -- Load balancing weight
    soft_limit INTEGER,       -- Comfortable connection count
    hard_limit INTEGER,       -- Maximum connections
    deleted INTEGER DEFAULT 0
);
```

### Example Data

```sql
INSERT INTO backends VALUES
    ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 8080, 1, 2, 50, 100, 0),
    ('us-node-1', 'myapp', 'us', '10.50.2.1', 8080, 1, 2, 50, 100, 0),
    ('eu-node-1', 'myapp', 'eu', '10.50.3.1', 8080, 1, 2, 50, 100, 0);
```

## Supported Regions

| Code | Region |
|------|--------|
| `sa` | South America (Brazil, Argentina, Chile, etc.) |
| `us` | North America (USA, Canada, Mexico) |
| `eu` | Europe (Germany, France, UK, etc.) |
| `ap` | Asia Pacific (Japan, Singapore, Australia) |

## Project Structure

```
edgeproxy/
├── src/
│   ├── main.rs         # Entry point
│   ├── config.rs       # Environment configuration
│   ├── model.rs        # Data structures
│   ├── db.rs           # SQLite sync loop
│   ├── lb.rs           # Load balancer algorithm
│   ├── state.rs        # Shared state + GeoIP
│   └── proxy.rs        # TCP proxy
├── sql/
│   └── create_routing_db.sql
├── docs/               # Docusaurus documentation
├── docker/             # Docker configurations
├── fly-backend/        # Mock backend for Fly.io
├── Cargo.toml
├── Taskfile.yaml
└── README.md
```

## Performance

| Metric | Value |
|--------|-------|
| Cold Start | ~50ms |
| Connection Latency | <1ms overhead |
| Memory per 1K connections | ~10MB |
| Binary Size | ~5MB |

## Roadmap

edgeProxy is evolving towards a fully distributed, self-healing edge platform:

| Phase | Description | Status |
|-------|-------------|--------|
| Phase 1 | Internal DNS (.internal domains) | Planned |
| Phase 2 | Corrosion (distributed SQLite) | Planned |
| Phase 3 | Auto-Discovery | Planned |
| Phase 4 | IPv6 (6PN) | Planned |
| Phase 5 | Anycast BGP | Planned |
| Phase 6 | Active Health Checks | Planned |

See the full [Roadmap](https://docs.edgeproxy.io/roadmap) for details.

## Development

### Build

```bash
# Debug build
cargo build

# Release build
cargo build --release

# Run tests
cargo test
```

### Docker Development

```bash
# Build images
task docker-build

# Start environment (3 POPs, 9 backends)
task docker-up

# Run tests
task docker-test

# Cleanup
task docker-down
```

## Documentation

Full documentation is available at [docs.edgeproxy.io](https://docs.edgeproxy.io)

- [Getting Started](https://docs.edgeproxy.io/getting-started)
- [Architecture](https://docs.edgeproxy.io/architecture)
- [Configuration](https://docs.edgeproxy.io/configuration)
- [Deployment](https://docs.edgeproxy.io/deployment/docker)
- [Internals](https://docs.edgeproxy.io/internals/load-balancer)
- [Roadmap](https://docs.edgeproxy.io/roadmap)

## Related Technologies

- **WireGuard**: Secure overlay network between POPs
- **Corrosion**: SQLite replication for distributed routing.db (planned)
- **MaxMind GeoLite2**: IP geolocation (embedded in binary)
- **Fly.io**: Recommended deployment platform

## Troubleshooting

### Connection Timeout

```bash
# Check backend health
sqlite3 routing.db "SELECT id, healthy FROM backends"

# Check WireGuard connectivity
ping 10.50.1.1
```

### No Backends Available

```bash
# Verify routing.db
sqlite3 routing.db "SELECT * FROM backends WHERE healthy=1"
```

## License

[MIT](LICENSE)

## Author

Developed by [Andre Bassi](https://andrebassi.com.br)
