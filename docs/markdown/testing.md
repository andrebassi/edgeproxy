---
sidebar_position: 12
---

# Testing

This guide covers how to test edgeProxy locally and in deployment environments using the mock backend server.

## Mock Backend Server

The `tests/mock-backend/` directory contains a lightweight Go HTTP server that simulates real backend services for testing purposes.

### Features

- **Multi-region simulation**: Configure different regions per instance
- **Request tracking**: Counts requests per backend
- **Multiple endpoints**: Root, health, info, and latency endpoints
- **JSON responses**: Structured responses for easy parsing
- **Minimal footprint**: ~8MB binary, low memory usage

### Building the Mock Server

```bash
# Native build (for local testing)
cd tests/mock-backend
go build -o mock-backend main.go

# Cross-compile for Linux AMD64 (for EC2/cloud deployment)
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go
```

### Running Locally

Start multiple instances to simulate different backends:

```bash
# Terminal 1: EU backend 1
./mock-backend -port 9001 -region eu -id mock-eu-1

# Terminal 2: EU backend 2
./mock-backend -port 9002 -region eu -id mock-eu-2

# Terminal 3: US backend
./mock-backend -port 9003 -region us -id mock-us-1
```

### CLI Options

| Flag | Default | Description |
|------|---------|-------------|
| `-port` | `9001` | TCP port to listen on |
| `-region` | `eu` | Region identifier (eu, us, sa, ap) |
| `-id` | `mock-{region}-{port}` | Unique backend identifier |

### Endpoints

| Endpoint | Description | Response |
|----------|-------------|----------|
| `/` | Root | Text with backend info |
| `/health` | Health check | `OK - {id} ({region})` |
| `/api/info` | JSON info | Full backend details |
| `/api/latency` | Minimal JSON | For latency testing |

### Example Response (`/api/info`)

```json
{
  "backend_id": "mock-eu-1",
  "region": "eu",
  "hostname": "ip-172-31-29-183",
  "port": "9001",
  "request_count": 42,
  "uptime_secs": 3600,
  "timestamp": "2025-12-08T00:11:43Z",
  "message": "Hello from mock backend!"
}
```

## Local Testing Setup

### 1. Configure routing.db

Add mock backends to your local routing.db:

```sql
-- Clear existing test backends
DELETE FROM backends WHERE id LIKE 'mock-%';

-- Add mock backends
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
```

### 2. Start Mock Backends

```bash
# Start all 3 backends
./tests/mock-backend/mock-backend -port 9001 -region eu -id mock-eu-1 &
./tests/mock-backend/mock-backend -port 9002 -region eu -id mock-eu-2 &
./tests/mock-backend/mock-backend -port 9003 -region us -id mock-us-1 &
```

### 3. Run edgeProxy

```bash
EDGEPROXY_REGION=eu \
EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080 \
cargo run --release
```

### 4. Test Requests

```bash
# Simple test
curl http://localhost:8080/api/info

# Multiple requests (observe load balancing)
for i in {1..10}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done

# Health check
curl http://localhost:8080/health
```

## EC2 Deployment Testing

### 1. Deploy Mock Server to EC2

```bash
# Build for Linux
cd tests/mock-backend
GOOS=linux GOARCH=amd64 go build -o mock-backend-linux-amd64 main.go

# Copy to EC2
scp -i ~/.ssh/edgeproxy-key.pem mock-backend-linux-amd64 ubuntu@<EC2-IP>:/tmp/

# SSH and setup
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@<EC2-IP>
sudo mv /tmp/mock-backend-linux-amd64 /opt/edgeproxy/mock-backend
sudo chmod +x /opt/edgeproxy/mock-backend
```

### 2. Start Mock Backends on EC2

```bash
# Start 3 instances
cd /opt/edgeproxy
nohup ./mock-backend -port 9001 -region eu -id mock-eu-1 > /tmp/mock-9001.log 2>&1 &
nohup ./mock-backend -port 9002 -region eu -id mock-eu-2 > /tmp/mock-9002.log 2>&1 &
nohup ./mock-backend -port 9003 -region us -id mock-us-1 > /tmp/mock-9003.log 2>&1 &

# Verify
ps aux | grep mock-backend
curl localhost:9001/health
curl localhost:9002/health
curl localhost:9003/health
```

### 3. Configure routing.db on EC2

```bash
sqlite3 /opt/edgeproxy/routing.db "
DELETE FROM backends WHERE id LIKE 'mock-%';
INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES
  ('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150),
  ('mock-eu-2', 'test', 'eu', '127.0.0.1', 9002, 1, 2, 100, 150),
  ('mock-us-1', 'test', 'us', '127.0.0.1', 9003, 1, 2, 100, 150);
SELECT id, region, port, healthy FROM backends WHERE deleted=0;
"
```

#### Backend Fields Explained

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `id` | TEXT | Unique identifier for the backend. Used in logs and client affinity. | `mock-eu-1` |
| `app` | TEXT | Application name. Groups backends serving the same app. | `test` |
| `region` | TEXT | Geographic region code. Used for geo-routing decisions. Valid: `eu`, `us`, `sa`, `ap`. | `eu` |
| `wg_ip` | TEXT | Backend IP address. Use `127.0.0.1` for local testing, WireGuard IPs (10.50.x.x) in production. | `127.0.0.1` |
| `port` | INTEGER | TCP port the backend listens on. | `9001` |
| `healthy` | INTEGER | Health status. `1` = healthy (receives traffic), `0` = unhealthy (excluded from routing). | `1` |
| `weight` | INTEGER | Relative weight for load balancing. Higher weight = more traffic. Range: 1-10. | `2` |
| `soft_limit` | INTEGER | Comfortable connection count. Above this, the backend is considered "loaded" and less preferred. | `100` |
| `hard_limit` | INTEGER | Maximum connections. At or above this limit, backend is excluded from new connections. | `150` |

#### Example Data Breakdown

```sql
('mock-eu-1', 'test', 'eu', '127.0.0.1', 9001, 1, 2, 100, 150)
```

| Value | Field | Meaning |
|-------|-------|---------|
| `mock-eu-1` | id | Backend identifier, first EU mock server |
| `test` | app | Application name for testing |
| `eu` | region | Located in Europe region |
| `127.0.0.1` | wg_ip | Localhost (same machine as proxy) |
| `9001` | port | Listening on port 9001 |
| `1` | healthy | Backend is healthy and active |
| `2` | weight | Medium priority (scale 1-10) |
| `100` | soft_limit | Comfortable with up to 100 connections |
| `150` | hard_limit | Maximum 150 connections allowed |

#### Load Balancer Scoring

The proxy uses these fields to calculate a score for each backend:

```
score = geo_score * 100 + (connections / soft_limit) / weight
```

- **geo_score**: 0 (same country), 1 (same region), 2 (local POP region), 3 (global fallback)
- **connections**: Current active connections (from metrics)
- **soft_limit**: Divides load factor
- **weight**: Higher weight reduces the score (more preferred)

**Lowest score wins.** Backends with `healthy=0` or at `hard_limit` are excluded.

### 4. Test from External Client

```bash
# From your local machine
curl http://<EC2-PUBLIC-IP>:8080/api/info
curl http://<EC2-PUBLIC-IP>:8080/health

# Multiple requests to see load balancing
for i in {1..5}; do
  curl -s http://<EC2-PUBLIC-IP>:8080/api/info
  echo ""
done
```

## Testing Scenarios

### Client Affinity

Client affinity (sticky sessions) binds clients to the same backend:

```bash
# All requests from same IP go to same backend
for i in {1..5}; do
  curl -s http://localhost:8080/api/info | grep backend_id
done
# Expected: All show the same backend_id
```

### Load Distribution

To test load distribution, simulate different clients:

```bash
# Use different source IPs or wait for TTL expiration
# Check request_count on each backend
curl localhost:9001/api/info | grep request_count
curl localhost:9002/api/info | grep request_count
curl localhost:9003/api/info | grep request_count
```

### Backend Health

Test health-based routing by stopping a backend:

```bash
# Stop mock-eu-1
pkill -f 'mock-backend.*9001'

# Requests should now go to healthy backends
curl http://localhost:8080/api/info
# Expected: Routes to mock-eu-2 or mock-us-1
```

### Geo-Routing

The proxy routes clients to backends in their region:

1. Configure backends in multiple regions
2. Test from different geographic locations
3. Observe routing decisions in proxy logs

## Monitoring During Tests

### edgeProxy Logs

```bash
# On EC2
sudo journalctl -u edgeproxy -f

# Look for:
# - Backend selection logs
# - Connection counts
# - GeoIP resolution
```

### Mock Backend Logs

```bash
# Check individual backend logs
tail -f /tmp/mock-9001.log
tail -f /tmp/mock-9002.log
tail -f /tmp/mock-9003.log
```

### Request Distribution

```bash
# Quick check of request distribution
echo "mock-eu-1: $(curl -s localhost:9001/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-eu-2: $(curl -s localhost:9002/api/info | grep -o '"request_count":[0-9]*')"
echo "mock-us-1: $(curl -s localhost:9003/api/info | grep -o '"request_count":[0-9]*')"
```

## Cleanup

### Local

```bash
# Kill all mock backends
pkill -f mock-backend
```

### EC2

```bash
# Kill mock backends
sudo pkill -f mock-backend

# Or kill by port
sudo fuser -k 9001/tcp 9002/tcp 9003/tcp
```

## Troubleshooting

### Mock Backend Won't Start

```bash
# Check if port is in use
sudo ss -tlnp | grep 9001

# Kill existing process
sudo fuser -k 9001/tcp
```

### Proxy Can't Connect to Backend

1. Verify backend is running: `curl localhost:9001/health`
2. Check routing.db configuration
3. Verify `wg_ip` matches (use `127.0.0.1` for local testing)
4. Check firewall rules on EC2

### Requests Timeout

1. Check edgeProxy is running: `sudo systemctl status edgeproxy`
2. Verify backend health in routing.db
3. Check connection limits aren't exceeded
