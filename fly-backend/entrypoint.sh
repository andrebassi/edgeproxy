#!/bin/bash
set -e

echo "=== Starting WireGuard + Backend ==="
echo "FLY_REGION: ${FLY_REGION}"

# EC2 endpoint e public key (hub)
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# GCP HKG endpoint e public key (full mesh para APAC)
HKG_ENDPOINT="35.241.112.61:51820"
HKG_PUBKEY="GxuSsvO9/raKe5WctZQfX5tkHOrTf0PLJWmHEzrw1Go="

# Mapear região para IP e chave privada
case "${FLY_REGION}" in
  gru)
    WG_IP="10.50.1.1/32"
    WG_PRIVATE="MENNp+hWPGoRMVhbObpNLJYpgAExjbwOSajiTchwsno="
    ;;
  iad)
    WG_IP="10.50.2.1/32"
    WG_PRIVATE="UHKsvajWt38Oe1D/vLrj0k7FQD7d9Tn0qtAxc+/e538="
    ;;
  ord)
    WG_IP="10.50.2.2/32"
    WG_PRIVATE="kEeHNS0OGP4Ubl78PoGw/cj7DNKJrxD4nMAm0A6bq0s="
    ;;
  lax)
    WG_IP="10.50.2.3/32"
    WG_PRIVATE="kIk+cVQ1rbh/YnWUikDikNRvF1pfZ5wp4L86EZmKd3I="
    ;;
  lhr)
    WG_IP="10.50.3.1/32"
    WG_PRIVATE="OIyE5jJJw+HR1K6InBSZOAsF4JwK4W32oNQZf0Y2UH8="
    ;;
  fra)
    WG_IP="10.50.3.2/32"
    WG_PRIVATE="iDlDxTX5YgnWdowm8o1UDNBwrLqBHZMDgPlgvbpVBnQ="
    ;;
  cdg)
    WG_IP="10.50.3.3/32"
    WG_PRIVATE="qJOjGFQOvLYQ3PIQLGmiaPxj1cVN0XXJpwqUdpInCls="
    ;;
  nrt)
    WG_IP="10.50.4.1/32"
    WG_PRIVATE="cEs2BDD01y8cvPygwcs7bW3sP2Bw5ZNxJHLvnT8/KGA="
    ;;
  sin)
    WG_IP="10.50.4.2/32"
    WG_PRIVATE="SCMcReLQo154dBpnSBvNTZ/vH/nwcWad7fE5NaPz+lo="
    ;;
  syd)
    WG_IP="10.50.4.3/32"
    WG_PRIVATE="eI9nV+ZMP3ZvUX3EYsCpXQBueDd8apcdDRwUhpGtRWY="
    ;;
  *)
    echo "Unknown region: ${FLY_REGION}, skipping WireGuard"
    exec ./backend
    ;;
esac

echo "Configuring WireGuard with IP: ${WG_IP}"

# Criar configuração WireGuard
mkdir -p /etc/wireguard

# Config base com EC2 como hub
cat > /etc/wireguard/wg0.conf << WGEOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
# EC2 Ireland (hub)
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/24, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24
PersistentKeepalive = 25
WGEOF

# Adicionar HKG como peer direto para regiões APAC
case "${FLY_REGION}" in
  nrt|sin|syd)
    echo "Adding HKG direct peer for APAC region..."
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

# Iniciar WireGuard
echo "Starting WireGuard interface..."
wg-quick up wg0 || echo "WireGuard failed (might need NET_ADMIN capability)"

# Mostrar status
wg show || true

echo "Starting backend server..."
exec ./backend
