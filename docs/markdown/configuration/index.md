---
sidebar_position: 1
---

# Configuration

edgeProxy is configured entirely through environment variables. This section covers all available options with examples.

## Documentation Sections

| Section | Description |
|---------|-------------|
| [Environment Variables](./environment-variables) | Core settings, TLS, DNS, API |
| [Database Schema](./database-schema) | Backend routing table structure |
| [Internal DNS](./dns-server) | Geo-aware `.internal` domain resolution |
| [Auto-Discovery API](./auto-discovery-api) | Dynamic backend registration |
| [Distributed Control Plane](./corrosion) | Corrosion distributed SQLite |
| [Infrastructure Components](./infrastructure) | Rate limiting, circuit breaker, metrics |

## Quick Start

### Development

```bash
export EDGEPROXY_LISTEN_ADDR="127.0.0.1:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="./routing.db"
export DEBUG="1"

./target/release/edge-proxy
```

### Production

```bash
export EDGEPROXY_LISTEN_ADDR="0.0.0.0:8080"
export EDGEPROXY_REGION="sa"
export EDGEPROXY_DB_PATH="/data/routing.db"
export EDGEPROXY_BINDING_TTL_SECS="600"

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
    ports:
      - "8080:8080"
    volumes:
      - ./routing.db:/app/routing.db:ro
```
