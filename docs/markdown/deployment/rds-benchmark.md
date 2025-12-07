---
sidebar_position: 7
---

# Benchmark

This guide documents the complete setup and results of benchmarking PostgreSQL RDS access from 10 global Fly.io regions through WireGuard overlay network.

## Overview

The benchmark measures INSERT and SELECT latencies from Fly.io edge nodes to AWS RDS PostgreSQL in Ireland (eu-west-1), routing through an EC2 WireGuard hub.

![RDS Benchmark Architecture](/img/rds-benchmark-architecture.svg)

## Benchmark Results

| Flag | Region | Location | Insert (ms) | Read (ms) | Rank |
|:----:|:------:|----------|:-----------:|:---------:|:----:|
| 🇬🇧 | lhr | London | **25.39** | **11.89** | 1 |
| 🇫🇷 | cdg | Paris | 37.61 | 18.48 | 2 |
| 🇩🇪 | fra | Frankfurt | 54.17 | 26.69 | 3 |
| 🇺🇸 | iad | Virginia | 173.16 | 86.10 | 4 |
| 🇺🇸 | ord | Chicago | 244.20 | 121.68 | 5 |
| 🇺🇸 | lax | Los Angeles | 285.52 | 138.19 | 6 |
| 🇸🇬 | sin | Singapore | 332.65 | 165.96 | 7 |
| 🇧🇷 | gru | Sao Paulo | 396.09 | 197.72 | 8 |
| 🇯🇵 | nrt | Tokyo | 523.35 | 261.36 | 9 |
| 🇦🇺 | syd | Sydney | 538.58 | 268.85 | 10 |

### Visual Results

![RDS Benchmark Results](/img/rds-benchmark-results.svg)

### Key Insights

- **London (LHR)** has the best latency (~25ms INSERT, ~12ms READ) - closest to RDS in Ireland
- **European regions** (LHR, CDG, FRA) dominate top 3 due to geographic proximity
- **US East Coast** (IAD) ~173ms - crossing the Atlantic
- **APAC regions** (NRT, SYD) have highest latencies (~520-540ms) - maximum geographic distance

---

## Step 1: Fly.io Backend Setup

### 1.1 Create the Go Backend

Create `fly-backend/main.go`:

```go
package main

import (
    "database/sql"
    "encoding/json"
    "fmt"
    "net/http"
    "os"
    "strconv"
    "time"

    _ "github.com/lib/pq"
)

var (
    region   string
    hostname string
    db       *sql.DB
)

func main() {
    region = os.Getenv("FLY_REGION")
    if region == "" {
        region = "local"
    }

    hostname = os.Getenv("FLY_ALLOC_ID")
    if hostname == "" {
        hostname, _ = os.Hostname()
    }
    if len(hostname) > 8 {
        hostname = hostname[:8]
    }

    port := os.Getenv("PORT")
    if port == "" {
        port = "8080"
    }

    // Initialize database
    initDB()

    // RDS Benchmark endpoints
    http.HandleFunc("/api/rds/benchmark", handleRDSBenchmark)
    http.HandleFunc("/api/rds/health", handleRDSHealth)
    http.HandleFunc("/api/info", handleInfo)

    fmt.Printf("Backend running in region [%s] on port %s\n", region, port)
    http.ListenAndServe(":"+port, nil)
}

func getEnv(key, defaultValue string) string {
    if value := os.Getenv(key); value != "" {
        return value
    }
    return defaultValue
}

func initDB() {
    dbHost := getEnv("DB_HOST", "")
    if dbHost == "" {
        fmt.Println("DB_HOST not set, RDS benchmark disabled")
        return
    }

    dbPort := getEnv("DB_PORT", "5432")
    dbUser := getEnv("DB_USER", "postgres")
    dbPassword := getEnv("DB_PASSWORD", "")
    dbName := getEnv("DB_NAME", "contacts")

    connStr := fmt.Sprintf("host=%s port=%s user=%s password=%s dbname=%s sslmode=disable",
        dbHost, dbPort, dbUser, dbPassword, dbName)

    var err error
    db, err = sql.Open("postgres", connStr)
    if err != nil {
        fmt.Printf("Failed to open database: %v\n", err)
        return
    }

    db.SetMaxOpenConns(10)
    db.SetMaxIdleConns(5)
    db.SetConnMaxLifetime(time.Minute * 5)

    if err := db.Ping(); err != nil {
        fmt.Printf("Failed to ping database: %v\n", err)
        db = nil
        return
    }

    fmt.Printf("Database connected: %s\n", dbHost)
}

func handleRDSBenchmark(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")
    w.Header().Set("X-Fly-Region", region)

    dbHost := getEnv("DB_HOST", "not configured")

    if db == nil {
        json.NewEncoder(w).Encode(map[string]interface{}{
            "error":   "Database not configured",
            "region":  region,
            "db_host": dbHost,
        })
        return
    }

    iterations := 10
    if iter := r.URL.Query().Get("iterations"); iter != "" {
        if n, err := strconv.Atoi(iter); err == nil && n > 0 && n <= 100 {
            iterations = n
        }
    }

    readLatencies := make([]float64, iterations)
    insertLatencies := make([]float64, iterations)

    // Run READ benchmarks (SELECT COUNT)
    for i := 0; i < iterations; i++ {
        start := time.Now()
        var count int
        db.QueryRow("SELECT COUNT(*) FROM contacts").Scan(&count)
        readLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
    }

    // Run INSERT benchmarks
    for i := 0; i < iterations; i++ {
        start := time.Now()
        name := fmt.Sprintf("Bench-%s-%d-%d", region, time.Now().UnixNano(), i)
        email := fmt.Sprintf("bench-%d@test.local", time.Now().UnixNano())
        db.Exec(`INSERT INTO contacts (name, email, notes) VALUES ($1, $2, $3)`,
            name, email, "Benchmark")
        insertLatencies[i] = float64(time.Since(start).Microseconds()) / 1000.0
    }

    // Calculate statistics
    calcStats := func(latencies []float64) (avg, min, max float64) {
        if len(latencies) == 0 {
            return 0, 0, 0
        }
        min = latencies[0]
        max = latencies[0]
        var sum float64
        for _, l := range latencies {
            sum += l
            if l < min {
                min = l
            }
            if l > max {
                max = l
            }
        }
        avg = sum / float64(len(latencies))
        return
    }

    readAvg, readMin, readMax := calcStats(readLatencies)
    insertAvg, insertMin, insertMax := calcStats(insertLatencies)

    result := map[string]interface{}{
        "region":           region,
        "db_host":          dbHost,
        "iterations":       iterations,
        "read_avg_ms":      readAvg,
        "read_min_ms":      readMin,
        "read_max_ms":      readMax,
        "insert_avg_ms":    insertAvg,
        "insert_min_ms":    insertMin,
        "insert_max_ms":    insertMax,
        "read_latencies":   readLatencies,
        "insert_latencies": insertLatencies,
        "timestamp":        time.Now().UTC().Format(time.RFC3339),
    }

    json.NewEncoder(w).Encode(result)
}

func handleRDSHealth(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")

    result := map[string]interface{}{
        "region":  region,
        "db_host": getEnv("DB_HOST", "not configured"),
    }

    if db == nil {
        result["status"] = "disabled"
    } else if err := db.Ping(); err != nil {
        result["status"] = "error"
        result["message"] = err.Error()
    } else {
        result["status"] = "connected"
    }

    json.NewEncoder(w).Encode(result)
}

func handleInfo(w http.ResponseWriter, r *http.Request) {
    w.Header().Set("Content-Type", "application/json")

    json.NewEncoder(w).Encode(map[string]interface{}{
        "region":   region,
        "hostname": hostname,
    })
}
```

### 1.2 Create go.mod

```go
module fly-backend

go 1.21

require github.com/lib/pq v1.10.9
```

### 1.3 Create Dockerfile

```dockerfile
FROM golang:1.21-alpine AS builder

WORKDIR /app
COPY go.mod go.sum ./
RUN go mod download
COPY main.go .
RUN CGO_ENABLED=0 GOOS=linux go build -ldflags="-s -w" -o backend main.go

FROM alpine:3.19
RUN apk --no-cache add ca-certificates wireguard-tools iptables ip6tables iproute2 bash
WORKDIR /app
COPY --from=builder /app/backend .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh
CMD ["./entrypoint.sh"]
```

### 1.4 Create entrypoint.sh (WireGuard + Backend)

```bash
#!/bin/bash
set -e

echo "=== Starting WireGuard + Backend ==="
echo "FLY_REGION: ${FLY_REGION}"

# EC2 endpoint and public key (hub)
EC2_ENDPOINT="54.171.48.207:51820"
EC2_PUBKEY="bzM6rw/efq+75VGhBgkCRChDnKfFlXQY560ejhvKCQY="

# Map region to WireGuard IP and private key
case "${FLY_REGION}" in
  gru)
    WG_IP="10.50.1.1/32"
    WG_PRIVATE="YOUR_GRU_PRIVATE_KEY"
    ;;
  iad)
    WG_IP="10.50.2.1/32"
    WG_PRIVATE="YOUR_IAD_PRIVATE_KEY"
    ;;
  ord)
    WG_IP="10.50.2.2/32"
    WG_PRIVATE="YOUR_ORD_PRIVATE_KEY"
    ;;
  lax)
    WG_IP="10.50.2.3/32"
    WG_PRIVATE="YOUR_LAX_PRIVATE_KEY"
    ;;
  lhr)
    WG_IP="10.50.3.1/32"
    WG_PRIVATE="YOUR_LHR_PRIVATE_KEY"
    ;;
  fra)
    WG_IP="10.50.3.2/32"
    WG_PRIVATE="YOUR_FRA_PRIVATE_KEY"
    ;;
  cdg)
    WG_IP="10.50.3.3/32"
    WG_PRIVATE="YOUR_CDG_PRIVATE_KEY"
    ;;
  nrt)
    WG_IP="10.50.4.1/32"
    WG_PRIVATE="YOUR_NRT_PRIVATE_KEY"
    ;;
  sin)
    WG_IP="10.50.4.2/32"
    WG_PRIVATE="YOUR_SIN_PRIVATE_KEY"
    ;;
  syd)
    WG_IP="10.50.4.3/32"
    WG_PRIVATE="YOUR_SYD_PRIVATE_KEY"
    ;;
  *)
    echo "Unknown region: ${FLY_REGION}, skipping WireGuard"
    exec ./backend
    ;;
esac

echo "Configuring WireGuard with IP: ${WG_IP}"

# Create WireGuard configuration
mkdir -p /etc/wireguard

cat > /etc/wireguard/wg0.conf << WGEOF
[Interface]
PrivateKey = ${WG_PRIVATE}
Address = ${WG_IP}

[Peer]
# EC2 Ireland (hub)
PublicKey = ${EC2_PUBKEY}
Endpoint = ${EC2_ENDPOINT}
AllowedIPs = 10.50.0.0/24, 10.50.1.0/24, 10.50.2.0/24, 10.50.3.0/24, 10.50.4.0/24
PersistentKeepalive = 25
WGEOF

# Start WireGuard
echo "Starting WireGuard interface..."
wg-quick up wg0 || echo "WireGuard failed (might need NET_ADMIN capability)"

# Show status
wg show || true

echo "Starting backend server..."
exec ./backend
```

### 1.5 Create fly.toml

```toml
app = 'edgeproxy-backend'
primary_region = 'gru'

[build]

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

---

## Step 2: Deploy to Fly.io

### 2.1 Create the app

```bash
fly apps create edgeproxy-backend
```

### 2.2 Set database secrets

```bash
fly secrets set \
  DB_HOST=10.50.0.1 \
  DB_PORT=5432 \
  DB_USER=contacts_user \
  DB_PASSWORD=your_password \
  DB_NAME=contacts \
  -a edgeproxy-backend
```

### 2.3 Deploy to all regions

```bash
# Deploy the app
fly deploy

# Scale to all 10 regions
fly scale count 1 --region gru,iad,ord,lax,lhr,fra,cdg,nrt,sin,syd -a edgeproxy-backend
```

---

## Step 3: AWS RDS Setup

### 3.1 Create RDS Instance

```bash
aws rds create-db-instance \
  --db-instance-identifier edgeproxy-db \
  --db-instance-class db.t3.micro \
  --engine postgres \
  --engine-version 15.4 \
  --master-username postgres \
  --master-user-password YOUR_PASSWORD \
  --allocated-storage 20 \
  --vpc-security-group-ids sg-xxxxxxxx \
  --availability-zone eu-west-1a \
  --publicly-accessible \
  --no-multi-az
```

### 3.2 Disable SSL Requirement

Create a custom parameter group:

```bash
aws rds create-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --db-parameter-group-family postgres15 \
  --description "Disable SSL for WireGuard connections"

aws rds modify-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --parameters "ParameterName=rds.force_ssl,ParameterValue=0,ApplyMethod=pending-reboot"

aws rds modify-db-instance \
  --db-instance-identifier edgeproxy-db \
  --db-parameter-group-name edgeproxy-nossl \
  --apply-immediately

aws rds reboot-db-instance --db-instance-identifier edgeproxy-db
```

### 3.3 Create Database and Table

```sql
CREATE DATABASE contacts;

\c contacts

CREATE TABLE contacts (
    id SERIAL PRIMARY KEY,
    name VARCHAR(255) NOT NULL,
    email VARCHAR(255),
    notes TEXT,
    created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE USER contacts_user WITH PASSWORD 'your_password';
GRANT ALL PRIVILEGES ON DATABASE contacts TO contacts_user;
GRANT ALL PRIVILEGES ON ALL TABLES IN SCHEMA public TO contacts_user;
GRANT USAGE, SELECT ON ALL SEQUENCES IN SCHEMA public TO contacts_user;
```

---

## Step 4: EC2 WireGuard Hub Configuration

### 4.1 EC2 User Data (cloud-init)

```bash
#!/bin/bash
set -e

# Install WireGuard
apt-get update && apt-get install -y wireguard

# Enable IP forwarding
echo "net.ipv4.ip_forward = 1" >> /etc/sysctl.conf
sysctl -p

# Generate WireGuard keys
wg genkey | tee /etc/wireguard/privatekey | wg pubkey > /etc/wireguard/publickey
PRIVATE_KEY=$(cat /etc/wireguard/privatekey)

# Create WireGuard config
cat > /etc/wireguard/wg0.conf << 'EOF'
[Interface]
PrivateKey = PRIVATE_KEY_HERE
Address = 10.50.0.1/24
ListenPort = 51820
PostUp = iptables -t nat -A POSTROUTING -o ens5 -j MASQUERADE
PostDown = iptables -t nat -D POSTROUTING -o ens5 -j MASQUERADE

# Fly.io peers (add after generating their keys)
[Peer]
# fly-gru-1
PublicKey = FLY_GRU_PUBKEY
AllowedIPs = 10.50.1.1/32

[Peer]
# fly-iad-1
PublicKey = FLY_IAD_PUBKEY
AllowedIPs = 10.50.2.1/32

# ... add all 10 regions
EOF

# Replace placeholder
sed -i "s|PRIVATE_KEY_HERE|$PRIVATE_KEY|" /etc/wireguard/wg0.conf

# Start WireGuard
systemctl enable wg-quick@wg0
systemctl start wg-quick@wg0

# DNAT for RDS access (route 10.50.0.1:5432 to RDS)
RDS_IP="172.31.x.x"  # Your RDS private IP
iptables -t nat -A PREROUTING -d 10.50.0.1 -p tcp --dport 5432 -j DNAT --to-destination $RDS_IP:5432
iptables -t nat -A POSTROUTING -d $RDS_IP -p tcp --dport 5432 -j MASQUERADE
```

### 4.2 Security Group Rules

**EC2 Security Group:**
- Inbound: UDP 51820 from 0.0.0.0/0 (WireGuard)
- Inbound: TCP 22 from your IP (SSH)
- Outbound: All traffic

**RDS Security Group:**
- Inbound: TCP 5432 from EC2 Security Group
- Inbound: TCP 5432 from EC2 private IP

---

## Step 5: Running the Benchmark

### 5.1 Test from EC2 (via WireGuard)

```bash
# Test each backend directly
for backend in "gru:10.50.1.1" "iad:10.50.2.1" "lhr:10.50.3.1"; do
  region=$(echo $backend | cut -d: -f1)
  ip=$(echo $backend | cut -d: -f2)
  echo "=== $region ==="
  curl -s http://$ip:8080/api/rds/benchmark | jq '{region, insert_avg_ms, read_avg_ms}'
done
```

### 5.2 Test via edgeProxy (geo-routing)

```bash
# The edge-proxy will route based on client IP
curl -s http://54.171.48.207:8080/api/rds/benchmark | jq .
```

### 5.3 Full Benchmark Script

```bash
#!/bin/bash
echo "=== RDS Benchmark: Fly.io → AWS RDS Ireland ==="
echo ""
printf "| %-4s | %-6s | %-13s | %-11s | %-9s |\n" "Flag" "Region" "Location" "Insert (ms)" "Read (ms)"
echo "|------|--------|---------------|-------------|-----------|"

for backend in \
  "🇧🇷:gru:10.50.1.1:Sao Paulo" \
  "🇺🇸:iad:10.50.2.1:Virginia" \
  "🇺🇸:ord:10.50.2.2:Chicago" \
  "🇺🇸:lax:10.50.2.3:Los Angeles" \
  "🇬🇧:lhr:10.50.3.1:London" \
  "🇩🇪:fra:10.50.3.2:Frankfurt" \
  "🇫🇷:cdg:10.50.3.3:Paris" \
  "🇯🇵:nrt:10.50.4.1:Tokyo" \
  "🇸🇬:sin:10.50.4.2:Singapore" \
  "🇦🇺:syd:10.50.4.3:Sydney"
do
  flag=$(echo $backend | cut -d: -f1)
  region=$(echo $backend | cut -d: -f2)
  ip=$(echo $backend | cut -d: -f3)
  location=$(echo $backend | cut -d: -f4)

  result=$(curl -s --connect-timeout 10 http://$ip:8080/api/rds/benchmark 2>/dev/null)

  if [ -n "$result" ]; then
    insert=$(echo $result | jq -r '.insert_avg_ms' | xargs printf "%.2f")
    read=$(echo $result | jq -r '.read_min_ms' | xargs printf "%.2f")
    printf "| %-4s | %-6s | %-13s | %11s | %9s |\n" "$flag" "$region" "$location" "$insert" "$read"
  else
    printf "| %-4s | %-6s | %-13s | %11s | %9s |\n" "$flag" "$region" "$location" "TIMEOUT" "TIMEOUT"
  fi
done
```

---

## API Reference

### GET /api/rds/benchmark

Runs INSERT and SELECT benchmarks against the configured RDS database.

**Query Parameters:**
- `iterations` (optional): Number of iterations (1-100, default: 10)

**Response:**
```json
{
  "region": "lhr",
  "db_host": "10.50.0.1",
  "iterations": 10,
  "read_avg_ms": 18.72,
  "read_min_ms": 11.89,
  "read_max_ms": 65.45,
  "insert_avg_ms": 25.39,
  "insert_min_ms": 24.60,
  "insert_max_ms": 29.04,
  "read_latencies": [65.45, 12.10, 11.99, ...],
  "insert_latencies": [24.97, 25.62, 24.60, ...],
  "timestamp": "2025-12-07T15:48:02Z"
}
```

### GET /api/rds/health

Returns database connection status.

**Response:**
```json
{
  "region": "lhr",
  "db_host": "10.50.0.1",
  "status": "connected"
}
```

---

## Troubleshooting

### Issue: "no pg_hba.conf entry for host"

**Cause:** RDS requires SSL by default.

**Solution:** Disable SSL requirement:
```bash
aws rds modify-db-parameter-group \
  --db-parameter-group-name edgeproxy-nossl \
  --parameters "ParameterName=rds.force_ssl,ParameterValue=0,ApplyMethod=pending-reboot"
```

### Issue: Connection timeout from Fly.io

**Cause:** WireGuard not connecting to EC2 hub.

**Solution:**
1. Verify EC2 public key matches in entrypoint.sh
2. Check EC2 security group allows UDP 51820
3. Verify NAT rules on EC2:
```bash
iptables -t nat -L -n -v
```

### Issue: "Database not configured" response

**Cause:** DB_HOST secret not set.

**Solution:**
```bash
fly secrets set DB_HOST=10.50.0.1 -a edgeproxy-backend
```

---

## WireGuard IP Allocation

| Region | Fly.io WG IP | Purpose |
|--------|-------------|---------|
| EC2 Hub | 10.50.0.1 | WireGuard hub + NAT to RDS |
| gru | 10.50.1.1 | South America |
| iad | 10.50.2.1 | US East |
| ord | 10.50.2.2 | US Central |
| lax | 10.50.2.3 | US West |
| lhr | 10.50.3.1 | Europe (UK) |
| fra | 10.50.3.2 | Europe (Germany) |
| cdg | 10.50.3.3 | Europe (France) |
| nrt | 10.50.4.1 | Asia (Japan) |
| sin | 10.50.4.2 | Asia (Singapore) |
| syd | 10.50.4.3 | Oceania (Australia) |

---

## Performance Optimization Tips

1. **Use connection pooling**: The Go backend uses `SetMaxOpenConns(10)` and `SetMaxIdleConns(5)`

2. **Persistent connections**: WireGuard `PersistentKeepalive = 25` keeps tunnels warm

3. **Place RDS in same region as hub**: EC2 and RDS in eu-west-1 minimizes internal latency

4. **Consider read replicas**: For read-heavy workloads, deploy RDS read replicas in other regions
