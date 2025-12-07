#!/bin/bash
# Setup script to run on EC2 to enable RDS access via WireGuard
# Run this on the EC2 instance (edgeproxy-pop-eu)

set -e

RDS_HOST="edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com"
RDS_PORT=5432

echo "=== Setting up EC2 routing for RDS via WireGuard ==="

# Enable IP forwarding
echo "1. Enabling IP forwarding..."
sudo sysctl -w net.ipv4.ip_forward=1
echo "net.ipv4.ip_forward=1" | sudo tee -a /etc/sysctl.conf

# Get RDS IP
echo "2. Resolving RDS hostname..."
RDS_IP=$(dig +short $RDS_HOST | head -1)
echo "   RDS IP: $RDS_IP"

# Add iptables rules for DNAT
# Traffic from WireGuard (10.50.0.0/16) to EC2 port 5432 will be forwarded to RDS
echo "3. Setting up iptables DNAT rules..."

# Allow forwarding for RDS traffic
sudo iptables -A FORWARD -d $RDS_IP -p tcp --dport $RDS_PORT -j ACCEPT
sudo iptables -A FORWARD -s $RDS_IP -p tcp --sport $RDS_PORT -j ACCEPT

# DNAT: Redirect traffic to 10.50.0.1:5432 to RDS
sudo iptables -t nat -A PREROUTING -i wg0 -p tcp --dport $RDS_PORT -j DNAT --to-destination $RDS_IP:$RDS_PORT

# SNAT: Ensure return traffic comes back through EC2
sudo iptables -t nat -A POSTROUTING -d $RDS_IP -p tcp --dport $RDS_PORT -j MASQUERADE

echo "4. Saving iptables rules..."
sudo iptables-save | sudo tee /etc/iptables/rules.v4

echo "5. Testing connection to RDS..."
nc -zv $RDS_IP $RDS_PORT

echo ""
echo "=== Setup complete ==="
echo ""
echo "From Fly.io apps, connect to the database using:"
echo "  Host: 10.50.0.1 (EC2 WireGuard IP)"
echo "  Port: 5432"
echo "  Database: contacts"
echo "  User: postgres"
echo "  Password: EdgeProxy2024"
echo ""
echo "The traffic will be routed:"
echo "  Fly (10.50.x.x) -> WireGuard -> EC2 (10.50.0.1) -> NAT -> RDS ($RDS_IP)"
