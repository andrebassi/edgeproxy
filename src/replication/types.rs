//! Replication Types
//!
//! Core types for the replication system including changes, messages, and identifiers.

use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

/// Unique identifier for a node in the cluster.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NodeId(pub String);

impl NodeId {
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl From<String> for NodeId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

impl From<&str> for NodeId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Hybrid Logical Clock timestamp for ordering events.
///
/// Combines wall clock time with a logical counter to ensure
/// total ordering even when wall clocks are skewed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct HLCTimestamp {
    /// Wall clock time in microseconds since UNIX epoch
    pub wall_time: u64,
    /// Logical counter for events at the same wall time
    pub counter: u32,
    /// Node ID hash for tie-breaking
    pub node_hash: u32,
}

impl HLCTimestamp {
    /// Create a new timestamp for the current time.
    pub fn now(node_id: &NodeId) -> Self {
        let wall_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        Self {
            wall_time,
            counter: 0,
            node_hash: crc32fast::hash(node_id.0.as_bytes()),
        }
    }

    /// Create a timestamp that is greater than self and other.
    pub fn tick(&self, other: Option<&HLCTimestamp>, node_id: &NodeId) -> Self {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_micros() as u64;

        let node_hash = crc32fast::hash(node_id.0.as_bytes());

        match other {
            Some(o) => {
                let max_wall = now.max(self.wall_time).max(o.wall_time);
                let counter = if max_wall == self.wall_time && max_wall == o.wall_time {
                    self.counter.max(o.counter) + 1
                } else if max_wall == self.wall_time {
                    self.counter + 1
                } else if max_wall == o.wall_time {
                    o.counter + 1
                } else {
                    0
                };
                Self {
                    wall_time: max_wall,
                    counter,
                    node_hash,
                }
            }
            None => {
                let max_wall = now.max(self.wall_time);
                let counter = if max_wall == self.wall_time {
                    self.counter + 1
                } else {
                    0
                };
                Self {
                    wall_time: max_wall,
                    counter,
                    node_hash,
                }
            }
        }
    }
}

impl Default for HLCTimestamp {
    fn default() -> Self {
        Self {
            wall_time: 0,
            counter: 0,
            node_hash: 0,
        }
    }
}

/// Kind of change operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChangeKind {
    /// Insert a new row
    Insert,
    /// Update an existing row
    Update,
    /// Delete a row (soft delete with deleted=1)
    Delete,
}

/// A single change to be replicated.
///
/// Uses Last-Write-Wins (LWW) semantics based on HLC timestamp.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Change {
    /// Unique ID for this change (for deduplication)
    pub id: u64,
    /// Table name
    pub table: String,
    /// Primary key of the affected row
    pub pk: String,
    /// Type of change
    pub kind: ChangeKind,
    /// Column values (JSON-encoded for flexibility)
    pub data: String,
    /// HLC timestamp for ordering
    pub timestamp: HLCTimestamp,
    /// Node that originated this change
    pub origin: NodeId,
}

impl Change {
    /// Create a new change.
    pub fn new(
        table: impl Into<String>,
        pk: impl Into<String>,
        kind: ChangeKind,
        data: impl Into<String>,
        node_id: &NodeId,
    ) -> Self {
        Self {
            id: rand_id(),
            table: table.into(),
            pk: pk.into(),
            kind,
            data: data.into(),
            timestamp: HLCTimestamp::now(node_id),
            origin: node_id.clone(),
        }
    }

    /// Check if this change wins over another for the same key.
    pub fn wins_over(&self, other: &Change) -> bool {
        self.timestamp > other.timestamp
    }
}

/// A set of changes to be broadcast together.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChangeSet {
    /// Source node
    pub source: NodeId,
    /// Sequence number for ordering within a node
    pub seq: u64,
    /// Changes in this batch
    pub changes: Vec<Change>,
    /// CRC32 checksum for integrity
    pub checksum: u32,
}

impl ChangeSet {
    /// Create a new changeset.
    pub fn new(source: NodeId, seq: u64, changes: Vec<Change>) -> Self {
        let checksum = Self::compute_checksum(&changes);
        Self {
            source,
            seq,
            changes,
            checksum,
        }
    }

    /// Verify the checksum.
    pub fn verify(&self) -> bool {
        self.checksum == Self::compute_checksum(&self.changes)
    }

    fn compute_checksum(changes: &[Change]) -> u32 {
        let bytes = bincode::serialize(changes).unwrap_or_default();
        crc32fast::hash(&bytes)
    }
}

/// Message types for peer communication.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Message {
    /// Broadcast changes to peers
    Broadcast(ChangeSet),
    /// Request changes since a version
    SyncRequest {
        from_seq: u64,
        table: Option<String>,
    },
    /// Response with requested changes
    SyncResponse(Vec<ChangeSet>),
    /// Acknowledge receipt of a broadcast
    Ack {
        source: NodeId,
        seq: u64,
    },
    /// Ping for liveness
    Ping,
    /// Pong response
    Pong,
}

/// Generate a random ID using timestamp and random bits.
fn rand_id() -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos() as u64;
    now ^ (rand_u32() as u64)
}

/// Simple random u32 using system time entropy.
fn rand_u32() -> u32 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    (now as u32).wrapping_mul(1664525).wrapping_add(1013904223)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_node_id() {
        let id = NodeId::new("pop-sa-1");
        assert_eq!(id.as_str(), "pop-sa-1");
        assert_eq!(format!("{}", id), "pop-sa-1");
    }

    #[test]
    fn test_node_id_from_string() {
        let id: NodeId = "pop-us-1".into();
        assert_eq!(id.as_str(), "pop-us-1");
    }

    #[test]
    fn test_hlc_timestamp_ordering() {
        let node = NodeId::new("node-1");
        let t1 = HLCTimestamp::now(&node);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let t2 = HLCTimestamp::now(&node);

        assert!(t2 > t1);
    }

    #[test]
    fn test_hlc_timestamp_tick() {
        let node = NodeId::new("node-1");
        let t1 = HLCTimestamp::now(&node);
        let t2 = t1.tick(None, &node);
        let t3 = t2.tick(Some(&t1), &node);

        assert!(t2 > t1);
        assert!(t3 > t2);
    }

    #[test]
    fn test_change_wins_over() {
        let node1 = NodeId::new("node-1");
        let node2 = NodeId::new("node-2");

        let c1 = Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node1);
        std::thread::sleep(std::time::Duration::from_millis(1));
        let c2 = Change::new("backends", "pk1", ChangeKind::Update, "{}", &node2);

        assert!(c2.wins_over(&c1));
        assert!(!c1.wins_over(&c2));
    }

    #[test]
    fn test_changeset_checksum() {
        let node = NodeId::new("node-1");
        let changes = vec![
            Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node),
        ];

        let cs = ChangeSet::new(node, 1, changes);
        assert!(cs.verify());
    }

    #[test]
    fn test_changeset_checksum_fails_on_tamper() {
        let node = NodeId::new("node-1");
        let changes = vec![
            Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node),
        ];

        let mut cs = ChangeSet::new(node.clone(), 1, changes);
        cs.changes[0].data = "tampered".to_string();

        assert!(!cs.verify());
    }

    #[test]
    fn test_message_serialization() {
        let node = NodeId::new("node-1");
        let cs = ChangeSet::new(node, 1, vec![]);
        let msg = Message::Broadcast(cs);

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&bytes).unwrap();

        match decoded {
            Message::Broadcast(cs) => assert_eq!(cs.seq, 1),
            _ => panic!("wrong message type"),
        }
    }
}
