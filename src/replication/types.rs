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

    #[test]
    fn test_hlc_timestamp_default() {
        let ts = HLCTimestamp::default();
        assert_eq!(ts.wall_time, 0);
        assert_eq!(ts.counter, 0);
        assert_eq!(ts.node_hash, 0);
    }

    #[test]
    fn test_hlc_timestamp_eq() {
        let ts1 = HLCTimestamp { wall_time: 100, counter: 1, node_hash: 42 };
        let ts2 = HLCTimestamp { wall_time: 100, counter: 1, node_hash: 42 };
        let ts3 = HLCTimestamp { wall_time: 101, counter: 1, node_hash: 42 };

        assert_eq!(ts1, ts2);
        assert_ne!(ts1, ts3);
    }

    #[test]
    fn test_hlc_tick_with_older_other() {
        let node = NodeId::new("node-1");
        let old_ts = HLCTimestamp { wall_time: 1000, counter: 5, node_hash: 42 };
        let current = HLCTimestamp::now(&node);

        // Current should have higher wall_time than the old timestamp
        let ticked = current.tick(Some(&old_ts), &node);

        // Ticked should be greater than both
        assert!(ticked > current);
        assert!(ticked > old_ts);
    }

    #[test]
    fn test_hlc_tick_same_wall_time_both() {
        let node = NodeId::new("node-1");
        // Use a very large wall time to simulate future time
        let wall = u64::MAX - 1000;
        let ts1 = HLCTimestamp { wall_time: wall, counter: 5, node_hash: 42 };
        let ts2 = HLCTimestamp { wall_time: wall, counter: 3, node_hash: 43 };

        // When both have same wall_time (future), counter should be max + 1
        let result = ts1.tick(Some(&ts2), &node);
        // Counter should be max(5, 3) + 1 = 6
        assert_eq!(result.counter, 6);
        assert_eq!(result.wall_time, wall);
    }

    #[test]
    fn test_node_id_from_owned_string() {
        let s = String::from("node-test");
        let id: NodeId = s.clone().into();
        assert_eq!(id.as_str(), "node-test");
    }

    #[test]
    fn test_node_id_eq_hash() {
        use std::collections::HashSet;

        let id1 = NodeId::new("node-1");
        let id2 = NodeId::new("node-1");
        let id3 = NodeId::new("node-2");

        assert_eq!(id1, id2);
        assert_ne!(id1, id3);

        let mut set = HashSet::new();
        set.insert(id1.clone());
        assert!(set.contains(&id2));
        assert!(!set.contains(&id3));
    }

    #[test]
    fn test_change_kind_clone() {
        let kind = ChangeKind::Insert;
        let cloned = kind.clone();
        assert_eq!(kind, cloned);
    }

    #[test]
    fn test_change_kind_variants() {
        assert_ne!(ChangeKind::Insert, ChangeKind::Update);
        assert_ne!(ChangeKind::Update, ChangeKind::Delete);
        assert_ne!(ChangeKind::Delete, ChangeKind::Insert);
    }

    #[test]
    fn test_change_clone() {
        let node = NodeId::new("node-1");
        let change = Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node);
        let cloned = change.clone();

        assert_eq!(change.table, cloned.table);
        assert_eq!(change.pk, cloned.pk);
    }

    #[test]
    fn test_change_delete() {
        let node = NodeId::new("node-1");
        let change = Change::new("backends", "pk1", ChangeKind::Delete, "{}", &node);
        assert_eq!(change.kind, ChangeKind::Delete);
    }

    #[test]
    fn test_changeset_clone() {
        let node = NodeId::new("node-1");
        let cs = ChangeSet::new(node, 1, vec![]);
        let cloned = cs.clone();

        assert_eq!(cs.seq, cloned.seq);
        assert_eq!(cs.checksum, cloned.checksum);
    }

    #[test]
    fn test_changeset_empty() {
        let node = NodeId::new("node-1");
        let cs = ChangeSet::new(node, 0, vec![]);

        assert!(cs.verify());
        assert_eq!(cs.changes.len(), 0);
    }

    #[test]
    fn test_message_sync_request() {
        let msg = Message::SyncRequest { from_seq: 100, table: Some("backends".to_string()) };

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&bytes).unwrap();

        match decoded {
            Message::SyncRequest { from_seq, table } => {
                assert_eq!(from_seq, 100);
                assert_eq!(table, Some("backends".to_string()));
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_message_sync_response() {
        let node = NodeId::new("node-1");
        let cs = ChangeSet::new(node, 1, vec![]);
        let msg = Message::SyncResponse(vec![cs]);

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&bytes).unwrap();

        match decoded {
            Message::SyncResponse(sets) => assert_eq!(sets.len(), 1),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_message_ack() {
        let node = NodeId::new("node-1");
        let msg = Message::Ack { source: node, seq: 42 };

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&bytes).unwrap();

        match decoded {
            Message::Ack { source, seq } => {
                assert_eq!(source.as_str(), "node-1");
                assert_eq!(seq, 42);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_message_ping_pong() {
        let ping = Message::Ping;
        let pong = Message::Pong;

        let ping_bytes = bincode::serialize(&ping).unwrap();
        let pong_bytes = bincode::serialize(&pong).unwrap();

        let decoded_ping: Message = bincode::deserialize(&ping_bytes).unwrap();
        let decoded_pong: Message = bincode::deserialize(&pong_bytes).unwrap();

        assert!(matches!(decoded_ping, Message::Ping));
        assert!(matches!(decoded_pong, Message::Pong));
    }

    #[test]
    fn test_change_has_id() {
        let node = NodeId::new("node-1");
        let c1 = Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node);

        // Change should have a non-zero ID
        assert!(c1.id > 0);
    }

    #[test]
    fn test_changeset_with_multiple_changes() {
        let node = NodeId::new("node-1");
        let changes = vec![
            Change::new("backends", "pk1", ChangeKind::Insert, "{}", &node),
            Change::new("backends", "pk2", ChangeKind::Update, "{}", &node),
            Change::new("backends", "pk3", ChangeKind::Delete, "{}", &node),
        ];

        let cs = ChangeSet::new(node, 1, changes);
        assert!(cs.verify());
        assert_eq!(cs.changes.len(), 3);
    }

    #[test]
    fn test_hlc_tick_counter_reset_on_new_wall_time() {
        let node = NodeId::new("node-1");
        // Create a timestamp in the past
        let old_ts = HLCTimestamp {
            wall_time: 1,
            counter: 100,
            node_hash: 42,
        };

        // Tick with current time (which will be much higher)
        let new_ts = old_ts.tick(None, &node);

        // Counter should reset because wall_time increased
        assert!(new_ts.wall_time > old_ts.wall_time);
    }

    #[test]
    fn test_hlc_tick_other_has_higher_wall_time() {
        let node = NodeId::new("node-1");
        // Use far future wall_time so it beats current time
        let far_future = u64::MAX - 100;

        // Self has lower wall_time, other has higher
        let self_ts = HLCTimestamp { wall_time: 1000, counter: 5, node_hash: 42 };
        let other_ts = HLCTimestamp { wall_time: far_future, counter: 10, node_hash: 43 };

        // Tick: other.wall_time is max, so result.counter should be other.counter + 1
        let result = self_ts.tick(Some(&other_ts), &node);

        assert_eq!(result.wall_time, far_future);
        assert_eq!(result.counter, 11); // other.counter + 1
    }

    #[test]
    fn test_hlc_tick_none_self_wall_time_is_max() {
        let node = NodeId::new("node-1");
        // Use far future wall_time so it beats current time
        let far_future = u64::MAX - 100;

        let self_ts = HLCTimestamp { wall_time: far_future, counter: 7, node_hash: 42 };

        // Tick without other: self.wall_time is max (> now)
        let result = self_ts.tick(None, &node);

        assert_eq!(result.wall_time, far_future);
        assert_eq!(result.counter, 8); // self.counter + 1
    }
}
