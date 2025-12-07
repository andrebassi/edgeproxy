---
sidebar_position: 4
---

# WireGuard Overlay Network

This document covers the WireGuard overlay network that connects all edgeProxy POPs and backends globally.

:::info Why WireGuard?
WireGuard provides a secure, high-performance overlay network that allows edgeProxy POPs to route traffic to backends regardless of their physical location or network topology.
:::

---

## Network Architecture

### Overview

![WireGuard Full Mesh Network](/img/wireguard-full-mesh.svg)

### IP Allocation Scheme

| Subnet | Region | Description |
|--------|--------|-------------|
| `10.50.0.0/24` | Central | EC2 Ireland (Hub) |
| `10.50.1.0/24` | South America | GRU (Sao Paulo) |
| `10.50.2.0/24` | North America | IAD, ORD, LAX |
| `10.50.3.0/24` | Europe | LHR, FRA, CDG |
| `10.50.4.0/24` | Asia Pacific | NRT, SIN, SYD |
| `10.50.5.0/24` | Asia Pacific | HKG POP (GCP) |

### Backend IP Assignments

| Backend | Code | WireGuard IP | Location |
|---------|------|--------------|----------|
| EC2 Ireland | - | 10.50.0.1 | eu-west-1 |
| Sao Paulo | GRU | 10.50.1.1 | gru |
| Virginia | IAD | 10.50.2.1 | iad |
| Chicago | ORD | 10.50.2.2 | ord |
| Los Angeles | LAX | 10.50.2.3 | lax |
| London | LHR | 10.50.3.1 | lhr |
| Frankfurt | FRA | 10.50.3.2 | fra |
| Paris | CDG | 10.50.3.3 | cdg |
| Tokyo | NRT | 10.50.4.1 | nrt |
| Singapore | SIN | 10.50.4.2 | sin |
| Sydney | SYD | 10.50.4.3 | syd |
| Hong Kong | HKG | 10.50.5.1 | asia-east2 |

---

## Topologies

### Hub-and-Spoke (Legacy)

In the initial setup, all traffic routed through a central hub (EC2 Ireland):

![Hub-and-Spoke Topology](/img/wireguard-hub-spoke.svg)

**Problems:**
- High latency for geographically distant backends
- Single point of failure
- All traffic crosses Ireland regardless of destination

**Example latencies (from HKG POP):**
| Backend | Latency via Hub |
|---------|-----------------|
| NRT (Tokyo) | 492ms |
| SIN (Singapore) | 408ms |
| SYD (Sydney) | ~500ms |

### Full Mesh (Current)

POPs connect directly to their regional backends:

![HKG Full Mesh](/img/wireguard-hkg-mesh.svg)

**Benefits:**
- ~10x lower latency for regional traffic
- No single point of failure for regional routing
- Traffic stays within geographic region

**Example latencies (from HKG POP with full mesh):**
| Backend | Hub Latency | Mesh Latency | Improvement |
|---------|-------------|--------------|-------------|
| NRT (Tokyo) | 492ms | **49ms** | **10x** |
| SIN (Singapore) | 408ms | **38ms** | **10.7x** |
| SYD (Sydney) | ~500ms | **122ms** | **~4x** |

---

## Configuration

### Key Generation

Generate a keypair for each node:

```bash
# Generate private key
wg genkey > private.key

# Derive public key
cat private.key | wg pubkey > public.key

# Generate all backend keys at once
for region in gru iad ord lax lhr fra cdg nrt sin syd hkg; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
  echo "${region}: $(cat wireguard/${region}-public.key)"
done
```

### EC2 Hub Configuration

The EC2 Ireland instance acts as the central hub for non-regional traffic:

```ini
# /etc/wireguard/wg0.conf on EC2 Ireland
[Interface]
PrivateKey = <ec2-private-key>
Address = 10.50.0.1/24
ListenPort = 51820

# Enable IP forwarding for routing
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT

# GRU - Sao Paulo
[Peer]
PublicKey = He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc=
AllowedIPs = 10.50.1.1/32
PersistentKeepalive = 25

# IAD - Virginia
[Peer]
PublicKey = rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ=
AllowedIPs = 10.50.2.1/32
PersistentKeepalive = 25

# ORD - Chicago
[Peer]
PublicKey = SIh+oa2J6k4rYA+N1SzskwztVVR/1Hx3ef/yLyyh+VU=
AllowedIPs = 10.50.2.2/32
PersistentKeepalive = 25

# LAX - Los Angeles
[Peer]
PublicKey = z7JmcJguquFBQiphSSmYBsttr6BoRs8MkCev9o5JkAU=
AllowedIPs = 10.50.2.3/32
PersistentKeepalive = 25

# LHR - London
[Peer]
PublicKey = w+XApd9CmhlyweQr8Fp7YPMbjd6RAk/cmXA6OET9/H0=
AllowedIPs = 10.50.3.1/32
PersistentKeepalive = 25

# FRA - Frankfurt
[Peer]
PublicKey = g5IzaRpt1hkvFhGTfy5LC0HLwPxVTC5dQb3if5sds24=
AllowedIPs = 10.50.3.2/32
PersistentKeepalive = 25

# CDG - Paris
[Peer]
PublicKey = C1My7suqoLuYchPIaVLbsB5A/dX21h7wICqa7yL2oX4=
AllowedIPs = 10.50.3.3/32
PersistentKeepalive = 25

# NRT - Tokyo
[Peer]
PublicKey = 9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=
AllowedIPs = 10.50.4.1/32
PersistentKeepalive = 25

# SIN - Singapore
[Peer]
PublicKey = gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=
AllowedIPs = 10.50.4.2/32
PersistentKeepalive = 25

# SYD - Sydney
[Peer]
PublicKey = 9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=
AllowedIPs = 10.50.4.3/32
PersistentKeepalive = 25

# HKG POP - Hong Kong
[Peer]
PublicKey = GxuSsvO9/raKe5WctZQfX5tkHOrTf0PLJWmHEzrw1Go=
AllowedIPs = 10.50.5.0/24
PersistentKeepalive = 25
```

### GCP HKG POP Configuration (Full Mesh)

The HKG POP uses full mesh for APAC backends:

```ini
# /etc/wireguard/wg0.conf on GCP HKG
[Interface]
PrivateKey = <hkg-private-key>
Address = 10.50.5.1/24
ListenPort = 51820

# Enable IP forwarding
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT

# EC2 Ireland (for non-APAC backends: SA, NA, EU)
[Peer]
PublicKey = bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=
Endpoint = 54.171.48.207:51820
AllowedIPs = 10.50.0.1/32, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24
PersistentKeepalive = 25

# NRT - Tokyo (DIRECT MESH)
[Peer]
PublicKey = 9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=
AllowedIPs = 10.50.4.1/32
PersistentKeepalive = 25

# SIN - Singapore (DIRECT MESH)
[Peer]
PublicKey = gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=
AllowedIPs = 10.50.4.2/32
PersistentKeepalive = 25

# SYD - Sydney (DIRECT MESH)
[Peer]
PublicKey = 9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=
AllowedIPs = 10.50.4.3/32
PersistentKeepalive = 25
```

### Backend Configuration (APAC)

APAC backends connect to both EC2 hub and HKG POP:

```bash
#!/bin/bash
# entrypoint.sh for APAC backends

# EC2 endpoint (hub)
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# HKG endpoint (direct mesh)
HKG_ENDPOINT="35.241.112.61:51820"
HKG_PUBKEY="GxuSsvO9/raKe5WctZQfX5tkHOrTf0PLJWmHEzrw1Go="

# Base config with EC2 hub
cat > /etc/wireguard/wg0.conf << WGEOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
# EC2 Ireland (hub for non-APAC traffic)
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/24, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24
PersistentKeepalive = 25
WGEOF

# Add HKG direct peer for APAC regions
case "${REGION}" in
  nrt|sin|syd)
    cat >> /etc/wireguard/wg0.conf << WGEOF

[Peer]
# GCP HKG (direct mesh for APAC)
PublicKey = ${HKG_PUBKEY}
Endpoint = ${HKG_ENDPOINT}
AllowedIPs = 10.50.5.0/24
PersistentKeepalive = 25
WGEOF
    ;;
esac

wg-quick up wg0
```

---

## User Data Script

Use this unified script to provision POPs on any cloud (AWS, GCP, Azure):

```bash
#!/bin/bash
# =============================================================================
# edgeProxy POP - User Data / Cloud Init Script
# =============================================================================
# Works on: AWS EC2, GCP Compute Engine, Azure VM, any Ubuntu 22.04+
#
# Required variables:
#   POP_REGION      - Region code (eu, ap, us, sa)
#   WG_PRIVATE_KEY  - WireGuard private key for this POP
#   WG_ADDRESS      - WireGuard IP address (e.g., 10.50.5.1/24)
# =============================================================================

set -e
exec > >(tee /var/log/userdata.log) 2>&1
echo "=== edgeProxy POP Setup Started: $(date) ==="

# Configuration
POP_REGION="${POP_REGION:-ap}"
WG_PRIVATE_KEY="${WG_PRIVATE_KEY}"
WG_ADDRESS="${WG_ADDRESS:-10.50.5.1/24}"
WG_LISTEN_PORT="${WG_LISTEN_PORT:-51820}"

# All backend peers (full mesh)
declare -a WG_PEERS=(
  # EC2 Ireland (central hub)
  "bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY=|54.171.48.207:51820|10.50.0.1/32|ec2-ireland"

  # South America
  "He2jX3+iEl7hUaaJG/i3YcSnStEFAcW/rs/lP0Pw+nc=||10.50.1.1/32|gru"

  # North America
  "rImgzxPu9MuhqLpcvXQ9xckSSA+AGbDOpBGvTUOwaHQ=||10.50.2.1/32|iad"
  "SIh+oa2J6k4rYA+N1SzskwztVVR/1Hx3ef/yLyyh+VU=||10.50.2.2/32|ord"
  "z7JmcJguquFBQiphSSmYBsttr6BoRs8MkCev9o5JkAU=||10.50.2.3/32|lax"

  # Europe
  "w+XApd9CmhlyweQr8Fp7YPMbjd6RAk/cmXA6OET9/H0=||10.50.3.1/32|lhr"
  "g5IzaRpt1hkvFhGTfy5LC0HLwPxVTC5dQb3if5sds24=||10.50.3.2/32|fra"
  "C1My7suqoLuYchPIaVLbsB5A/dX21h7wICqa7yL2oX4=||10.50.3.3/32|cdg"

  # Asia Pacific
  "9ZK9FzSzihxrRX9gktc99Oj0WFSJMa0mf33pP2LJ/lU=||10.50.4.1/32|nrt"
  "gcwoqaT950PGW1ZaUEV75VEV7HOdyYT5rwdYOUBQzR0=||10.50.4.2/32|sin"
  "9yHQmzbLKEyM+F1x7obbX0WNaR25XhAcUOdU9SLBeEo=||10.50.4.3/32|syd"
)

# Install packages
apt-get update
apt-get install -y wireguard curl jq

# Create WireGuard config
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE_KEY}
Address = ${WG_ADDRESS}
ListenPort = ${WG_LISTEN_PORT}

PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT
EOF

# Add all peers
for peer in "${WG_PEERS[@]}"; do
  IFS='|' read -r pubkey endpoint allowed_ips name <<< "$peer"

  cat >> /etc/wireguard/wg0.conf << EOF

# ${name}
[Peer]
PublicKey = ${pubkey}
AllowedIPs = ${allowed_ips}
PersistentKeepalive = 25
EOF

  if [ -n "$endpoint" ]; then
    sed -i "/PublicKey = ${pubkey}/a Endpoint = ${endpoint}" /etc/wireguard/wg0.conf
  fi
done

chmod 600 /etc/wireguard/wg0.conf

# Start WireGuard
wg-quick up wg0
systemctl enable wg-quick@wg0

echo "=== WireGuard Status ==="
wg show
```

---

## Operations

### Starting WireGuard

```bash
# Start interface
sudo wg-quick up wg0

# Enable on boot
sudo systemctl enable wg-quick@wg0

# Check status
sudo wg show
```

### Checking Connectivity

```bash
# Show all peers and their status
sudo wg show

# Ping specific backend
ping 10.50.4.1  # NRT
ping 10.50.4.2  # SIN
ping 10.50.4.3  # SYD

# Check handshake times
sudo wg show wg0 latest-handshakes
```

### Adding a New Peer

```bash
# Generate keys for new peer
wg genkey > new-peer-private.key
cat new-peer-private.key | wg pubkey > new-peer-public.key

# Add peer dynamically (no restart needed)
sudo wg set wg0 peer $(cat new-peer-public.key) allowed-ips 10.50.6.1/32

# Save to config file
sudo wg-quick save wg0
```

### Removing a Peer

```bash
# Remove peer dynamically
sudo wg set wg0 peer <public-key> remove

# Or edit config and restart
sudo vim /etc/wireguard/wg0.conf
sudo wg-quick down wg0 && sudo wg-quick up wg0
```

---

## Troubleshooting

### Peer Not Connecting

```bash
# Check interface exists
ip addr show wg0

# Check peer is configured
sudo wg show wg0 peers

# Check firewall
sudo iptables -L -n | grep 51820

# Check UDP port is open
nc -zvu <peer-ip> 51820
```

### High Latency

```bash
# Measure latency to each peer
for ip in 10.50.4.1 10.50.4.2 10.50.4.3; do
  echo "Ping to $ip:"
  ping -c 3 $ip | tail -1
done

# If latency is high, check if traffic is going through hub
# Add direct peer endpoints for regional traffic
```

### Handshake Not Happening

```bash
# Check last handshake time
sudo wg show wg0 latest-handshakes

# If handshake is old (>2 minutes), peer may be unreachable
# Try forcing a new handshake
ping -c 1 <peer-wg-ip>
```

### Traffic Not Routing

```bash
# Check routing table
ip route | grep wg0

# Verify AllowedIPs includes destination
sudo wg show wg0 allowed-ips

# Check IP forwarding is enabled
sysctl net.ipv4.ip_forward
```

---

## Security Best Practices

### Key Management

1. **Never share private keys** - Each node must have unique keys
2. **Rotate keys periodically** - Regenerate keys every 6-12 months
3. **Store keys securely** - Use secrets management (AWS Secrets Manager, Vault)

### Firewall Rules

```bash
# Only allow WireGuard UDP
ufw allow 51820/udp

# Restrict SSH to known IPs
ufw allow from 1.2.3.4 to any port 22
```

### Network Segmentation

- Use separate subnets for each region
- Limit AllowedIPs to minimum required
- Don't use 0.0.0.0/0 unless absolutely necessary

---

## Performance Optimization

### MTU Tuning

```ini
[Interface]
MTU = 1420  # Optimal for most scenarios
```

### Persistent Keepalive

```ini
[Peer]
PersistentKeepalive = 25  # Keep NAT mappings alive
```

### Endpoint Selection

- Use static endpoints for servers with public IPs
- Omit endpoints for clients behind NAT (they'll connect to us)
- Use DNS names for dynamic IPs

---

## Related Documentation

- [AWS EC2 Deployment](./deployment/aws) - EC2 hub setup
- [GCP Deployment](./deployment/gcp) - GCP POP setup
- [Docker Deployment](./deployment/docker) - Local development
- [Benchmarks](./benchmark) - Performance results with WireGuard mesh
