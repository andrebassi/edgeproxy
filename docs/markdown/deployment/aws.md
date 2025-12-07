---
sidebar_position: 3
---

# EC2 Deployment

This guide covers deploying edgeProxy as a POP (Point of Presence) node on AWS EC2 with WireGuard overlay network.

## Prerequisites

```bash
# AWS CLI configured with credentials
export AWS_ACCESS_KEY_ID="your-access-key"
export AWS_SECRET_ACCESS_KEY="your-secret-key"
export AWS_DEFAULT_REGION="eu-west-1"

# Verify credentials
aws sts get-caller-identity
```

## Infrastructure Overview

![AWS Infrastructure](/img/aws-infrastructure.svg)

---

## EC2 Instance Creation

### Using Taskfile

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

  aws:key:create:
    desc: Create SSH Key Pair
    cmds:
      - |
        aws ec2 create-key-pair --key-name {{.KEY_NAME}} \
          --query 'KeyMaterial' --output text > ~/.ssh/{{.KEY_NAME}}.pem
        chmod 400 ~/.ssh/{{.KEY_NAME}}.pem

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

        PUBLIC_IP=$(aws ec2 describe-instances --instance-ids $INSTANCE_ID \
          --query 'Reservations[0].Instances[0].PublicIpAddress' --output text)

        echo "Instance: $INSTANCE_ID"
        echo "Public IP: $PUBLIC_IP"
        echo "SSH: ssh -i ~/.ssh/{{.KEY_NAME}}.pem ubuntu@$PUBLIC_IP"
```

### Step-by-Step Creation

```bash
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

---

## Building and Deploying edgeProxy

### Cross-Compile for Linux (from macOS/Linux)

Build the binary locally using Docker for faster deployment:

```bash
# Build for Linux amd64 using Docker
docker run --rm --platform linux/amd64 \
  -v "$(pwd)":/app -w /app \
  rust:latest \
  bash -c "apt-get update && apt-get install -y pkg-config libssl-dev && cargo build --release"

# Binary will be at target/release/edge-proxy (~16MB)
ls -la target/release/edge-proxy
```

### Deploy to EC2

```bash
# Get EC2 public IP
EC2_IP=54.171.48.207

# Copy binary and routing database to EC2
scp -i ~/.ssh/edgeproxy-key.pem target/release/edge-proxy ubuntu@$EC2_IP:/tmp/
scp -i ~/.ssh/edgeproxy-key.pem routing.db ubuntu@$EC2_IP:/tmp/

# SSH and setup on EC2
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@$EC2_IP "
  sudo mkdir -p /opt/edgeproxy
  sudo mv /tmp/edge-proxy /opt/edgeproxy/
  sudo mv /tmp/routing.db /opt/edgeproxy/
  sudo chmod +x /opt/edgeproxy/edge-proxy
"
```

### Create systemd Service

```bash
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@$EC2_IP "
cat | sudo tee /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy TCP Proxy
After=network.target wireguard.service

[Service]
Type=simple
User=root
WorkingDirectory=/opt/edgeproxy
Environment=EDGEPROXY_REGION=eu
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_DB_PATH=/opt/edgeproxy/routing.db
ExecStart=/opt/edgeproxy/edge-proxy
Restart=always
RestartSec=5

[Install]
WantedBy=multi-user.target
EOF

sudo systemctl daemon-reload
sudo systemctl enable edgeproxy
sudo systemctl start edgeproxy
sudo systemctl status edgeproxy
"
```

### Verify Deployment

```bash
# Check service status
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@$EC2_IP "sudo systemctl status edgeproxy"

# Check logs
ssh -i ~/.ssh/edgeproxy-key.pem ubuntu@$EC2_IP "sudo journalctl -u edgeproxy -n 20"

# Test connectivity (from local machine)
nc -zv $EC2_IP 8080
```

---

## WireGuard Configuration

### Generate Keys

```bash
# Generate keys for EC2 (central server)
wg genkey > wireguard/ec2-private.key
cat wireguard/ec2-private.key | wg pubkey > wireguard/ec2-public.key

# Generate keys for each backend region
for region in gru iad ord lax lhr fra cdg nrt sin syd; do
  wg genkey > wireguard/${region}-private.key
  cat wireguard/${region}-private.key | wg pubkey > wireguard/${region}-public.key
done
```

### EC2 Server Config

```ini
# /etc/wireguard/wg0.conf
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

# ORD - Chicago (North America)
[Peer]
PublicKey = <ord-public-key>
AllowedIPs = 10.50.2.2/32

# LAX - Los Angeles (North America)
[Peer]
PublicKey = <lax-public-key>
AllowedIPs = 10.50.2.3/32

# LHR - London (Europe)
[Peer]
PublicKey = <lhr-public-key>
AllowedIPs = 10.50.3.1/32

# FRA - Frankfurt (Europe)
[Peer]
PublicKey = <fra-public-key>
AllowedIPs = 10.50.3.2/32

# CDG - Paris (Europe)
[Peer]
PublicKey = <cdg-public-key>
AllowedIPs = 10.50.3.3/32

# NRT - Tokyo (Asia)
[Peer]
PublicKey = <nrt-public-key>
AllowedIPs = 10.50.4.1/32

# SIN - Singapore (Asia)
[Peer]
PublicKey = <sin-public-key>
AllowedIPs = 10.50.4.2/32

# SYD - Sydney (Oceania)
[Peer]
PublicKey = <syd-public-key>
AllowedIPs = 10.50.4.3/32
```

### Start WireGuard

```bash
# Copy config
sudo cp wg0.conf /etc/wireguard/

# Start WireGuard
sudo wg-quick up wg0

# Enable on boot
sudo systemctl enable wg-quick@wg0

# Verify connections
sudo wg show
```

---

## Network Topology

![WireGuard Topology](/img/wireguard-topology.svg)

### IP Allocation

| Region | Code | WireGuard IP | Location |
|--------|------|--------------|----------|
| **Central** | EC2 | 10.50.0.1 | Ireland (eu-west-1) |
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

## Security Group Rules

| Port | Protocol | Source | Description |
|------|----------|--------|-------------|
| 22 | TCP | Your IP | SSH access |
| 8080 | TCP | 0.0.0.0/0 | edgeProxy TCP |
| 51820 | UDP | 0.0.0.0/0 | WireGuard |

### Restricting SSH

```bash
# Get your IP
MY_IP=$(curl -s ifconfig.me)

# Update security group
aws ec2 authorize-security-group-ingress \
  --group-id $SG_ID \
  --protocol tcp \
  --port 22 \
  --cidr ${MY_IP}/32
```

---

## Monitoring

### Check WireGuard Status

```bash
# Show all peers
sudo wg show

# Show specific peer
sudo wg show wg0 peers

# Check handshakes
sudo wg show wg0 latest-handshakes
```

### Check edgeProxy

```bash
# Service status
sudo systemctl status edgeproxy

# Logs
sudo journalctl -u edgeproxy -f

# Test connection
curl http://localhost:8080/api/info
```

---

## Troubleshooting

### WireGuard Not Connecting

```bash
# Check interface
ip addr show wg0

# Check routing
ip route | grep wg0

# Test connectivity
ping 10.50.1.1  # GRU backend
```

### EC2 Instance Not Reachable

```bash
# Check security groups
aws ec2 describe-security-groups --group-ids $SG_ID

# Check instance status
aws ec2 describe-instance-status --instance-ids $INSTANCE_ID
```

---

## Next Steps

- [Fly.io Deployment](./flyio) - Deploy backends globally on Fly.io
- [Benchmarks](../benchmark) - Global performance tests
- [Docker Deployment](./docker) - Local development
