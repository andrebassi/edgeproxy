#!/bin/bash
# =============================================================================
# edgeProxy POP - User Data / Cloud Init Script
# =============================================================================
# Works on: AWS EC2, GCP Compute Engine, Azure VM, any Ubuntu 22.04+
#
# Usage:
#   1. Set environment variables before running or pass as metadata
#   2. Script will install WireGuard (mesh), edgeProxy, and start services
#
# Required variables:
#   POP_REGION      - Region code (eu, ap, us, sa)
#   WG_PRIVATE_KEY  - WireGuard private key for this POP
#   WG_ADDRESS      - WireGuard IP address (e.g., 10.50.5.1/24)
#
# Optional:
#   EDGEPROXY_BINARY_URL - URL to download edgeProxy binary
#   ROUTING_DB_URL       - URL to download routing.db
# =============================================================================

set -e
exec > >(tee /var/log/userdata.log) 2>&1
echo "=== edgeProxy POP Setup Started: $(date) ==="

# -----------------------------------------------------------------------------
# Configuration - Set these via cloud metadata or environment
# -----------------------------------------------------------------------------
POP_REGION="${POP_REGION:-ap}"
WG_PRIVATE_KEY="${WG_PRIVATE_KEY}"
WG_ADDRESS="${WG_ADDRESS:-10.50.5.1/24}"
WG_LISTEN_PORT="${WG_LISTEN_PORT:-51820}"

# -----------------------------------------------------------------------------
# WireGuard Mesh Peers - All backends
# -----------------------------------------------------------------------------
# Format: "PublicKey|Endpoint|AllowedIPs|Name"
# Endpoint can be empty for peers behind NAT (they connect to us)

declare -a WG_PEERS=(
  # EC2 Ireland (central, optional for mesh but useful for non-APAC traffic)
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

# -----------------------------------------------------------------------------
# Install packages
# -----------------------------------------------------------------------------
echo "=== Installing packages ==="
apt-get update
apt-get install -y wireguard curl jq

# -----------------------------------------------------------------------------
# Configure WireGuard (Mesh)
# -----------------------------------------------------------------------------
echo "=== Configuring WireGuard Mesh ==="

if [ -z "$WG_PRIVATE_KEY" ]; then
  echo "ERROR: WG_PRIVATE_KEY not set!"
  exit 1
fi

mkdir -p /etc/wireguard

# Create base config
cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE_KEY}
Address = ${WG_ADDRESS}
ListenPort = ${WG_LISTEN_PORT}

# Enable IP forwarding for routing between peers
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -o wg0 -j ACCEPT
EOF

# Add all peers (mesh topology)
for peer in "${WG_PEERS[@]}"; do
  IFS='|' read -r pubkey endpoint allowed_ips name <<< "$peer"

  cat >> /etc/wireguard/wg0.conf << EOF

# ${name}
[Peer]
PublicKey = ${pubkey}
AllowedIPs = ${allowed_ips}
PersistentKeepalive = 25
EOF

  # Add endpoint if specified (for peers with static IPs)
  if [ -n "$endpoint" ]; then
    sed -i "/PublicKey = ${pubkey}/a Endpoint = ${endpoint}" /etc/wireguard/wg0.conf
  fi
done

# Set permissions
chmod 600 /etc/wireguard/wg0.conf

# Start WireGuard
echo "=== Starting WireGuard ==="
wg-quick up wg0
systemctl enable wg-quick@wg0

# Show status
wg show

# -----------------------------------------------------------------------------
# Install edgeProxy
# -----------------------------------------------------------------------------
echo "=== Installing edgeProxy ==="

mkdir -p /opt/edgeproxy

# Download binary if URL provided, otherwise expect it to be copied manually
if [ -n "$EDGEPROXY_BINARY_URL" ]; then
  curl -L -o /opt/edgeproxy/edge-proxy "$EDGEPROXY_BINARY_URL"
  chmod +x /opt/edgeproxy/edge-proxy
fi

if [ -n "$ROUTING_DB_URL" ]; then
  curl -L -o /opt/edgeproxy/routing.db "$ROUTING_DB_URL"
fi

# Create systemd service
cat > /etc/systemd/system/edgeproxy.service << EOF
[Unit]
Description=edgeProxy TCP Proxy
After=network.target wg-quick@wg0.service
Wants=wg-quick@wg0.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/edgeproxy
Environment=EDGEPROXY_REGION=${POP_REGION}
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_DB_PATH=/opt/edgeproxy/routing.db
ExecStart=/opt/edgeproxy/edge-proxy
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable edgeproxy

# Start if binary exists
if [ -f /opt/edgeproxy/edge-proxy ]; then
  systemctl start edgeproxy
  echo "=== edgeProxy started ==="
else
  echo "=== edgeProxy binary not found, service enabled but not started ==="
  echo "=== Copy binary to /opt/edgeproxy/edge-proxy and run: systemctl start edgeproxy ==="
fi

# -----------------------------------------------------------------------------
# Summary
# -----------------------------------------------------------------------------
echo ""
echo "=============================================="
echo "edgeProxy POP Setup Complete!"
echo "=============================================="
echo "Region: ${POP_REGION}"
echo "WireGuard IP: ${WG_ADDRESS}"
echo "WireGuard Port: ${WG_LISTEN_PORT}"
echo "Peers configured: ${#WG_PEERS[@]}"
echo ""
echo "WireGuard status:"
wg show wg0 | head -20
echo ""
echo "Next steps:"
echo "  1. Copy edge-proxy binary to /opt/edgeproxy/"
echo "  2. Copy routing.db to /opt/edgeproxy/"
echo "  3. Run: systemctl start edgeproxy"
echo "=============================================="
