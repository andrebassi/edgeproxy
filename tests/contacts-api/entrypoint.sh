#!/bin/sh
set -e

echo "=== Starting WireGuard ==="

# Create WireGuard config from environment variables
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << EOF
[Interface]
PrivateKey = ${WG_PRIVATE_KEY}
Address = ${WG_ADDRESS:-10.50.3.10/32}

[Peer]
PublicKey = ${WG_PEER_PUBLIC_KEY}
Endpoint = ${WG_PEER_ENDPOINT}
AllowedIPs = 10.50.0.0/24
PersistentKeepalive = 25
EOF

chmod 600 /etc/wireguard/wg0.conf

# Start WireGuard
wg-quick up wg0

echo "=== WireGuard Status ==="
wg show

echo "=== Testing connectivity to EC2 Hub ==="
ping -c 2 10.50.0.1 || echo "Ping failed (may be blocked by firewall)"

echo "=== Starting contacts-api ==="
exec ./contacts-api
