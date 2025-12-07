---
sidebar_position: 1
---

# Docker Deployment

This guide covers deploying edgeProxy using Docker and Docker Compose for both development and production environments.

## Quick Start

```bash
# Build and start multi-region environment
task docker-build
task docker-up

# Run tests
task docker-test

# View logs
task docker-logs

# Stop environment
task docker-down
```

## Dockerfile

The production Dockerfile uses multi-stage builds for minimal image size:

```dockerfile
# Build stage
FROM rust:1.83-alpine AS builder
RUN apk add --no-cache musl-dev sqlite-dev
WORKDIR /app

# Cache dependencies
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release && rm -rf src

# Build application
COPY src ./src
RUN touch src/main.rs && cargo build --release

# Runtime stage
FROM alpine:3.19
RUN apk add --no-cache sqlite sqlite-libs ca-certificates
WORKDIR /app

COPY --from=builder /app/target/release/edge-proxy /app/edge-proxy
COPY sql ./sql
RUN sqlite3 /app/routing.db < /app/sql/create_routing_db.sql

EXPOSE 8080
CMD ["/app/edge-proxy"]
```

### Image Size

| Stage | Size |
|-------|------|
| Builder | ~2.5 GB |
| Runtime | ~25 MB |

## Docker Compose

### Development Environment

Full multi-region simulation with 3 POPs and 9 backends:

```yaml
version: '3.8'

services:
  # South America Backends
  sa-node-1:
    build:
      context: .
      dockerfile: Dockerfile.backend
    environment:
      - BACKEND_PORT=8080
      - BACKEND_ID=sa-node-1
      - BACKEND_REGION=sa
    networks:
      edgenet:
        ipv4_address: 10.10.1.1

  sa-node-2:
    build:
      context: .
      dockerfile: Dockerfile.backend
    environment:
      - BACKEND_PORT=8080
      - BACKEND_ID=sa-node-2
      - BACKEND_REGION=sa
    networks:
      edgenet:
        ipv4_address: 10.10.1.2

  sa-node-3:
    build:
      context: .
      dockerfile: Dockerfile.backend
    environment:
      - BACKEND_PORT=8080
      - BACKEND_ID=sa-node-3
      - BACKEND_REGION=sa
    networks:
      edgenet:
        ipv4_address: 10.10.1.3

  # US Backends (similar pattern)
  us-node-1:
    # ...
    networks:
      edgenet:
        ipv4_address: 10.10.2.1

  # EU Backends (similar pattern)
  eu-node-1:
    # ...
    networks:
      edgenet:
        ipv4_address: 10.10.3.1

  # edgeProxy POPs
  pop-sa:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8080:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.10

  pop-us:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=us
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8081:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.11

  pop-eu:
    build:
      context: .
      dockerfile: Dockerfile
    environment:
      - EDGEPROXY_REGION=eu
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/app/routing.db
      - EDGEPROXY_BINDING_TTL_SECS=30
      - DEBUG=1
    ports:
      - "8082:8080"
    volumes:
      - ./docker/routing-docker.db:/app/routing.db:ro
    networks:
      edgenet:
        ipv4_address: 10.10.0.12

networks:
  edgenet:
    driver: bridge
    ipam:
      config:
        - subnet: 10.10.0.0/16
```

### Network Layout

```
10.10.0.0/16 - Edge Network
├── 10.10.0.10 - POP SA (localhost:8080)
├── 10.10.0.11 - POP US (localhost:8081)
├── 10.10.0.12 - POP EU (localhost:8082)
├── 10.10.1.0/24 - SA Backends
│   ├── 10.10.1.1 - sa-node-1
│   ├── 10.10.1.2 - sa-node-2
│   └── 10.10.1.3 - sa-node-3
├── 10.10.2.0/24 - US Backends
│   ├── 10.10.2.1 - us-node-1
│   └── ...
└── 10.10.3.0/24 - EU Backends
    ├── 10.10.3.1 - eu-node-1
    └── ...
```

## Mock Backend

For testing, a Python mock backend echoes data with identification:

```python
#!/usr/bin/env python3
import socket
import threading

def handle_client(conn, addr, backend_id, region):
    # Send identity on connect
    welcome = f"Backend: {backend_id} | Region: {region}\n"
    conn.send(welcome.encode())

    # Echo loop
    while True:
        data = conn.recv(1024)
        if not data:
            break
        response = f"[{backend_id}] Echo: {data.decode().strip()}\n"
        conn.send(response.encode())

    conn.close()

def start_backend(host, port, backend_id, region):
    server = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    server.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
    server.bind((host, port))
    server.listen(5)

    while True:
        conn, addr = server.accept()
        thread = threading.Thread(
            target=handle_client,
            args=(conn, addr, backend_id, region)
        )
        thread.daemon = True
        thread.start()
```

### Backend Dockerfile

```dockerfile
FROM python:3.12-alpine
WORKDIR /app
COPY tests/mock_backend.py /app/mock_backend.py

CMD ["python", "/app/mock_backend.py", \
     "${BACKEND_PORT}", "${BACKEND_ID}", "${BACKEND_REGION}"]
```

## Testing

### Docker Test Suite

```bash
#!/bin/bash
# tests/test_docker.sh

# Test 1: POP Connectivity
for pop in "pop-sa:10.10.0.10" "pop-us:10.10.0.11" "pop-eu:10.10.0.12"; do
    ip=$(echo $pop | cut -d: -f2)
    nc -z -w 2 $ip 8080 && echo "✓ $pop reachable"
done

# Test 2: Regional Routing
for pop in "SA:10.10.0.10" "US:10.10.0.11" "EU:10.10.0.12"; do
    region=$(echo $pop | cut -d: -f1)
    ip=$(echo $pop | cut -d: -f2)
    response=$(printf '\n' | nc -w 2 $ip 8080 | head -1)
    echo "POP $region -> $response"
done

# Test 3: Client Affinity
for i in 1 2 3 4 5; do
    response=$(printf '\n' | nc -w 1 10.10.0.10 8080 | head -1)
    echo "Connection $i: $response"
done

# Test 4: Data Transfer
echo "Hello World" | nc -w 2 10.10.0.10 8080
```

### Running Tests

```bash
# Full test suite
task docker-test

# Manual testing
docker compose exec pop-sa /bin/sh
nc 10.10.1.1 8080  # Direct backend
nc 10.10.0.10 8080 # Through proxy
```

## Production Deployment

### Single POP

```yaml
# docker-compose.prod.yml
version: '3.8'

services:
  edgeproxy:
    image: edgeproxy:latest
    restart: unless-stopped
    environment:
      - EDGEPROXY_REGION=sa
      - EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
      - EDGEPROXY_DB_PATH=/data/routing.db
      - EDGEPROXY_GEOIP_PATH=/data/GeoLite2-Country.mmdb
      - EDGEPROXY_BINDING_TTL_SECS=600
    ports:
      - "8080:8080"
    volumes:
      - ./data:/data:ro
    healthcheck:
      test: ["CMD", "nc", "-z", "localhost", "8080"]
      interval: 30s
      timeout: 5s
      retries: 3
    deploy:
      resources:
        limits:
          cpus: '2'
          memory: 512M
```

### With Traefik

```yaml
services:
  edgeproxy:
    image: edgeproxy:latest
    labels:
      - "traefik.enable=true"
      - "traefik.tcp.routers.edgeproxy.rule=HostSNI(`*`)"
      - "traefik.tcp.routers.edgeproxy.entrypoints=tcp"
      - "traefik.tcp.services.edgeproxy.loadbalancer.server.port=8080"
    networks:
      - traefik-net
```

## Monitoring

### Health Check

```bash
# Simple TCP check
nc -z localhost 8080 && echo "healthy" || echo "unhealthy"

# With curl (if HTTP endpoint added)
curl -f http://localhost:8080/health
```

### Logs

```bash
# Follow all logs
docker compose logs -f

# Specific service
docker compose logs -f pop-sa

# Filter by level
docker compose logs pop-sa 2>&1 | grep -E "(INFO|ERROR)"
```

### Metrics

edgeProxy logs connection metrics:

```
DEBUG edge_proxy::proxy: proxying 10.10.0.100 -> sa-node-1 (10.10.1.1:8080)
INFO edge_proxy::db: routing reload ok, version=5 backends=9
```

For Prometheus integration, see [Fly.io Deployment](./flyio).

## Troubleshooting

### Container Won't Start

```bash
# Check logs
docker compose logs edgeproxy

# Verify routing.db exists
docker compose exec edgeproxy ls -la /app/routing.db

# Test database
docker compose exec edgeproxy sqlite3 /app/routing.db "SELECT * FROM backends"
```

### Connection Refused

```bash
# Verify port mapping
docker compose ps

# Check internal connectivity
docker compose exec edgeproxy nc -z localhost 8080

# Test backend reachability
docker compose exec pop-sa nc -z 10.10.1.1 8080
```

### No Response from Proxy

```bash
# Enable debug logging
docker compose down
DEBUG=1 docker compose up

# Check backend health
docker compose exec pop-sa sqlite3 /app/routing.db "SELECT id, healthy FROM backends"
```

## Next Steps

- [Fly.io Deployment](./flyio) - Global edge deployment
- [Configuration](../configuration) - Environment variables
- [Architecture](../architecture) - System design
