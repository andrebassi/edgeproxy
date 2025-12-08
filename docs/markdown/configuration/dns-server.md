---
sidebar_position: 4
---

# Internal DNS Server

The DNS server provides geo-aware name resolution for `.internal` domains.

## Usage

```bash
# Enable DNS server
export EDGEPROXY_DNS_ENABLED=true
export EDGEPROXY_DNS_LISTEN_ADDR=0.0.0.0:5353
export EDGEPROXY_DNS_DOMAIN=internal

# Query for best backend IP (geo-aware)
dig @localhost -p 5353 myapp.internal A

# Response: Best backend IP based on client location
;; ANSWER SECTION:
myapp.internal.    300    IN    A    10.50.1.5
```

## DNS Schema

| Domain | Resolves To | Example |
|--------|-------------|---------|
| `<app>.internal` | Best backend IP | `myapp.internal` → `10.50.1.5` |
| `<region>.backends.internal` | Backend WG IP | `nrt.backends.internal` → `10.50.4.1` |
| `<region>.pops.internal` | POP WG IP | `hkg.pops.internal` → `10.50.5.1` |

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_DNS_ENABLED` | `false` | Enable DNS server |
| `EDGEPROXY_DNS_LISTEN_ADDR` | `0.0.0.0:5353` | DNS listen address |
| `EDGEPROXY_DNS_DOMAIN` | `internal` | DNS domain suffix |

## Benefits

- **Abstraction**: Change IPs without updating configs
- **Migration**: Move backends without downtime
- **Geo-aware**: Returns best backend based on client location

## Integration Examples

### Docker Compose

```yaml
services:
  app:
    dns: edgeproxy
    environment:
      - API_HOST=backend.internal
```

### Application Configuration

```bash
# Instead of hardcoding IPs
export BACKEND_HOST=myapp.internal

# Application resolves via edgeProxy DNS
curl http://myapp.internal:8080/api
```
