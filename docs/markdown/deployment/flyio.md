---
sidebar_position: 3
---

# Fly.io Deployment

This guide covers deploying edgeProxy backends on Fly.io with WireGuard overlay network for global distribution.

## Overview

![Fly.io Infrastructure](/img/flyio-infrastructure.svg)

Fly.io provides edge computing with machines in 30+ regions worldwide. We use it to deploy backend servers that connect to the central edgeProxy POP via WireGuard.

## Prerequisites

```bash
# Install Fly CLI
curl -L https://fly.io/install.sh | sh

# Login to Fly.io
fly auth login

# Verify authentication
fly auth whoami
```

## Available Regions

| Code | Location | Continent |
|------|----------|-----------|
| **gru** | Sao Paulo | South America |
| **iad** | Virginia | North America |
| **ord** | Chicago | North America |
| **lax** | Los Angeles | North America |
| **lhr** | London | Europe |
| **fra** | Frankfurt | Europe |
| **cdg** | Paris | Europe |
| **nrt** | Tokyo | Asia Pacific |
| **sin** | Singapore | Asia Pacific |
| **syd** | Sydney | Oceania |

---

## Project Structure

```
fly-backend/
├── fly.toml              # Fly.io configuration
├── Dockerfile            # Multi-stage build with WireGuard
├── main.go               # Backend server (Go)
├── entrypoint.sh         # WireGuard + backend startup
└── wireguard/
    └── keys/             # WireGuard keys per region
```

---

## Dockerfile

Multi-stage build with WireGuard support:

```dockerfile
FROM golang:1.21-alpine AS builder
WORKDIR /app
COPY main.go .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o backend main.go

FROM alpine:3.19
RUN apk --no-cache add ca-certificates wireguard-tools iptables ip6tables iproute2 bash
WORKDIR /app
COPY --from=builder /app/backend .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080
EXPOSE 51820/udp

ENTRYPOINT ["./entrypoint.sh"]
```

---

## Entrypoint Script

The entrypoint configures WireGuard based on the Fly region:

```bash
#!/bin/bash
set -e

# Central edgeProxy endpoint
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Map region to WireGuard IP
case "${FLY_REGION}" in
  gru) WG_IP="10.50.1.1/32"; WG_PRIVATE="${WG_KEY_GRU}" ;;
  iad) WG_IP="10.50.2.1/32"; WG_PRIVATE="${WG_KEY_IAD}" ;;
  ord) WG_IP="10.50.2.2/32"; WG_PRIVATE="${WG_KEY_ORD}" ;;
  lax) WG_IP="10.50.2.3/32"; WG_PRIVATE="${WG_KEY_LAX}" ;;
  lhr) WG_IP="10.50.3.1/32"; WG_PRIVATE="${WG_KEY_LHR}" ;;
  fra) WG_IP="10.50.3.2/32"; WG_PRIVATE="${WG_KEY_FRA}" ;;
  cdg) WG_IP="10.50.3.3/32"; WG_PRIVATE="${WG_KEY_CDG}" ;;
  nrt) WG_IP="10.50.4.1/32"; WG_PRIVATE="${WG_KEY_NRT}" ;;
  sin) WG_IP="10.50.4.2/32"; WG_PRIVATE="${WG_KEY_SIN}" ;;
  syd) WG_IP="10.50.4.3/32"; WG_PRIVATE="${WG_KEY_SYD}" ;;
  *) echo "Unknown region: ${FLY_REGION}"; exit 1 ;;
esac

# Create WireGuard config
mkdir -p /etc/wireguard
cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/16
PersistentKeepalive = 25
EOF

# Start WireGuard
wg-quick up wg0

echo "WireGuard connected: ${FLY_REGION} -> ${WG_IP}"

# Start backend
exec ./backend
```

---

## fly.toml Configuration

```toml
app = "edgeproxy-backend"
primary_region = "gru"

[build]
  dockerfile = "Dockerfile"

[env]
  PORT = "8080"

[http_service]
  internal_port = 8080
  force_https = false
  auto_stop_machines = false
  auto_start_machines = true
  min_machines_running = 1

[[vm]]
  cpu_kind = "shared"
  cpus = 1
  memory_mb = 256
```

---

## WireGuard Keys Setup

### Generate Keys

```bash
# Generate key pair for each region
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/keys/${region}-private.key
  cat wireguard/keys/${region}-private.key | wg pubkey > wireguard/keys/${region}-public.key
done
```

### Set Secrets in Fly.io

```bash
# Set WireGuard private keys as secrets
fly secrets set \
  WG_KEY_GRU="$(cat wireguard/keys/gru-private.key)" \
  WG_KEY_IAD="$(cat wireguard/keys/iad-private.key)" \
  WG_KEY_ORD="$(cat wireguard/keys/ord-private.key)" \
  WG_KEY_LAX="$(cat wireguard/keys/lax-private.key)" \
  WG_KEY_LHR="$(cat wireguard/keys/lhr-private.key)" \
  WG_KEY_FRA="$(cat wireguard/keys/fra-private.key)" \
  WG_KEY_CDG="$(cat wireguard/keys/cdg-private.key)" \
  WG_KEY_NRT="$(cat wireguard/keys/nrt-private.key)" \
  WG_KEY_SIN="$(cat wireguard/keys/sin-private.key)" \
  WG_KEY_SYD="$(cat wireguard/keys/syd-private.key)"
```

---

## Deployment

### Create App

```bash
cd fly-backend

# Create new app
fly apps create edgeproxy-backend

# Or launch interactively
fly launch --no-deploy
```

### Deploy to All Regions

```bash
# Deploy application
fly deploy --remote-only

# Scale to multiple regions (1 machine per region)
fly scale count 1 --region gru,iad,ord,lax,lhr,fra,cdg,nrt,sin,syd

# Verify deployment
fly status
```

### Scale Individual Regions

```bash
# Add more machines to specific region
fly scale count 2 --region gru

# Remove machines from region
fly scale count 0 --region lax
```

---

## Monitoring

### Check Status

```bash
# Application status
fly status

# List all machines
fly machines list

# View logs
fly logs

# Logs from specific region
fly logs --region gru
```

### SSH into Machine

```bash
# SSH to random machine
fly ssh console

# SSH to specific region
fly ssh console --region gru

# Check WireGuard status inside
wg show
```

### Health Check

```bash
# Test specific region
curl https://edgeproxy-backend.fly.dev/api/info

# Test via edgeProxy (should route to nearest backend)
curl http://54.171.48.207:8080/api/info
```

---

## Troubleshooting

### WireGuard Not Connecting

```bash
# SSH into machine
fly ssh console

# Check WireGuard status
wg show

# Check if interface exists
ip addr show wg0

# Check logs
cat /var/log/wireguard.log
```

### Machine Not Starting

```bash
# Check machine logs
fly logs --instance <machine-id>

# Restart machine
fly machines restart <machine-id>

# Destroy and recreate
fly machines destroy <machine-id>
fly scale count 1 --region <region>
```

### Secrets Not Set

```bash
# List secrets
fly secrets list

# Set missing secret
fly secrets set WG_KEY_GRU="<private-key>"
```

---

## Cost Optimization

### Shared CPU Machines

```toml
[[vm]]
  cpu_kind = "shared"
  cpus = 1
  memory_mb = 256  # Minimum for WireGuard
```

### Auto-Stop Idle Machines

```toml
[http_service]
  auto_stop_machines = true
  auto_start_machines = true
  min_machines_running = 0  # Scale to zero when idle
```

---

## Related Documentation

- [AWS EC2 Deployment](./aws) - Central POP setup
- [Docker Deployment](./docker) - Local development
- [Benchmarks](../benchmark) - Global performance tests
