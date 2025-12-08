---
sidebar_position: 6
---

# Built-in Replication

edgeProxy v0.3.0 includes **built-in SQLite replication** for automatic state synchronization across multiple POPs. This document provides a deep-dive into how the replication system works, targeted at developers who want to understand the internals.

## Overview

The built-in replication system enables automatic synchronization of `routing.db` across multiple POPs (Points of Presence). When a backend is registered at one POP, it automatically propagates to all other POPs in the cluster.

![Replication Architecture](/img/replication-architecture.svg)

## Key Concepts

### 1. Hybrid Logical Clock (HLC)

The HLC is the foundation for ordering events across distributed nodes. It combines:

- **Wall Clock Time**: Real timestamp in milliseconds
- **Logical Counter**: Incremented when events happen at the same millisecond

```rust
// src/replication/types.rs
pub struct HlcTimestamp {
    pub wall_time: u64,   // milliseconds since epoch
    pub logical: u32,     // logical counter
    pub node_id: String,  // which node generated this timestamp
}
```

**Why HLC?**

Physical clocks can drift between servers. If Node A's clock is 100ms ahead of Node B, events on Node A would incorrectly appear newer. HLC solves this by:

1. Using the maximum of local time and received message time
2. Incrementing logical counter for ties
3. Including node_id for deterministic tie-breaking

```rust
impl HlcTimestamp {
    pub fn tick(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;

        if now > self.wall_time {
            self.wall_time = now;
            self.logical = 0;
        } else {
            self.logical += 1;
        }
    }
}
```

### 2. Last-Write-Wins (LWW) Conflict Resolution

When two nodes modify the same record simultaneously, we need deterministic conflict resolution. LWW uses the HLC timestamp:

```rust
impl Change {
    pub fn wins_over(&self, other: &Change) -> bool {
        // Compare wall time first
        if self.hlc_timestamp.wall_time != other.hlc_timestamp.wall_time {
            return self.hlc_timestamp.wall_time > other.hlc_timestamp.wall_time;
        }
        // Then logical counter
        if self.hlc_timestamp.logical != other.hlc_timestamp.logical {
            return self.hlc_timestamp.logical > other.hlc_timestamp.logical;
        }
        // Finally node_id for deterministic tie-breaking
        self.hlc_timestamp.node_id > other.hlc_timestamp.node_id
    }
}
```

**Example scenario:**

1. Node SA updates backend `b1` at HLC(1000, 0, "sa")
2. Node US updates backend `b1` at HLC(1000, 0, "us")
3. Both changes arrive at Node EU
4. EU applies `sa`'s change because "us" > "sa" lexicographically? No!
5. Actually, "sa" < "us", so US wins (highest node_id wins ties)

### 3. Change Detection

Changes are tracked via the `Change` struct:

```rust
pub struct Change {
    pub table: String,      // "backends"
    pub row_id: String,     // primary key
    pub kind: ChangeKind,   // Insert, Update, Delete
    pub data: String,       // JSON serialized row data
    pub hlc_timestamp: HlcTimestamp,
}

pub enum ChangeKind {
    Insert,
    Update,
    Delete,
}
```

The `SyncService` collects pending changes and flushes them as a `ChangeSet`:

```rust
pub struct ChangeSet {
    pub origin_node: String,
    pub changes: Vec<Change>,
    pub checksum: u32,  // CRC32 for integrity
}
```

### 4. SWIM-like Gossip Protocol

The gossip protocol handles cluster membership and failure detection. It's inspired by [SWIM](http://www.cs.cornell.edu/projects/Quicksilver/public_pdfs/SWIM.pdf) (Scalable Weakly-consistent Infection-style Process Group Membership).

```rust
// src/replication/gossip.rs
pub enum GossipMessage {
    // Check if node is alive
    Ping {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    // Response to ping
    Ack {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    // Announce joining the cluster
    Join {
        node_id: String,
        gossip_addr: SocketAddr,
        transport_addr: SocketAddr,
    },
    // Share member list
    MemberList {
        members: Vec<(String, SocketAddr, SocketAddr, u64)>,
    },
}
```

**Membership flow:**

1. New node sends `Join` to bootstrap peers
2. Bootstrap peer adds new node to member list
3. Bootstrap peer responds with `MemberList`
4. New node adds all discovered members
5. Periodic `Ping`/`Ack` maintains liveness

**Failure detection:**

- Nodes ping random members every `gossip_interval` (default: 1s)
- If no `Ack` received within 30s, member is marked `Dead`
- Dead members are removed from routing

### 5. QUIC Transport

Data synchronization uses [QUIC](https://quicwg.org/) via the [Quinn](https://github.com/quinn-rs/quinn) library:

```rust
// src/replication/transport.rs
pub struct TransportService {
    endpoint: Endpoint,
    peers: RwLock<HashMap<String, Connection>>,
    // ...
}
```

**Why QUIC?**

- **Multiplexed streams**: Multiple ChangeSets can sync simultaneously
- **Built-in encryption**: TLS 1.3 for secure peer communication
- **Connection migration**: Handles IP changes gracefully
- **Low latency**: 0-RTT handshakes for known peers

**Self-signed certificates:**

The transport generates self-signed certificates for cluster communication:

```rust
fn generate_self_signed_cert() -> (CertificateDer, PrivateKeyDer) {
    let cert = rcgen::generate_simple_self_signed(vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
    ]).unwrap();
    // ...
}
```

## Data Flow: End-to-End

Let's trace a backend registration from start to finish:

### Step 1: Backend Registration

```bash
# Backend registers via Auto-Discovery API
curl -X POST http://pop-sa:8081/api/v1/register \
  -H "Content-Type: application/json" \
  -d '{"id": "sa-node-1", "app": "myapp", "region": "sa", "ip": "10.50.1.1", "port": 9000}'
```

### Step 2: Local SQLite Write

The `ApiServer` inserts into local SQLite:

```rust
// adapters/inbound/api_server.rs
async fn register_backend(State(state): State<AppState>, Json(req): Json<RegisterRequest>) {
    // Insert into SQLite
    sqlx::query("INSERT INTO backends ...")
        .execute(&state.db)
        .await?;
}
```

### Step 3: Change Recorded

The `SyncService` records the change with an HLC timestamp:

```rust
// replication/sync.rs
pub fn record_backend_change(&self, id: &str, kind: ChangeKind, data: &str) {
    let mut hlc = self.hlc.write();
    hlc.tick();

    let change = Change {
        table: "backends".to_string(),
        row_id: id.to_string(),
        kind,
        data: data.to_string(),
        hlc_timestamp: hlc.clone(),
    };

    self.pending_changes.write().push(change);
}
```

### Step 4: Flush to ChangeSet

Periodically (default: 5s), pending changes are flushed:

```rust
pub async fn flush(&self) -> Option<ChangeSet> {
    let changes: Vec<Change> = {
        let mut pending = self.pending_changes.write();
        if pending.is_empty() { return None; }
        pending.drain(..).collect()
    };

    let changeset = ChangeSet::new(&self.node_id, changes);
    let _ = self.event_tx.send(SyncEvent::BroadcastReady(changeset.clone())).await;
    Some(changeset)
}
```

### Step 5: Broadcast via QUIC

The `ReplicationAgent` receives the event and broadcasts to all peers:

```rust
// replication/agent.rs
async fn handle_sync_event(&self, event: SyncEvent) {
    match event {
        SyncEvent::BroadcastReady(changeset) => {
            let transport = self.transport.read().await;
            for member in self.gossip.alive_members() {
                transport.send_changeset(&member.transport_addr, &changeset).await;
            }
        }
    }
}
```

### Step 6: Remote Node Receives

On the receiving POP (e.g., POP-US):

```rust
// replication/transport.rs
async fn handle_incoming_stream(&self, stream: RecvStream) {
    let msg: Message = bincode::deserialize(&data)?;
    match msg {
        Message::ChangeBroadcast(changeset) => {
            if changeset.verify_checksum() {
                self.event_tx.send(TransportEvent::ChangeSetReceived(changeset)).await;
            }
        }
    }
}
```

### Step 7: Apply with LWW

The `SyncService` applies changes using LWW:

```rust
pub async fn apply_changeset(&self, changeset: &ChangeSet) -> anyhow::Result<usize> {
    let mut applied = 0;

    for change in &changeset.changes {
        // Check if we already have a newer version
        let existing = self.version_vector.read().get(&change.row_id);
        if let Some(existing_hlc) = existing {
            if !change.wins_over_hlc(existing_hlc) {
                continue; // Skip, we have newer
            }
        }

        // Apply the change
        match change.kind {
            ChangeKind::Insert => self.apply_insert(&change).await?,
            ChangeKind::Update => self.apply_update(&change).await?,
            ChangeKind::Delete => self.apply_delete(&change).await?,
        }

        // Update version vector
        self.version_vector.write().insert(change.row_id.clone(), change.hlc_timestamp.clone());
        applied += 1;
    }

    Ok(applied)
}
```

### Step 8: Backend Available Everywhere

Now `sa-node-1` is available on all POPs:

```bash
# Query from POP-US
curl http://pop-us:8081/api/v1/backends
# Returns: [{"id": "sa-node-1", "app": "myapp", "region": "sa", ...}]

# Query from POP-EU
curl http://pop-eu:8081/api/v1/backends
# Returns: [{"id": "sa-node-1", "app": "myapp", "region": "sa", ...}]
```

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `EDGEPROXY_REPLICATION_ENABLED` | `false` | Enable built-in replication |
| `EDGEPROXY_REPLICATION_NODE_ID` | hostname | Unique node identifier |
| `EDGEPROXY_REPLICATION_GOSSIP_ADDR` | `0.0.0.0:4001` | UDP address for gossip |
| `EDGEPROXY_REPLICATION_TRANSPORT_ADDR` | `0.0.0.0:4002` | QUIC address for data sync |
| `EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS` | (none) | Comma-separated peer addresses |
| `EDGEPROXY_REPLICATION_GOSSIP_INTERVAL_MS` | `1000` | Gossip ping interval |
| `EDGEPROXY_REPLICATION_SYNC_INTERVAL_MS` | `5000` | Sync flush interval |
| `EDGEPROXY_REPLICATION_CLUSTER_NAME` | `edgeproxy` | Cluster name for isolation |

### Example: 3-POP Cluster

**POP-SA (Bootstrap)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-sa
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
# No bootstrap peers - this is the first node
```

**POP-US (Joins SA)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-us
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS=10.50.1.1:4001
```

**POP-EU (Joins SA and US)**

```bash
EDGEPROXY_REPLICATION_ENABLED=true
EDGEPROXY_REPLICATION_NODE_ID=pop-eu
EDGEPROXY_REPLICATION_GOSSIP_ADDR=0.0.0.0:4001
EDGEPROXY_REPLICATION_TRANSPORT_ADDR=0.0.0.0:4002
EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS=10.50.1.1:4001,10.50.2.1:4001
```

## Source Code Reference

| File | Purpose |
|------|---------|
| `src/replication/mod.rs` | Module exports |
| `src/replication/config.rs` | ReplicationConfig struct |
| `src/replication/types.rs` | HlcTimestamp, NodeId, Change, ChangeSet |
| `src/replication/gossip.rs` | GossipService, GossipMessage, Member |
| `src/replication/sync.rs` | SyncService, change tracking |
| `src/replication/transport.rs` | TransportService, QUIC peer communication |
| `src/replication/agent.rs` | ReplicationAgent orchestrator |

## Troubleshooting

### Nodes not discovering each other

```bash
# Check if gossip port is open
nc -zvu 10.50.1.1 4001

# Verify bootstrap peers are correct
echo $EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS

# Check firewall rules
sudo ufw status
```

### Changes not propagating

```bash
# Check transport connectivity
nc -zv 10.50.1.1 4002

# Verify cluster membership (check logs)
journalctl -u edgeproxy | grep "member joined"

# Ensure sync interval is reasonable
echo $EDGEPROXY_REPLICATION_SYNC_INTERVAL_MS
```

### HLC drift warnings

If you see HLC drift warnings, ensure NTP is running:

```bash
# Check NTP status
timedatectl status

# Install and enable NTP
sudo apt install chrony
sudo systemctl enable chronyd
sudo systemctl start chronyd
```

## Performance Tuning

### Gossip Interval

- **Lower (500ms)**: Faster failure detection, more network traffic
- **Higher (2000ms)**: Less traffic, slower detection
- **Recommendation**: 1000ms for most deployments

### Sync Interval

- **Lower (1000ms)**: Near real-time sync, higher CPU usage
- **Higher (10000ms)**: Batches more changes, potential lag
- **Recommendation**: 5000ms for balanced performance

### Network Requirements

| Path | Protocol | Port | Bandwidth |
|------|----------|------|-----------|
| Gossip | UDP | 4001 | ~1 KB/s per node |
| Transport | QUIC/UDP | 4002 | Varies with change rate |

## Security Considerations

1. **Network Isolation**: Run replication ports on WireGuard overlay
2. **Firewall**: Only allow trusted POPs to connect to 4001/4002
3. **TLS**: Transport uses TLS 1.3 (self-signed certs for cluster)
4. **Cluster Name**: Use unique cluster names to prevent cross-cluster pollution

```bash
# Firewall rules example (UFW)
sudo ufw allow from 10.50.0.0/16 to any port 4001 proto udp
sudo ufw allow from 10.50.0.0/16 to any port 4002 proto udp
```

## Future Improvements

- [ ] Delta sync (only send changed fields)
- [ ] Merkle tree-based anti-entropy
- [ ] Automatic cluster discovery via mDNS
- [ ] Prometheus metrics for replication lag
- [ ] Read replicas for local SQLite
