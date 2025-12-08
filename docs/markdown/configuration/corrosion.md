---
sidebar_position: 6
---

# Distributed Control Plane (Corrosion)

Corrosion enables distributed SQLite replication across all POPs.

## Architecture

![Corrosion Architecture](/img/corrosion-architecture.svg)

## How It Works

When `EDGEPROXY_CORROSION_ENABLED=true`, edgeProxy **ignores** the local `EDGEPROXY_DB_PATH` and instead queries the Corrosion HTTP API for backend data. Corrosion handles all replication between POPs automatically.

![Corrosion Data Flow](/img/corrosion-data-flow.svg)

## Installation

### Option A: Native Installation (Debian/Ubuntu)

#### Step 1: Install Corrosion

```bash
# Download latest Corrosion release
CORROSION_VERSION="0.5.0"
curl -L -o /tmp/corrosion.tar.gz \
  "https://github.com/superfly/corrosion/releases/download/v${CORROSION_VERSION}/corrosion-x86_64-unknown-linux-gnu.tar.gz"

# Extract and install
sudo tar -xzf /tmp/corrosion.tar.gz -C /usr/local/bin/
sudo chmod +x /usr/local/bin/corrosion

# Verify installation
corrosion --version
```

#### Step 2: Create Corrosion Configuration

```bash
# Create directories
sudo mkdir -p /etc/corrosion /var/lib/corrosion

# Create configuration file
sudo tee /etc/corrosion/corrosion.toml << 'EOF'
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
# Bootstrap nodes (other POPs) - leave empty for first node
bootstrap = []

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
EOF
```

#### Step 3: Create Systemd Service for Corrosion

```bash
sudo tee /etc/systemd/system/corrosion.service << 'EOF'
[Unit]
Description=Corrosion - Distributed SQLite
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/corrosion agent -c /etc/corrosion/corrosion.toml
Restart=always
RestartSec=5
User=root

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable corrosion
sudo systemctl start corrosion

# Check status
sudo systemctl status corrosion
```

#### Step 4: Install edgeProxy

```bash
# Download edgeProxy (or build from source)
curl -L -o /tmp/edgeproxy.tar.gz \
  "https://github.com/andrebassi/edgeproxy/releases/latest/download/edgeproxy-linux-amd64.tar.gz"

# Extract and install
sudo tar -xzf /tmp/edgeproxy.tar.gz -C /usr/local/bin/
sudo chmod +x /usr/local/bin/edgeproxy

# Verify installation
edgeproxy --version
```

#### Step 5: Create Systemd Service for edgeProxy

```bash
sudo tee /etc/systemd/system/edgeproxy.service << 'EOF'
[Unit]
Description=edgeProxy - Geo-aware TCP Proxy
After=network.target corrosion.service
Requires=corrosion.service

[Service]
Type=simple
ExecStart=/usr/local/bin/edgeproxy
Restart=always
RestartSec=5
User=root

# Environment configuration
Environment=EDGEPROXY_REGION=sa
Environment=EDGEPROXY_LISTEN_ADDR=0.0.0.0:8080
Environment=EDGEPROXY_TLS_LISTEN_ADDR=0.0.0.0:8443
Environment=EDGEPROXY_API_LISTEN_ADDR=0.0.0.0:8081
Environment=EDGEPROXY_CORROSION_ENABLED=true
Environment=EDGEPROXY_CORROSION_API_URL=http://127.0.0.1:8090

[Install]
WantedBy=multi-user.target
EOF

# Enable and start
sudo systemctl daemon-reload
sudo systemctl enable edgeproxy
sudo systemctl start edgeproxy

# Check status
sudo systemctl status edgeproxy
```

#### Step 6: Initialize Schema and Verify

```bash
# Wait for Corrosion to be ready
sleep 2

# Create backends table
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "CREATE TABLE IF NOT EXISTS backends (id TEXT PRIMARY KEY, app TEXT, region TEXT, wg_ip TEXT, port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0)"
  }'

# Verify edgeProxy is connected
curl http://localhost:8081/health
```

#### Firewall Configuration (UFW)

```bash
# Allow edgeProxy ports
sudo ufw allow 8080/tcp   # TCP proxy
sudo ufw allow 8081/tcp   # Auto-Discovery API
sudo ufw allow 8443/tcp   # TLS proxy
sudo ufw allow 4001/tcp   # Corrosion gossip (only if multi-POP)
```

#### Logs

```bash
# View Corrosion logs
sudo journalctl -u corrosion -f

# View edgeProxy logs
sudo journalctl -u edgeproxy -f
```

---

### Option B: Docker Compose

Corrosion runs as a **sidecar container** alongside edgeProxy. Both containers share the same network, allowing edgeProxy to reach Corrosion via `http://corrosion:8090`.

### Step 1: Create Corrosion Configuration

```toml
# corrosion.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
# Bootstrap nodes (other POPs) - leave empty for first node
bootstrap = []

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### Step 2: Create Docker Compose

```yaml
# docker-compose.yml
version: '3.8'

services:
  edgeproxy:
    image: edgeproxy:latest
    ports:
      - "8080:8080"   # TCP proxy
      - "8081:8081"   # Auto-Discovery API
      - "8443:8443"   # TLS proxy
    environment:
      EDGEPROXY_REGION: sa
      EDGEPROXY_LISTEN_ADDR: 0.0.0.0:8080
      # Connect to Corrosion sidecar
      EDGEPROXY_CORROSION_ENABLED: "true"
      EDGEPROXY_CORROSION_API_URL: http://corrosion:8090
      EDGEPROXY_CORROSION_POLL_SECS: "5"
    depends_on:
      - corrosion
    networks:
      - edgeproxy-net

  corrosion:
    image: ghcr.io/superfly/corrosion:latest
    volumes:
      - ./corrosion.toml:/etc/corrosion/corrosion.toml:ro
      - corrosion-data:/var/lib/corrosion
    ports:
      - "4001:4001"   # Gossip (for other POPs)
      - "8090:8090"   # HTTP API (internal)
    command: ["/corrosion", "agent", "-c", "/etc/corrosion/corrosion.toml"]
    networks:
      - edgeproxy-net

networks:
  edgeproxy-net:
    driver: bridge

volumes:
  corrosion-data:
```

### Step 3: Start the Stack

```bash
# Start edgeProxy + Corrosion
docker-compose up -d

# Verify Corrosion is running
curl http://localhost:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT 1"}'

# Check edgeProxy logs
docker-compose logs -f edgeproxy
```

### Step 4: Initialize the Schema

```bash
# Create the backends table (once)
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "CREATE TABLE IF NOT EXISTS backends (id TEXT PRIMARY KEY, app TEXT, region TEXT, wg_ip TEXT, port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0)"
  }'
```

## Multi-POP Setup

For multiple POPs, each POP runs its own edgeProxy + Corrosion pair. Corrosion instances discover each other via the gossip protocol.

### POP SA (São Paulo) - First Node

```toml
# corrosion-sa.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = []  # First node, no bootstrap

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### POP US (Virginia) - Joins SA

```toml
# corrosion-us.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = ["pop-sa.example.com:4001"]  # Points to SA

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

### POP EU (Frankfurt) - Joins SA or US

```toml
# corrosion-eu.toml
[db]
path = "/var/lib/corrosion/state.db"

[cluster]
name = "edgeproxy"
bootstrap = ["pop-sa.example.com:4001", "pop-us.example.com:4001"]

[gossip]
addr = "0.0.0.0:4001"

[api]
addr = "0.0.0.0:8090"
```

:::info WireGuard Required
The gossip port (4001) must be accessible between POPs. Use WireGuard overlay network for secure communication.
:::

## Deployment Topology

![Corrosion Topology](/img/corrosion-topology.svg)

## Configuration Reference

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_CORROSION_ENABLED` | `false` | Enable Corrosion backend |
| `EDGEPROXY_CORROSION_API_URL` | `http://localhost:8090` | Corrosion HTTP API URL |
| `EDGEPROXY_CORROSION_POLL_SECS` | `5` | Polling interval for backend sync |

## Benefits

- **Real-time sync**: Changes propagate in ~100ms via gossip protocol
- **No manual intervention**: Automatic replication across all POPs
- **Partition tolerance**: Works during network splits (CRDT-based)
- **Single source of truth**: Register backend once, available everywhere

## Registering Backends

There are three ways to register backends, depending on your setup:

### Option 1: Auto-Discovery API (Recommended for Production)

:::tip Recommended
The Auto-Discovery API is the **simplest method for production**. Backends register themselves automatically via HTTP - no SQL required!
:::

```bash
# Backend registers itself (from the backend server)
curl -X POST http://localhost:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{
    "id": "sa-node-1",
    "app": "myapp",
    "region": "sa",
    "ip": "10.50.1.1",
    "port": 9000,
    "weight": 2,
    "soft_limit": 100,
    "hard_limit": 150
  }'

# Backend sends periodic heartbeat to stay healthy
curl -X POST http://localhost:8081/api/v1/heartbeat/sa-node-1
```

The backend automatically expires if it stops sending heartbeats. See [Auto-Discovery API](./auto-discovery-api) for details.

### Option 2: Corrosion SQL API (With Corrosion Enabled)

When using Corrosion, insert backends via the Corrosion HTTP API. The data replicates automatically to all POPs:

```bash
# Insert backend (on any POP - replicates to all)
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{
    "sql": "INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit) VALUES (\"sa-node-1\", \"myapp\", \"sa\", \"10.50.1.1\", 9000, 1, 2, 100, 150)"
  }'

# Update backend health
curl -X POST http://localhost:8090/v1/transactions \
  -H "Content-Type: application/json" \
  -d '{"sql": "UPDATE backends SET healthy=0 WHERE id=\"sa-node-1\""}'

# List all backends
curl -X POST http://localhost:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT * FROM backends WHERE healthy=1"}'
```

### Option 3: Local SQLite (Standalone Mode)

Without Corrosion, insert directly into `routing.db` on **each POP manually**:

```bash
# On each POP (no automatic replication!)
sqlite3 routing.db "INSERT INTO backends (id, app, region, wg_ip, port, healthy, weight, soft_limit, hard_limit)
VALUES ('sa-node-1', 'myapp', 'sa', '10.50.1.1', 9000, 1, 2, 100, 150);"
```

:::warning
In standalone mode, you must manually insert backends on each POP. This is only recommended for development/testing.
:::

## Comparison

| Method | Replication | Complexity | Use Case |
|--------|-------------|------------|----------|
| Auto-Discovery API | Depends on storage | Low | Production (recommended) |
| Corrosion SQL API | Automatic | Medium | Production with Corrosion |
| Local SQLite | Manual | High | Development/Testing |

## Corrosion API Endpoints

Corrosion exposes a REST API:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/v1/queries` | POST | Execute SQL query (SELECT) |
| `/v1/transactions` | POST | Execute SQL transaction (INSERT/UPDATE/DELETE) |

## Troubleshooting

### Corrosion not reachable

```bash
# Check if Corrosion is running
docker-compose ps corrosion

# Check Corrosion logs
docker-compose logs corrosion

# Test API from edgeProxy container
docker-compose exec edgeproxy curl http://corrosion:8090/v1/queries \
  -H "Content-Type: application/json" \
  -d '{"sql": "SELECT 1"}'
```

### Data not replicating between POPs

```bash
# Check gossip connectivity
nc -zv pop-sa.example.com 4001

# Check Corrosion cluster status
curl http://localhost:8090/v1/cluster/status
```
