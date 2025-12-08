---
sidebar_position: 5
---

# Auto-Discovery API

The API allows backends to automatically register and deregister.

## Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/health` | Health check + version + backend count |
| POST | `/api/v1/register` | Register a new backend |
| POST | `/api/v1/heartbeat/:id` | Update backend heartbeat |
| GET | `/api/v1/backends` | List all registered backends |
| GET | `/api/v1/backends/:id` | Get specific backend details |
| DELETE | `/api/v1/backends/:id` | Deregister a backend |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_API_ENABLED` | `false` | Enable Auto-Discovery API |
| `EDGEPROXY_API_LISTEN_ADDR` | `0.0.0.0:8081` | API listen address |
| `EDGEPROXY_HEARTBEAT_TTL_SECS` | `60` | Backend heartbeat TTL |

## Registration Example

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

## Registration Payload

```json
{
  "id": "backend-eu-1",
  "app": "myapp",
  "region": "eu",
  "country": "DE",
  "ip": "10.50.1.1",
  "port": 8080,
  "weight": 2,
  "soft_limit": 100,
  "hard_limit": 150
}
```

| Field | Required | Default | Description |
|-------|----------|---------|-------------|
| `id` | Yes | - | Unique backend identifier |
| `app` | Yes | - | Application name |
| `region` | Yes | - | Region code (sa, us, eu, ap) |
| `country` | No | derived | Country code (ISO 3166-1) |
| `ip` | Yes | - | Backend IP address |
| `port` | Yes | - | Backend port |
| `weight` | No | 2 | Load balancing weight |
| `soft_limit` | No | 100 | Soft connection limit |
| `hard_limit` | No | 150 | Hard connection limit |

## Health Check Response

```bash
curl http://localhost:8081/health
```

```json
{
  "status": "ok",
  "version": "0.2.0",
  "backends": 5,
  "uptime_secs": 3600
}
```

## Benefits

- **Zero configuration**: Backends just start and register
- **Automatic scaling**: New instances appear automatically
- **Graceful shutdown**: Clean deregistration
- **TTL-based health**: Unhealthy = expired = deregistered
