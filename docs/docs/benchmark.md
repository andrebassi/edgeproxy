---
sidebar_position: 2
---

# Global Benchmark Tests

This document presents the complete benchmark results for edgeProxy with WireGuard overlay network, including infrastructure setup and test results across 9 global VPN locations.

## Test Results Summary

:::tip All Tests Passed
**Geo-Routing: 9/9 ✅** | **WireGuard: 10/10 peers ✅** | **Benchmark v2: Complete ✅**
:::

### Complete Test Table

| # | VPN Location | Country | Backend | Latency | Download 1MB | Download 5MB | RPS (20) | Status |
|---|--------------|---------|---------|---------|--------------|--------------|----------|--------|
| 1 | 🇫🇷 Paris | FR | **CDG** | 530ms | 0.5 MB/s | 2.1 MB/s | 35.7 | ✅ |
| 2 | 🇩🇪 Frankfurt | DE | **FRA** | 528ms | 0.6 MB/s | 2.3 MB/s | 34.0 | ✅ |
| 3 | 🇬🇧 London | GB | **LHR** | 490ms | 0.6 MB/s | 2.3 MB/s | 36.6 | ✅ |
| 4 | 🇺🇸 Detroit | US | **IAD** | 708ms | 0.6 MB/s | 2.5 MB/s | 27.4 | ✅ |
| 5 | 🇺🇸 Las Vegas | US | **IAD** | 857ms | 0.5 MB/s | 2.2 MB/s | 22.5 | ✅ |
| 6 | 🇯🇵 Tokyo | JP | **NRT** | 1546ms | 0.3 MB/s | 1.1 MB/s | 12.6 | ✅ |
| 7 | 🇸🇬 Singapore | SG | **SIN** | 1414ms | 0.3 MB/s | 1.2 MB/s | 13.8 | ✅ |
| 8 | 🇦🇺 Sydney | AU | **SYD** | 1847ms | 0.2 MB/s | 0.9 MB/s | 10.7 | ✅ |
| 9 | 🇧🇷 Sao Paulo | BR | **GRU** | 822ms | 0.4 MB/s | 1.6 MB/s | 23.3 | ✅ |

### Performance by Region

| Region | Latency | Observation |
|--------|---------|-------------|
| 🇪🇺 Europe (CDG/FRA/LHR) | 490-530ms | Best - closest to EC2 Ireland |
| 🇺🇸 USA (IAD) | 708-857ms | Medium - crosses Atlantic |
| 🇧🇷 Brazil (GRU) | 822ms | Good - direct route |
| 🇯🇵🇸🇬 Asia (NRT/SIN) | 1414-1546ms | High - geographic distance |
| 🇦🇺 Oceania (SYD) | 1847ms | Highest - half way around the world |

---

## Test Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                    edgeProxy + WireGuard - Production Test                  │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│   Client (VPN) ──► EC2 Ireland (edgeProxy) ──► WireGuard ──► Fly.io        │
│                    54.171.48.207:8080          10.50.x.x    10 regions     │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## Infrastructure Setup

### AWS EC2 Node Creation

The edgeProxy POP node was created on AWS EC2 using Taskfile automation:

#### Prerequisites

```bash
# AWS CLI configured with credentials
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_DEFAULT_REGION="eu-west-1"
```

#### Taskfile Configuration

The `fly-backend/Taskfile.yaml` contains all tasks for AWS infrastructure:

```yaml
version: '3'

vars:
  AWS_REGION: eu-west-1
  INSTANCE_TYPE: t3.micro
  AMI_ID: ami-0d940f23d527c3ab1  # Ubuntu 22.04 LTS
  KEY_NAME: edgeproxy-key
  SG_NAME: edgeproxy-sg
  INSTANCE_NAME: edgeproxy-pop-eu

tasks:
  aws:check:
    desc: Verify AWS credentials
    cmds:
      - aws sts get-caller-identity

  aws:sg:create:
    desc: Create Security Group for edgeProxy
    cmds:
      - |
        VPC_ID=$(aws ec2 describe-vpcs --filters "Name=is-default,Values=true" \
          --query 'Vpcs[0].VpcId' --output text)

        SG_ID=$(aws ec2 create-security-group \
          --group-name {{.SG_NAME}} \
          --description "EdgeProxy - TCP proxy with WireGuard" \
          --vpc-id $VPC_ID --query 'GroupId' --output text)

        # SSH, edgeProxy, WireGuard
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol tcp --port 22 --cidr 0.0.0.0/0
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol tcp --port 8080 --cidr 0.0.0.0/0
        aws ec2 authorize-security-group-ingress --group-id $SG_ID \
          --protocol udp --port 51820 --cidr 0.0.0.0/0

  aws:ec2:create:
    desc: Create EC2 instance for edgeProxy POP
    cmds:
      - |
        INSTANCE_ID=$(aws ec2 run-instances \
          --image-id {{.AMI_ID}} \
          --instance-type {{.INSTANCE_TYPE}} \
          --key-name {{.KEY_NAME}} \
          --security-group-ids $SG_ID \
          --user-data file://userdata.sh \
          --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value={{.INSTANCE_NAME}}}]' \
          --query 'Instances[0].InstanceId' --output text)

        aws ec2 wait instance-running --instance-ids $INSTANCE_ID
```

#### Creating the EC2 Instance

```bash
# Navigate to fly-backend directory
cd fly-backend

# 1. Verify AWS credentials
task aws:check

# 2. Create Security Group
task aws:sg:create

# 3. Create SSH Key Pair
task aws:key:create

# 4. Create EC2 Instance
task aws:ec2:create

# Output:
# Instance ID: i-0813ee3c789b40e51
# Public IP: 54.171.48.207
# SSH: ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@54.171.48.207
```

#### User Data Script (Auto-Install)

The EC2 instance auto-installs WireGuard and dependencies via user data:

```bash
#!/bin/bash
set -ex

# Update system
apt-get update && apt-get upgrade -y

# Install WireGuard
apt-get install -y wireguard wireguard-tools

# Install build tools
apt-get install -y curl wget git build-essential pkg-config libssl-dev

# Enable IP forwarding
echo "net.ipv4.ip_forward=1" >> /etc/sysctl.conf
echo "net.ipv6.conf.all.forwarding=1" >> /etc/sysctl.conf
sysctl -p

# Create edgeProxy directory
mkdir -p /opt/edgeproxy

# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
```

---

### WireGuard Configuration

#### Generating Keys

```bash
# Generate keys for EC2 (central server)
wg genkey > wireguard/ec2-private.key
cat wireguard/ec2-private.key | wg pubkey > wireguard/ec2-public.key

# Generate keys for each Fly.io region
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
done
```

#### EC2 WireGuard Server Config

```ini
[Interface]
PrivateKey = <ec2-private-key>
Address = 10.50.0.1/24
ListenPort = 51820
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT; iptables -t nat -A POSTROUTING -o ens5 -j MASQUERADE
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT; iptables -t nat -D POSTROUTING -o ens5 -j MASQUERADE

# GRU - Sao Paulo (South America)
[Peer]
PublicKey = <gru-public-key>
AllowedIPs = 10.50.1.1/32

# IAD - Virginia (North America)
[Peer]
PublicKey = <iad-public-key>
AllowedIPs = 10.50.2.1/32

# ... (all 10 peers)
```

#### Starting WireGuard

```bash
# On EC2
sudo cp wg0.conf /etc/wireguard/
sudo wg-quick up wg0

# Verify
sudo wg show
```

---

### Fly.io Backend Deployment

#### Dockerfile with WireGuard

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

#### Entrypoint Script

The entrypoint script configures WireGuard based on the Fly.io region:

```bash
#!/bin/bash
set -e

EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Map region to WireGuard IP
case "${FLY_REGION}" in
  gru) WG_IP="10.50.1.1/32"; WG_PRIVATE="<key>" ;;
  iad) WG_IP="10.50.2.1/32"; WG_PRIVATE="<key>" ;;
  # ... other regions
esac

# Create WireGuard config
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

# Start backend
exec ./backend
```

#### Deploy to Fly.io

```bash
cd fly-backend
fly deploy --remote-only

# Output: 10/10 machines deployed and healthy
```

---

### WireGuard Network Topology

```
                           WireGuard Mesh (10.50.x.x)
                                    │
        ┌───────────────────────────┼───────────────────────────┐
        │                           │                           │
        ▼                           ▼                           ▼
┌───────────────┐          ┌───────────────┐          ┌───────────────┐
│  EC2 Ireland  │          │  Fly.io GRU   │          │  Fly.io NRT   │
│  10.50.0.1    │◄────────►│  10.50.1.1    │          │  10.50.4.1    │
│  (edgeProxy)  │          │  (backend)    │          │  (backend)    │
└───────────────┘          └───────────────┘          └───────────────┘
        │
        │ All Fly.io backends connect to EC2
        │
        ├──► 10.50.2.1 (IAD) ──► 10.50.2.2 (ORD) ──► 10.50.2.3 (LAX)
        ├──► 10.50.3.1 (LHR) ──► 10.50.3.2 (FRA) ──► 10.50.3.3 (CDG)
        └──► 10.50.4.2 (SIN) ──► 10.50.4.3 (SYD)
```

| Region | Code | WireGuard IP | Location |
|--------|------|--------------|----------|
| South America | GRU | 10.50.1.1 | Sao Paulo, Brazil |
| North America | IAD | 10.50.2.1 | Virginia, USA |
| North America | ORD | 10.50.2.2 | Chicago, USA |
| North America | LAX | 10.50.2.3 | Los Angeles, USA |
| Europe | LHR | 10.50.3.1 | London, UK |
| Europe | FRA | 10.50.3.2 | Frankfurt, Germany |
| Europe | CDG | 10.50.3.3 | Paris, France |
| Asia Pacific | NRT | 10.50.4.1 | Tokyo, Japan |
| Asia Pacific | SIN | 10.50.4.2 | Singapore |
| Asia Pacific | SYD | 10.50.4.3 | Sydney, Australia |

---

## Geo-Routing Validation

All 9 VPN tests correctly routed to the expected backend:

| Client Location | Expected | Actual | Result |
|-----------------|----------|--------|--------|
| 🇫🇷 France | CDG | CDG | ✅ |
| 🇩🇪 Germany | FRA | FRA | ✅ |
| 🇬🇧 United Kingdom | LHR | LHR | ✅ |
| 🇺🇸 United States | IAD | IAD | ✅ |
| 🇯🇵 Japan | NRT | NRT | ✅ |
| 🇸🇬 Singapore | SIN | SIN | ✅ |
| 🇦🇺 Australia | SYD | SYD | ✅ |
| 🇧🇷 Brazil | GRU | GRU | ✅ |

---

## Running Your Own Tests

### Quick Latency Test

```bash
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://54.171.48.207:8080/api/latency
done
```

### Check Geo-Routing

```bash
curl -s http://54.171.48.207:8080/api/info | jq .
# Returns: {"region":"cdg","region_name":"Paris, France",...}
```

### Download Speed Test

```bash
# 1MB download
curl -w "Speed: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=1048576"

# 5MB download
curl -w "Speed: %{speed_download} B/s\n" -o /dev/null -s \
  "http://54.171.48.207:8080/api/download?size=5242880"
```

### Complete Benchmark Script

Use the provided script in `scripts/benchmark.sh`:

```bash
./scripts/benchmark.sh http://54.171.48.207:8080
```

---

## Benchmark Endpoints

| Endpoint | Description |
|----------|-------------|
| `/` | ASCII art banner with region info |
| `/api/info` | JSON server info (region, uptime, requests) |
| `/api/latency` | Minimal response for latency testing |
| `/api/download?size=N` | Download test (N bytes, max 100MB) |
| `/api/upload` | Upload test (POST body) |
| `/api/stats` | Server statistics |
| `/benchmark` | Interactive HTML benchmark page |

---

## Conclusions

1. **Geo-Routing**: 100% accuracy routing clients to correct regional backend
2. **WireGuard**: Stable tunnels with all 10 global backends
3. **Performance**: Latency scales predictably with geographic distance
4. **Reliability**: All tests passed with consistent results

### Production Deployment

For production, deploy edgeProxy POPs in multiple regions:

| Scenario | Expected Latency |
|----------|------------------|
| Client → Local POP → Local Backend | 5-20ms |
| Client → Local POP → Regional Backend | 20-50ms |
| Client → Local POP → Remote Backend | 50-150ms |

The test setup routes all traffic through Ireland. A full mesh deployment would significantly improve global performance.
