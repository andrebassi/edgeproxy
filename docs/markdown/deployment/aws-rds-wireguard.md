---
sidebar_position: 2
---

# RDS via WireGuard

This guide covers deploying a PostgreSQL database on AWS RDS and accessing it securely through a WireGuard overlay network from Fly.io edge applications.

:::info Overview
This architecture enables edge applications on Fly.io to securely access a centralized AWS RDS PostgreSQL database through an encrypted WireGuard tunnel, using an EC2 instance as a NAT gateway.
:::

---

## Architecture

![RDS WireGuard Architecture](/img/rds-wireguard-architecture.svg)

### Components

| Component | Type | WireGuard IP | Public/Private IP | Role |
|-----------|------|--------------|-------------------|------|
| **Fly.io App** | Container | 10.50.3.10/32 | dynamic | Go application (contacts-api) |
| **EC2 Hub** | t3.micro | 10.50.0.1/24 | 34.240.78.199 | WireGuard gateway + NAT |
| **RDS PostgreSQL** | db.t3.micro | - | 172.31.3.134 | Database |

### Ports

| Service | Port | Protocol | Description |
|---------|------|----------|-------------|
| WireGuard | 51820 | UDP | Encrypted VPN tunnel |
| PostgreSQL | 5432 | TCP | Database connection (via NAT) |
| HTTP API | 8080 | TCP | Application REST API |

---

## Traffic Flow

![Traffic Flow](/img/rds-wireguard-traffic-flow.svg)

### Step by Step

1. **App connects to `10.50.0.1:5432`** - Go application uses `DB_HOST=10.50.0.1`
2. **Kernel routes via wg0** - Packets to `10.50.0.0/24` go through WireGuard interface
3. **Encrypted UDP tunnel** - WireGuard encapsulates and sends to EC2 (`34.240.78.199:51820`)
4. **EC2 receives and decrypts** - wg0 interface receives the original packet
5. **iptables DNAT** - Rewrites destination from `10.50.0.1:5432` to `172.31.3.134:5432`
6. **iptables MASQUERADE** - Rewrites source from `10.50.3.10` to `172.31.18.19` (EC2 IP)
7. **RDS processes query** - Database sees request coming from EC2
8. **Response returns** - Reverse path through NAT and WireGuard

---

## iptables NAT Routing

![iptables NAT](/img/rds-wireguard-iptables.svg)

### How NAT Works

The EC2 Hub acts as a gateway between the WireGuard network (10.50.x.x) and the AWS VPC (172.31.x.x). This is done through two iptables rules:

#### 1. DNAT (Destination NAT) - PREROUTING

```bash
iptables -t nat -A PREROUTING -i wg0 -p tcp --dport 5432 \
  -j DNAT --to-destination 172.31.3.134:5432
```

**What it does:**
- Intercepts TCP packets arriving on `wg0` interface destined to port 5432
- Rewrites destination address from `10.50.0.1` to `172.31.3.134` (RDS IP)
- Packet can now be routed to RDS in the VPC

#### 2. SNAT (Source NAT) - POSTROUTING with MASQUERADE

```bash
iptables -t nat -A POSTROUTING -d 172.31.3.134 -p tcp --dport 5432 \
  -j MASQUERADE
```

**What it does:**
- Intercepts packets going to RDS (172.31.3.134:5432)
- Rewrites source address from `10.50.3.10` to `172.31.18.19` (EC2 private IP)
- RDS sees the request as coming from EC2, not from Fly.io
- Responses return to EC2, which forwards via WireGuard

#### 3. IP Forwarding

```bash
sysctl -w net.ipv4.ip_forward=1
```

**Prerequisite:** Enables the Linux kernel to route packets between interfaces (wg0 ↔ eth0).

#### 4. FORWARD Chain

```bash
iptables -A FORWARD -i wg0 -j ACCEPT
iptables -A FORWARD -o wg0 -j ACCEPT
```

**What it does:** Allows packets to be forwarded to/from the WireGuard interface.

### Packet Transformation

![Packet Transformation](/img/rds-wireguard-packet-transformation.svg)

---

## Step-by-Step Deployment

### Step 1: Create Security Group for RDS

```bash
# Create security group
aws ec2 create-security-group \
  --region eu-west-1 \
  --group-name edgeproxy-rds-sg \
  --description "Security group for edgeProxy RDS" \
  --vpc-id vpc-0af2bf5af1b4460f7

# Allow PostgreSQL (restrict in production)
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06ad37f4e3ef49d7c \
  --protocol tcp \
  --port 5432 \
  --cidr 0.0.0.0/0
```

### Step 2: Create DB Subnet Group

```bash
aws rds create-db-subnet-group \
  --region eu-west-1 \
  --db-subnet-group-name edgeproxy-subnet-group \
  --db-subnet-group-description "Subnet group for edgeProxy RDS" \
  --subnet-ids subnet-0e5a3518878e1e16d subnet-0ae5feb18dd1f0bb7 subnet-0c8b89f0384c4c3f8
```

### Step 3: Create RDS PostgreSQL

```bash
aws rds create-db-instance \
  --region eu-west-1 \
  --db-instance-identifier edgeproxy-contacts-db \
  --db-instance-class db.t3.micro \
  --engine postgres \
  --engine-version 15 \
  --master-username postgres \
  --master-user-password EdgeProxy2024 \
  --allocated-storage 20 \
  --storage-type gp2 \
  --db-name contacts \
  --vpc-security-group-ids sg-06ad37f4e3ef49d7c \
  --db-subnet-group-name edgeproxy-subnet-group \
  --publicly-accessible \
  --backup-retention-period 1 \
  --no-multi-az
```

### Step 4: Wait for RDS to be Available

```bash
# Check status (takes ~5-10 minutes)
aws rds describe-db-instances \
  --region eu-west-1 \
  --db-instance-identifier edgeproxy-contacts-db \
  --query 'DBInstances[0].[DBInstanceStatus,Endpoint.Address]' \
  --output text

# Output when ready:
# available    edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com
```

### Step 5: Generate WireGuard Keys

```bash
# EC2 Hub keys
wg genkey | tee ec2-wg-private.key | wg pubkey > ec2-wg-public.key
# Private: EHToyBXWXGOdh8dSngJnE9h6TGZ+VU6FLJDLnwq8Q2g=
# Public:  Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=

# Fly.io App keys
wg genkey | tee fly-wg-private.key | wg pubkey > fly-wg-public.key
# Private: QHgup1SNdoXT2X1SH8OoKbIhQfayX/7+lGCDNcmyPHY=
# Public:  92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
```

### Step 6: Create Security Group for EC2

```bash
# Create security group
aws ec2 create-security-group \
  --region eu-west-1 \
  --group-name edgeproxy-hub-sg \
  --description "Security group for edgeProxy WireGuard Hub" \
  --vpc-id vpc-0af2bf5af1b4460f7

# Allow SSH
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06b10b1222b9f530f \
  --protocol tcp \
  --port 22 \
  --cidr 0.0.0.0/0

# Allow WireGuard UDP
aws ec2 authorize-security-group-ingress \
  --region eu-west-1 \
  --group-id sg-06b10b1222b9f530f \
  --protocol udp \
  --port 51820 \
  --cidr 0.0.0.0/0
```

### Step 7: Create SSH Key Pair

```bash
aws ec2 create-key-pair \
  --region eu-west-1 \
  --key-name edgeproxy-hub \
  --query 'KeyMaterial' \
  --output text > edgeproxy-hub.pem

chmod 400 edgeproxy-hub.pem
```

### Step 8: User Data Script (Cloud-Init)

This script runs automatically when EC2 starts, configuring WireGuard and NAT:

```bash
#!/bin/bash
# =============================================================================
# edgeProxy Hub - EC2 Ireland - WireGuard + NAT to RDS
# Executed via cloud-init (User Data) - 100% non-interactive
# =============================================================================
set -e
exec > >(tee /var/log/userdata.log) 2>&1
echo "=== edgeProxy Hub Setup Started: $(date) ==="

# Disable interactive prompts
export DEBIAN_FRONTEND=noninteractive

# ============================================================================
# PACKAGE INSTALLATION
# ============================================================================
echo "=== Installing packages ==="
apt-get update -qq
apt-get install -y -qq wireguard dnsutils net-tools

# ============================================================================
# WIREGUARD CONFIGURATION
# ============================================================================
echo "=== Creating WireGuard configuration ==="
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << 'WGEOF'
[Interface]
PrivateKey = EHToyBXWXGOdh8dSngJnE9h6TGZ+VU6FLJDLnwq8Q2g=
Address = 10.50.0.1/24
ListenPort = 51820

# Enable IP forwarding and FORWARD rules
PostUp = sysctl -w net.ipv4.ip_forward=1
PostUp = iptables -A FORWARD -i wg0 -j ACCEPT
PostUp = iptables -A FORWARD -o wg0 -j ACCEPT
PostDown = iptables -D FORWARD -i wg0 -j ACCEPT
PostDown = iptables -D FORWARD -o wg0 -j ACCEPT

# Peer: Fly.io contacts-api (LHR region)
[Peer]
PublicKey = 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
AllowedIPs = 10.50.3.10/32
PersistentKeepalive = 25
WGEOF

chmod 600 /etc/wireguard/wg0.conf

echo "=== Starting WireGuard ==="
wg-quick up wg0
systemctl enable wg-quick@wg0

echo "=== WireGuard Status ==="
wg show

# ============================================================================
# NAT CONFIGURATION (iptables)
# ============================================================================
echo "=== Configuring NAT to RDS ==="

# Resolve RDS IP (follow CNAME and get A record)
RDS_IP=$(host edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com \
  | grep "has address" | awk '{print $4}' | head -1)
echo "RDS IP resolved: $RDS_IP"
echo "$RDS_IP" > /tmp/rds_ip.txt

if [ -z "$RDS_IP" ]; then
    echo "ERROR: Could not resolve RDS IP"
    exit 1
fi

# DNAT: Traffic from WireGuard to 10.50.0.1:5432 → RDS
# Packets arriving on wg0 destined to port 5432 are redirected to RDS
iptables -t nat -A PREROUTING -i wg0 -p tcp --dport 5432 \
  -j DNAT --to-destination ${RDS_IP}:5432

# SNAT/MASQUERADE: Ensures response returns through EC2
# Packets going to RDS have source rewritten to EC2 IP
iptables -t nat -A POSTROUTING -d ${RDS_IP} -p tcp --dport 5432 \
  -j MASQUERADE

# ============================================================================
# PERSIST RULES
# ============================================================================
mkdir -p /etc/iptables
iptables-save > /etc/iptables/rules.v4

# Create systemd service to restore rules on boot
cat > /etc/systemd/system/iptables-restore.service << 'SVCEOF'
[Unit]
Description=Restore iptables rules
After=network.target

[Service]
Type=oneshot
ExecStart=/sbin/iptables-restore /etc/iptables/rules.v4
RemainAfterExit=yes

[Install]
WantedBy=multi-user.target
SVCEOF

systemctl daemon-reload
systemctl enable iptables-restore.service

# ============================================================================
# VERIFICATION
# ============================================================================
echo "=== Testing RDS connectivity ==="
nc -zv ${RDS_IP} 5432 && echo "RDS connection OK" || echo "RDS connection failed"

echo "=== Final Status ==="
echo "EC2 WireGuard Public Key: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14="
echo "EC2 WireGuard IP: 10.50.0.1"
echo "EC2 Public IP: $(curl -s http://169.254.169.254/latest/meta-data/public-ipv4)"
echo "RDS NAT Target: ${RDS_IP}:5432"
echo ""
echo "NAT Rules:"
iptables -t nat -L -n
echo ""
wg show
echo "=== Setup Complete: $(date) ==="
```

### Step 9: Launch EC2

```bash
# Get latest Ubuntu 22.04 AMI
AMI_ID=$(aws ec2 describe-images \
  --region eu-west-1 \
  --owners 099720109477 \
  --filters "Name=name,Values=ubuntu/images/hvm-ssd/ubuntu-jammy-22.04-amd64-server-*" \
  --query 'sort_by(Images, &CreationDate)[-1].ImageId' \
  --output text)

# Launch instance with user-data
aws ec2 run-instances \
  --region eu-west-1 \
  --image-id $AMI_ID \
  --instance-type t3.micro \
  --key-name edgeproxy-hub \
  --security-group-ids sg-06b10b1222b9f530f \
  --subnet-id subnet-0e5a3518878e1e16d \
  --associate-public-ip-address \
  --user-data file://ec2-userdata.sh \
  --tag-specifications 'ResourceType=instance,Tags=[{Key=Name,Value=edgeproxy-hub}]'
```

### Step 10: Verify Setup

```bash
# Get public IP
aws ec2 describe-instances \
  --region eu-west-1 \
  --instance-ids i-079799a933a21ae5c \
  --query 'Reservations[0].Instances[0].PublicIpAddress' \
  --output text
# Output: 34.240.78.199

# Wait ~90s and check logs
ssh -i edgeproxy-hub.pem ubuntu@34.240.78.199 \
  "sudo tail -30 /var/log/userdata.log"
```

**Expected output:**

```
=== WireGuard Status ===
interface: wg0
  public key: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=
  private key: (hidden)
  listening port: 51820

peer: 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
  allowed ips: 10.50.3.10/32
  persistent keepalive: every 25 seconds

=== Configuring NAT to RDS ===
RDS IP resolved: 172.31.3.134
Connection to 172.31.3.134 5432 port [tcp/postgresql] succeeded!
RDS connection OK
=== Setup Complete ===
```

---

## Go Application (contacts-api)

### Project Structure

```
contacts-api/
├── main.go           # REST API server
├── seed.go           # Test data seeder
├── go.mod            # Go module
├── go.sum            # Dependencies checksum
├── Dockerfile        # Multi-stage build with WireGuard
├── entrypoint.sh     # WireGuard setup + app start
└── fly.toml          # Fly.io configuration
```

### main.go

Complete REST API with PostgreSQL:

```go
package main

import (
    "database/sql"
    "encoding/json"
    "fmt"
    "log"
    "net/http"
    "os"
    "time"

    _ "github.com/lib/pq"
)

type Contact struct {
    ID        int       `json:"id"`
    Name      string    `json:"name"`
    Email     string    `json:"email"`
    Phone     *string   `json:"phone,omitempty"`
    Company   *string   `json:"company,omitempty"`
    Notes     *string   `json:"notes,omitempty"`
    CreatedAt time.Time `json:"created_at"`
    UpdatedAt time.Time `json:"updated_at"`
}

var db *sql.DB

func getEnv(key, defaultValue string) string {
    if value := os.Getenv(key); value != "" {
        return value
    }
    return defaultValue
}

func initDB() error {
    dbHost := getEnv("DB_HOST", "localhost")
    dbPort := getEnv("DB_PORT", "5432")
    dbUser := getEnv("DB_USER", "postgres")
    dbPassword := getEnv("DB_PASSWORD", "")
    dbName := getEnv("DB_NAME", "contacts")

    connStr := fmt.Sprintf(
        "host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
        dbHost, dbPort, dbUser, dbPassword, dbName)

    var err error
    db, err = sql.Open("postgres", connStr)
    if err != nil {
        return err
    }

    db.SetMaxOpenConns(10)
    db.SetMaxIdleConns(5)
    db.SetConnMaxLifetime(time.Minute * 5)

    return db.Ping()
}

func healthHandler(w http.ResponseWriter, r *http.Request) {
    resp := map[string]string{
        "status":   "healthy",
        "database": "connected",
        "region":   getEnv("FLY_REGION", "local"),
        "db_host":  getEnv("DB_HOST", "localhost"),
    }

    if err := db.Ping(); err != nil {
        resp["status"] = "unhealthy"
        resp["database"] = err.Error()
    }

    w.Header().Set("Content-Type", "application/json")
    json.NewEncoder(w).Encode(resp)
}

// ... complete handlers in source code
```

### API Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/` | Service info |
| GET | `/health` | Health check with DB status |
| GET | `/stats` | Database statistics |
| GET | `/contacts` | List contacts (paginated) |
| GET | `/contacts/:id` | Get contact by ID |
| POST | `/contacts` | Create contact |
| PUT | `/contacts/:id` | Update contact |
| DELETE | `/contacts/:id` | Delete contact |
| GET | `/contacts/search/:query` | Search contacts |

### Dockerfile

```dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum* ./
RUN go mod download

COPY . .
RUN CGO_ENABLED=0 GOOS=linux go build -o contacts-api .

FROM alpine:3.19

# Install WireGuard and iptables
RUN apk add --no-cache ca-certificates wireguard-tools iptables

WORKDIR /app
COPY --from=builder /app/contacts-api .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

EXPOSE 8080

CMD ["./entrypoint.sh"]
```

### entrypoint.sh

```bash
#!/bin/sh
set -e

echo "=== Starting WireGuard ==="

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

wg-quick up wg0

echo "=== WireGuard Status ==="
wg show

echo "=== Testing connectivity to EC2 Hub ==="
ping -c 2 10.50.0.1 || echo "Ping failed"

echo "=== Starting contacts-api ==="
exec ./contacts-api
```

### fly.toml

```toml
app = 'edgeproxy-contacts-api'
primary_region = 'lhr'

[build]

[env]
  PORT = "8080"

[http_service]
  internal_port = 8080
  force_https = true
  auto_stop_machines = 'stop'
  auto_start_machines = true
  min_machines_running = 0
  processes = ['app']

[[vm]]
  memory = '256mb'
  cpu_kind = 'shared'
  cpus = 1
```

### Deploy to Fly.io

```bash
# Set secrets
fly secrets set \
  WG_PRIVATE_KEY="QHgup1SNdoXT2X1SH8OoKbIhQfayX/7+lGCDNcmyPHY=" \
  WG_ADDRESS="10.50.3.10/32" \
  WG_PEER_PUBLIC_KEY="Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=" \
  WG_PEER_ENDPOINT="34.240.78.199:51820" \
  DB_HOST="10.50.0.1" \
  DB_PORT="5432" \
  DB_USER="postgres" \
  DB_PASSWORD="EdgeProxy2024" \
  DB_NAME="contacts" \
  -a edgeproxy-contacts-api

# Deploy
fly deploy -a edgeproxy-contacts-api
```

---

## Verification

### WireGuard Logs on Fly.io

```bash
fly logs -a edgeproxy-contacts-api
```

**Expected output:**

```
=== Starting WireGuard ===
[#] ip link add wg0 type wireguard
[#] wg setconf wg0 /dev/fd/63
[#] ip -4 address add 10.50.3.10/32 dev wg0
[#] ip link set mtu 1340 up dev wg0
[#] ip -4 route add 10.50.0.0/24 dev wg0
=== WireGuard Status ===
interface: wg0
  public key: 92tt1di3bnUt9C5JGTW6CifmkebGmzAx5A4Rv+pXaCg=
  private key: (hidden)
  listening port: 46637

peer: Q9T4p88puHFgI8P8vLGjECvoXr85o5uncZQ2G35vE14=
  endpoint: 34.240.78.199:51820
  allowed ips: 10.50.0.0/24
  latest handshake: Now
  transfer: 92 B received, 180 B sent
  persistent keepalive: every 25 seconds

=== Testing connectivity to EC2 Hub ===
PING 10.50.0.1 (10.50.0.1): 56 data bytes
64 bytes from 10.50.0.1: seq=0 ttl=64 time=10.491 ms
64 bytes from 10.50.0.1: seq=1 ttl=64 time=10.704 ms

=== Starting contacts-api ===
2025/12/07 13:22:20 Initializing Contacts API...
2025/12/07 13:22:21 Database connected
2025/12/07 13:22:21 Schema initialized
2025/12/07 13:22:21 Server starting on port 8080
```

### Test Endpoints

```bash
# Health check
curl -s https://edgeproxy-contacts-api.fly.dev/health | jq .
```

```json
{
  "status": "healthy",
  "database": "connected",
  "region": "lhr",
  "db_host": "10.50.0.1"
}
```

```bash
# Statistics
curl -s https://edgeproxy-contacts-api.fly.dev/stats | jq .
```

```json
{
  "total_contacts": 500,
  "unique_companies": 33,
  "latest_contact": "2025-12-07T12:54:31.629798Z",
  "served_by": "lhr",
  "db_host": "10.50.0.1"
}
```

```bash
# List contacts
curl -s "https://edgeproxy-contacts-api.fly.dev/contacts?limit=3" | jq .
```

```json
{
  "contacts": [
    {
      "id": 115,
      "name": "Amanda Araujo",
      "email": "Amanda.Araujo@corporativo.com",
      "phone": "+55 11 93049-2680",
      "company": "Microservices Ltd",
      "notes": "Awaiting commercial proposal"
    }
  ],
  "limit": 3,
  "offset": 0,
  "served_by": "lhr",
  "total": 500
}
```

---

## Database Seeding

### seed.go

```go
// +build ignore

package main

import (
    "database/sql"
    "fmt"
    "log"
    "math/rand"
    "os"

    _ "github.com/lib/pq"
)

var firstNames = []string{
    "Ana", "Pedro", "Maria", "John", "Carla", "Lucas",
    "James", "Emma", "Hans", "François", "Marie",
}

var lastNames = []string{
    "Silva", "Santos", "Oliveira", "Smith", "Mueller", "Dubois",
}

var companies = []string{
    "TechCorp Brasil", "Cloud Nine Tech", "Kubernetes Masters",
    "AWS Partners", "DevSecOps Group",
}

func main() {
    connStr := fmt.Sprintf(
        "host=%s port=%s user=%s password=%s dbname=%s sslmode=require",
        os.Getenv("DB_HOST"), os.Getenv("DB_PORT"),
        os.Getenv("DB_USER"), os.Getenv("DB_PASSWORD"),
        os.Getenv("DB_NAME"))

    db, _ := sql.Open("postgres", connStr)
    defer db.Close()

    log.Println("Seeding 500 contacts...")

    for i := 0; i < 500; i++ {
        firstName := firstNames[rand.Intn(len(firstNames))]
        lastName := lastNames[rand.Intn(len(lastNames))]

        db.Exec(`INSERT INTO contacts (name, email, company) VALUES ($1, $2, $3)`,
            firstName+" "+lastName,
            fmt.Sprintf("%s.%s@email.com", firstName, lastName),
            companies[rand.Intn(len(companies))])
    }

    log.Println("Done!")
}
```

### Run Seeder

```bash
export DB_HOST="edgeproxy-contacts-db.cfy2y00ia7ys.eu-west-1.rds.amazonaws.com"
export DB_PORT="5432"
export DB_USER="postgres"
export DB_PASSWORD="EdgeProxy2024"
export DB_NAME="contacts"

go run seed.go
```

---

## Security

### Production Recommendations

1. **RDS Security Group**: Restrict to EC2 Hub only
   ```bash
   aws ec2 authorize-security-group-ingress \
     --group-id sg-06ad37f4e3ef49d7c \
     --protocol tcp --port 5432 \
     --source-group sg-06b10b1222b9f530f
   ```

2. **WireGuard Keys**: Store in AWS Secrets Manager

3. **RDS Encryption**: Enable encryption at rest
   ```bash
   --storage-encrypted --kms-key-id alias/aws/rds
   ```

4. **Private RDS**: Disable public access
   ```bash
   --no-publicly-accessible
   ```

---

## Cost Estimation (eu-west-1)

| Resource | Type | Monthly Cost (USD) |
|----------|------|-------------------|
| RDS PostgreSQL | db.t3.micro | ~$15 |
| EC2 Hub | t3.micro | ~$8 |
| EBS Storage | 20GB gp2 | ~$2 |
| Data Transfer | ~10GB | ~$1 |
| **Total** | | **~$26/month** |

---

## Troubleshooting

### WireGuard Handshake Not Happening

```bash
# On EC2 Hub
sudo wg show

# Check:
# 1. Security group allows UDP 51820
# 2. Fly.io app is running
# 3. Public keys match on both sides
```

### Database Connection Fails

```bash
# On EC2 Hub
nc -zv 172.31.3.134 5432

# Check NAT rules
sudo iptables -t nat -L -n

# Check RDS security group allows EC2
```

### Fly.io App Crashes

```bash
fly logs -a edgeproxy-contacts-api

# Common issues:
# - Missing secrets (WG_PRIVATE_KEY, DB_HOST, etc.)
# - Invalid WireGuard config
# - RDS not reachable (check NAT)
```

---

## Related Documentation

- [WireGuard Overlay Network](../wireguard.md)
- [AWS EC2 Deployment](./aws.md)
- [Fly.io Deployment](./flyio.md)
- [Architecture Overview](../architecture.md)

---

## Summary

This architecture provides:

- **Secure Access**: Database traffic encrypted via WireGuard
- **Edge Performance**: App runs close to users (Fly.io LHR)
- **Centralized Data**: Single RDS instance in AWS Ireland
- **Auto-scaling**: Fly.io machines scale to zero when idle
- **Low Cost**: ~$26/month for complete infrastructure

The WireGuard tunnel ensures all database traffic is encrypted and routed through a controlled path, while the NAT gateway on EC2 provides seamless connectivity to the private RDS instance.
