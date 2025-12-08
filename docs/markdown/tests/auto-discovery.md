---
sidebar_position: 2
---

# Auto-Discovery API Tests

Test results for the Auto-Discovery API that allows backends to register themselves dynamically.

**Test Date**: 2025-12-08
**API Port**: 8081

## API Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/health` | GET | Health check with version and backend count |
| `/api/v1/register` | POST | Register a new backend |
| `/api/v1/heartbeat/:id` | POST | Update backend heartbeat |
| `/api/v1/backends` | GET | List all registered backends |
| `/api/v1/backends/:id` | GET | Get specific backend details |
| `/api/v1/backends/:id` | DELETE | Deregister a backend |

---

## Test Results

### 1. Health Check

**Request**:
```bash
curl -s http://34.246.117.138:8081/health
```

**Response**:
```json
{
  "status": "ok",
  "version": "0.2.0",
  "registered_backends": 10
}
```

**Status**: OK

---

### 2. Backend Registration

**Request**:
```bash
curl -s -X POST http://34.246.117.138:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "pop-gru",
    "app": "gru.pop",
    "region": "sa",
    "ip": "10.50.1.1",
    "port": 80
  }'
```

**Response**:
```json
{
  "id": "pop-gru",
  "registered": true,
  "message": "Backend registered successfully"
}
```

**All Registrations**:

| ID | App | Region | IP | Port | Result |
|----|-----|--------|-----|------|--------|
| pop-gru | gru.pop | sa | 10.50.1.1 | 80 | OK |
| pop-iad | iad.pop | us | 10.50.2.1 | 80 | OK |
| pop-ord | ord.pop | us | 10.50.2.2 | 80 | OK |
| pop-lax | lax.pop | us | 10.50.2.3 | 80 | OK |
| pop-lhr | lhr.pop | eu | 10.50.3.1 | 80 | OK |
| pop-fra | fra.pop | eu | 10.50.3.2 | 80 | OK |
| pop-cdg | cdg.pop | eu | 10.50.3.3 | 80 | OK |
| pop-nrt | nrt.pop | ap | 10.50.4.1 | 80 | OK |
| pop-sin | sin.pop | ap | 10.50.4.2 | 80 | OK |
| pop-syd | syd.pop | ap | 10.50.4.3 | 80 | OK |

**Status**: 10/10 registrations successful

---

### 3. List Backends

**Request**:
```bash
curl -s http://34.246.117.138:8081/api/v1/backends
```

**Response**:
```json
{
  "backends": [
    {
      "id": "pop-gru",
      "app": "gru.pop",
      "region": "sa",
      "ip": "10.50.1.1",
      "port": 80,
      "healthy": true,
      "last_heartbeat_secs": 5,
      "registered_secs": 120
    },
    {
      "id": "pop-iad",
      "app": "iad.pop",
      "region": "us",
      "ip": "10.50.2.1",
      "port": 80,
      "healthy": true,
      "last_heartbeat_secs": 5,
      "registered_secs": 118
    }
    // ... more backends
  ],
  "total": 10
}
```

**Status**: OK - All 10 backends listed

---

### 4. Get Single Backend

**Request**:
```bash
curl -s http://34.246.117.138:8081/api/v1/backends/pop-gru
```

**Response**:
```json
{
  "id": "pop-gru",
  "app": "gru.pop",
  "region": "sa",
  "ip": "10.50.1.1",
  "port": 80,
  "healthy": true,
  "last_heartbeat_secs": 10,
  "registered_secs": 125
}
```

**Status**: OK

---

### 5. Heartbeat Update

**Request**:
```bash
curl -s -X POST http://34.246.117.138:8081/api/v1/heartbeat/pop-gru
```

**Response**:
```json
{
  "id": "pop-gru",
  "status": "ok"
}
```

**Status**: OK - Heartbeat updated, `last_heartbeat_secs` reset to 0

---

### 6. Backend Deregistration

**Request**:
```bash
curl -s -X DELETE http://34.246.117.138:8081/api/v1/backends/test-backend
```

**Response**:
```json
{
  "deregistered": true,
  "id": "test-backend"
}
```

**Status**: OK

---

## Registration Payload Schema

### Required Fields

| Field | Type | Description | Example |
|-------|------|-------------|---------|
| `id` | string | Unique backend identifier | `"pop-gru"` |
| `app` | string | Application name (used for DNS) | `"gru.pop"` |
| `region` | string | Region code (sa, us, eu, ap) | `"sa"` |
| `ip` | string | Backend IP address | `"10.50.1.1"` |
| `port` | number | Backend port | `80` |

### Optional Fields (with defaults)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `country` | string | derived | ISO country code |
| `weight` | number | `2` | Load balancing weight (1-10) |
| `soft_limit` | number | `100` | Comfortable connection count |
| `hard_limit` | number | `150` | Maximum connections |

### Example Full Payload

```json
{
  "id": "my-backend-1",
  "app": "myapp",
  "region": "eu",
  "country": "DE",
  "ip": "10.50.3.1",
  "port": 8080,
  "weight": 5,
  "soft_limit": 200,
  "hard_limit": 300
}
```

---

## Health Management

### Heartbeat TTL

Backends are marked as unhealthy if they don't send a heartbeat within the TTL period.

| Setting | Default | Environment Variable |
|---------|---------|---------------------|
| Heartbeat TTL | 60 seconds | `EDGEPROXY_HEARTBEAT_TTL_SECS` |

### Automatic Cleanup

- Backends that miss heartbeat are marked `healthy: false`
- Background task removes stale backends periodically
- Unhealthy backends are excluded from load balancing

### Keeping Backends Healthy

```bash
# Simple heartbeat loop (every 30 seconds)
while true; do
  curl -s -X POST http://hub:8081/api/v1/heartbeat/my-backend-1
  sleep 30
done
```

---

## Integration with DNS

Backends registered via API can be resolved through DNS:

```bash
# Register backend with app name
curl -X POST http://hub:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"id":"my-eu-1","app":"myservice","region":"eu","ip":"10.50.3.1","port":8080}'

# Resolve via DNS
dig @hub -p 5353 myservice.internal +short
# Returns: 10.50.3.1
```

**Note**: API-registered backends are stored in memory (DashMap). For DNS resolution, backends should also be in routing.db.

---

## Error Responses

### 400 Bad Request

```json
{
  "error": "Missing required field: id"
}
```

### 404 Not Found

```json
{
  "error": "Backend not found",
  "id": "unknown-backend"
}
```

### 409 Conflict

```json
{
  "error": "Backend already exists",
  "id": "existing-backend"
}
```

---

## Monitoring Registered Backends

### Via API

```bash
# Count backends
curl -s http://hub:8081/api/v1/backends | jq '.total'

# List healthy backends
curl -s http://hub:8081/api/v1/backends | jq '.backends[] | select(.healthy==true) | .id'

# Find backends by region
curl -s http://hub:8081/api/v1/backends | jq '.backends[] | select(.region=="eu")'
```

### Via Logs

```bash
# Watch registration events
sudo journalctl -u edgeproxy -f | grep -i "register\|heartbeat"
```

---

## Test Summary

| Test | Result |
|------|--------|
| Health Check | OK |
| Registration (10 backends) | OK |
| List Backends | OK |
| Get Single Backend | OK |
| Heartbeat Update | OK |
| Deregistration | OK |

**Total**: All API tests passing
