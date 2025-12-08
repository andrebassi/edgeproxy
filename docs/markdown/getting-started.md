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
task build:release

# Verify installation
./target/release/edge-proxy --help
```

### Using Docker

```bash
# Build Docker image
task docker:build

# Start multi-region environment
task docker:up
```

## Project Structure

edgeProxy uses **Hexagonal Architecture** (Ports & Adapters):

![Project Structure](/img/project-structure.svg)

## First Run

### 1. Initialize Routing Database

```bash
# Create database with sample backends
task db:init
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
task run:dev

# Or with custom settings
EDGEPROXY_REGION=us EDGEPROXY_LISTEN_ADDR=0.0.0.0:9000 task run:dev
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
# Run all tests (485 tests)
task test:all

# Run with coverage
task test:coverage
```

### Local Multi-Region Simulation

```bash
# Terminal 1: Start mock backends
task local:env

# Terminal 2: Start proxy
task run:sa

# Terminal 3: Run tests
task local:test
```

### Docker Tests

```bash
# Full Docker test suite
task docker:build
task docker:up
task docker:test

# Cleanup
task docker:down
```

## Available Tasks

Run `task --list` to see all available tasks. Main categories:

### Build

| Task | Description |
|------|-------------|
| `task build:release` | Build release binary |
| `task build:linux` | Cross-compile for Linux AMD64 |
| `task build:all` | Build for all platforms |

### Run

| Task | Description |
|------|-------------|
| `task run:dev` | Run with default config |
| `task run:sa` | Run as SA POP |
| `task run:us` | Run as US POP |
| `task run:eu` | Run as EU POP |

### Test

| Task | Description |
|------|-------------|
| `task test:all` | Run all unit tests |
| `task test:coverage` | Run with coverage report |

### Database

| Task | Description |
|------|-------------|
| `task db:init` | Initialize routing.db |
| `task db:reset` | Reset to initial state |

### Docker

| Task | Description |
|------|-------------|
| `task docker:build` | Build Docker images |
| `task docker:up` | Start Docker environment |
| `task docker:down` | Stop Docker environment |
| `task docker:test` | Run Docker test suite |

### Documentation

| Task | Description |
|------|-------------|
| `task docs:serve` | Build and serve docs (EN + PT-BR) |
| `task docs:dev` | Dev mode (EN only, hot reload) |

## Next Steps

- [Architecture](./architecture) - Understand how edgeProxy works
- [Configuration](./configuration) - All configuration options
- [Docker Deployment](./deployment/docker) - Production deployment
