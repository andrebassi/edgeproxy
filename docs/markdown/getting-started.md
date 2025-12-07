---
sidebar_position: 2
---

# Getting Started

This guide covers installation, building from source, and running edgeProxy locally.

## Prerequisites

### Required

- **Rust 1.75+** - [Install Rust](https://rustup.rs/)
- **SQLite 3.x** - Usually pre-installed on macOS/Linux
- **Task** - [Install Task](https://taskfile.dev/installation/)

### Optional

- **Docker & Docker Compose** - For containerized deployment

:::info GeoIP Included
The MaxMind GeoLite2 database is **embedded in the binary** - no external download required.
:::

## Installation

### From Source

```bash
# Clone repository
git clone https://github.com/andrebassi/edgeproxy.git
cd edgeproxy

# Build release binary
task build

# Verify installation
./target/release/edge-proxy --help
```

### Using Docker

```bash
# Build Docker image
task docker-build

# Start multi-region environment
task docker-up
```

## Project Structure

```
edgeproxy/
├── Cargo.toml              # Rust dependencies
├── Taskfile.yaml           # Task automation
├── routing.db              # SQLite routing database
├── sql/
│   └── create_routing_db.sql   # Schema + seed data
├── src/
│   ├── main.rs             # Entry point
│   ├── config.rs           # Configuration loading
│   ├── model.rs            # Data structures
│   ├── db.rs               # SQLite sync
│   ├── lb.rs               # Load balancer
│   ├── state.rs            # Shared state + GeoIP
│   └── proxy.rs            # TCP proxy logic
├── docker/
│   ├── init-routing.sql    # Docker routing config
│   └── routing-docker.db   # Pre-built Docker DB
├── tests/
│   ├── mock_backend.py     # Test backend server
│   └── test_docker.sh      # Docker test suite
└── docs/                   # Docusaurus documentation
```

## First Run

### 1. Initialize Routing Database

```bash
# Create database with sample backends
task db-init
```

This creates `routing.db` with the following schema:

```sql
CREATE TABLE backends (
    id TEXT PRIMARY KEY,
    app TEXT,
    region TEXT,          -- "sa", "us", "eu"
    wg_ip TEXT,           -- Backend IP (WireGuard)
    port INTEGER,
    healthy INTEGER,      -- 0 or 1
    weight INTEGER,       -- Relative weight
    soft_limit INTEGER,   -- Comfortable connections
    hard_limit INTEGER,   -- Maximum connections
    deleted INTEGER DEFAULT 0
);
```

### 2. Start edgeProxy

```bash
# Run with default configuration (region=sa, port=8080)
task run

# Or with custom settings
EDGEPROXY_REGION=us EDGEPROXY_LISTEN_ADDR=0.0.0.0:9000 task run
```

### 3. Test Connection

```bash
# Connect to proxy
echo "Hello" | nc localhost 8080

# Expected output (if backend is running):
# Backend: sa-node-1 | Region: sa | Your IP: 127.0.0.1:xxxxx
# [sa-node-1] Echo: Hello
```

## Running Tests

### Unit Tests

```bash
task test
```

### Local Multi-Region Simulation

```bash
# Terminal 1: Start mock backends
task local-env

# Terminal 2: Start proxy
task run-sa

# Terminal 3: Run tests
task local-test
```

### Docker Tests

```bash
# Full Docker test suite
task docker-build
task docker-up
task docker-test

# Cleanup
task docker-down
```

## Available Tasks

| Task | Description |
|------|-------------|
| `task build` | Build release binary |
| `task run` | Run with default config |
| `task run-sa` | Run as SA POP |
| `task run-us` | Run as US POP |
| `task run-eu` | Run as EU POP |
| `task test` | Run unit tests |
| `task db-init` | Initialize routing.db |
| `task docker-build` | Build Docker images |
| `task docker-up` | Start Docker environment |
| `task docker-down` | Stop Docker environment |
| `task docker-test` | Run Docker test suite |
| `task docker-logs` | View container logs |
| `task docs-dev` | Start documentation server |

## Next Steps

- [Architecture](./architecture) - Understand how edgeProxy works
- [Configuration](./configuration) - All configuration options
- [Docker Deployment](./deployment/docker) - Production deployment
