---
sidebar_position: 4
---

# GCP Compute Engine Deployment

This guide covers deploying edgeProxy as a POP (Point of Presence) node on Google Cloud Platform in Asia (Hong Kong region).

:::info Why Hong Kong?
GCP doesn't have data centers in mainland China. Hong Kong (`asia-east2`) is the closest region and provides excellent latency to China, Southeast Asia, and the broader APAC region.
:::

## Prerequisites

```bash
# Install gcloud CLI
# https://cloud.google.com/sdk/docs/install

# Authenticate
gcloud auth login

# Set project
gcloud config set project YOUR_PROJECT_ID

# Enable Compute Engine API
gcloud services enable compute.googleapis.com

# Verify
gcloud config list
```

## Infrastructure Overview

![GCP Infrastructure](/img/gcp-infrastructure.svg)

---

## VM Instance Creation

### Using Taskfile

```yaml
version: '3'

vars:
  GCP_PROJECT: your-project-id
  GCP_REGION: asia-east2      # Hong Kong
  GCP_ZONE: asia-east2-a
  MACHINE_TYPE: e2-micro      # Free tier eligible
  IMAGE_FAMILY: ubuntu-2204-lts
  IMAGE_PROJECT: ubuntu-os-cloud
  INSTANCE_NAME: edgeproxy-pop-hkg

tasks:
  gcp:check:
    desc: Verify GCP credentials
    cmds:
      - gcloud config list

  gcp:firewall:create:
    desc: Create firewall rules for edgeProxy
    cmds:
      - |
        gcloud compute firewall-rules create edgeproxy-allow-ssh \
          --allow tcp:22 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Allow SSH to edgeProxy"

        gcloud compute firewall-rules create edgeproxy-allow-proxy \
          --allow tcp:8080 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Allow edgeProxy TCP traffic"

        gcloud compute firewall-rules create edgeproxy-allow-wireguard \
          --allow udp:51820 \
          --source-ranges 0.0.0.0/0 \
          --target-tags edgeproxy \
          --description "Allow WireGuard VPN"

  gcp:vm:create:
    desc: Create VM instance for edgeProxy POP
    cmds:
      - |
        gcloud compute instances create {{.INSTANCE_NAME}} \
          --zone={{.GCP_ZONE}} \
          --machine-type={{.MACHINE_TYPE}} \
          --image-family={{.IMAGE_FAMILY}} \
          --image-project={{.IMAGE_PROJECT}} \
          --boot-disk-size=20GB \
          --boot-disk-type=pd-standard \
          --tags=edgeproxy \
          --metadata-from-file=startup-script=startup.sh

        echo "Instance created. Getting external IP..."
        gcloud compute instances describe {{.INSTANCE_NAME}} \
          --zone={{.GCP_ZONE}} \
          --format='get(networkInterfaces[0].accessConfigs[0].natIP)'

  gcp:vm:ssh:
    desc: SSH into the VM
    cmds:
      - gcloud compute ssh {{.INSTANCE_NAME}} --zone={{.GCP_ZONE}}

  gcp:vm:delete:
    desc: Delete VM instance
    cmds:
      - gcloud compute instances delete {{.INSTANCE_NAME}} --zone={{.GCP_ZONE}} --quiet
```

### Step-by-Step Creation

```bash
# 1. Verify GCP credentials
task gcp:check

# 2. Create firewall rules
task gcp:firewall:create

# 3. Create VM instance
task gcp:vm:create

# Output:
# Created [https://www.googleapis.com/compute/v1/projects/.../zones/asia-east2-a/instances/edgeproxy-pop-hkg]
# External IP: 34.92.xxx.xxx
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

### Deploy to GCP VM

```bash
# Copy binary and routing database to VM
gcloud compute scp target/release/edge-proxy edgeproxy-pop-hkg:/tmp/ --zone=asia-east2-a
gcloud compute scp routing.db edgeproxy-pop-hkg:/tmp/ --zone=asia-east2-a

# SSH and setup on VM
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="
  sudo mkdir -p /opt/edgeproxy
  sudo mv /tmp/edge-proxy /opt/edgeproxy/
  sudo mv /tmp/routing.db /opt/edgeproxy/
  sudo chmod +x /opt/edgeproxy/edge-proxy
"
```

### Create systemd Service

```bash
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="
cat | sudo tee /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy TCP Proxy
After=network.target

[Service]
Type=simple
User=root
WorkingDirectory=/opt/edgeproxy
Environment=EDGEPROXY_REGION=ap
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
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="sudo systemctl status edgeproxy"

# Check logs
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a --command="sudo journalctl -u edgeproxy -n 20"

# Test connectivity (from local machine)
nc -zv <EXTERNAL_IP> 8080
```

---

## WireGuard Configuration

### Generate Keys for HKG POP

```bash
# Generate keys for GCP Hong Kong
wg genkey > wireguard/hkg-private.key
cat wireguard/hkg-private.key | wg pubkey > wireguard/hkg-public.key

# Display keys
echo "Private: $(cat wireguard/hkg-private.key)"
echo "Public: $(cat wireguard/hkg-public.key)"
```

### GCP Server Config (Client Mode)

The GCP instance connects to the central EC2 server:

```ini
# /etc/wireguard/wg0.conf
[Interface]
PrivateKey = <hkg-private-key>
Address = 10.50.5.1/24

[Peer]
# EC2 Ireland (Central Server)
PublicKey = <ec2-public-key>
Endpoint = 54.171.48.207:51820
AllowedIPs = 10.50.0.0/16
PersistentKeepalive = 25
```

### Update EC2 Central Server

Add the HKG peer to the EC2 WireGuard config:

```ini
# Add to /etc/wireguard/wg0.conf on EC2

# HKG - Hong Kong (Asia)
[Peer]
PublicKey = <hkg-public-key>
AllowedIPs = 10.50.5.1/32
```

Then reload:

```bash
# On EC2
sudo wg syncconf wg0 <(wg-quick strip wg0)

# Verify
sudo wg show
```

---

## Network Topology

### Updated IP Allocation

| Region | Code | WireGuard IP | Location | Provider |
|--------|------|--------------|----------|----------|
| **Central** | EC2 | 10.50.0.1 | Ireland | AWS |
| South America | GRU | 10.50.1.1 | Sao Paulo | Fly.io |
| North America | IAD | 10.50.2.1 | Virginia | Fly.io |
| Europe | LHR | 10.50.3.1 | London | Fly.io |
| Asia Pacific | NRT | 10.50.4.1 | Tokyo | Fly.io |
| Asia Pacific | SIN | 10.50.4.2 | Singapore | Fly.io |
| **Asia (New)** | **HKG** | **10.50.5.1** | **Hong Kong** | **GCP** |

---

## Testing Geo-Routing from China

### Using VPN to Simulate China Location

```bash
# Connect to a China VPN server (e.g., Shenzhen, Shanghai, Beijing)

# Test geo-routing
curl -s http://34.92.xxx.xxx:8080/api/info | jq .

# Expected response:
{
  "region": "hkg",
  "region_name": "Hong Kong",
  "backend": "hkg-node-1",
  "client_country": "CN",
  "latency_ms": 15
}
```

### Latency Test

```bash
# Quick latency test from China VPN
for i in {1..10}; do
  curl -w "%{time_total}s\n" -o /dev/null -s http://34.92.xxx.xxx:8080/api/latency
done
```

### Expected Performance

| Client Location | Expected Backend | Expected Latency |
|-----------------|------------------|------------------|
| China (Shenzhen) | HKG | 10-30ms |
| China (Beijing) | HKG | 30-50ms |
| Japan (Tokyo) | NRT or HKG | 40-60ms |
| Singapore | SIN or HKG | 30-50ms |

---

## Firewall Rules

| Rule Name | Port | Protocol | Source | Description |
|-----------|------|----------|--------|-------------|
| edgeproxy-allow-ssh | 22 | TCP | Your IP | SSH access |
| edgeproxy-allow-proxy | 8080 | TCP | 0.0.0.0/0 | edgeProxy TCP |
| edgeproxy-allow-wireguard | 51820 | UDP | 0.0.0.0/0 | WireGuard |

### Restricting SSH

```bash
# Get your IP
MY_IP=$(curl -s ifconfig.me)

# Update firewall rule
gcloud compute firewall-rules update edgeproxy-allow-ssh \
  --source-ranges ${MY_IP}/32
```

---

## Monitoring

### Check WireGuard Status

```bash
# SSH into VM
gcloud compute ssh edgeproxy-pop-hkg --zone=asia-east2-a

# Show WireGuard status
sudo wg show

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

## Cost Estimation

| Resource | Specification | Monthly Cost (USD) |
|----------|---------------|-------------------|
| VM Instance | e2-micro (2 vCPU, 1GB) | ~$6.11 |
| Boot Disk | 20GB Standard | ~$0.80 |
| Network Egress | 10GB/month | ~$1.20 |
| **Total** | | **~$8/month** |

:::tip Free Tier
GCP offers 1 e2-micro instance free per month in us-west1, us-central1, and us-east1. Hong Kong is not in free tier, but costs are minimal.
:::

---

## Troubleshooting

### WireGuard Not Connecting

```bash
# Check interface
ip addr show wg0

# Check if port is open
sudo netstat -ulnp | grep 51820

# Test connectivity to EC2
ping 10.50.0.1
```

### VM Not Reachable

```bash
# Check firewall rules
gcloud compute firewall-rules list --filter="name~edgeproxy"

# Check VM status
gcloud compute instances describe edgeproxy-pop-hkg --zone=asia-east2-a

# Check serial console output
gcloud compute instances get-serial-port-output edgeproxy-pop-hkg --zone=asia-east2-a
```

---

## Next Steps

- [AWS EC2 Deployment](./aws) - Central POP in Ireland
- [Fly.io Deployment](./flyio) - Global backend deployment
- [Benchmarks](../benchmark) - Performance testing
