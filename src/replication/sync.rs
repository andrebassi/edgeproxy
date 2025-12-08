//! Sync Service
//!
//! Handles change detection, storage, and application using Last-Write-Wins (LWW)
//! semantics for conflict resolution.

use crate::replication::types::{Change, ChangeKind, ChangeSet, HLCTimestamp, NodeId};
use parking_lot::RwLock;
use rusqlite::{Connection, params};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tokio::sync::mpsc;

/// Version vector for tracking per-node sequence numbers.
#[derive(Debug, Clone, Default)]
pub struct VersionVector {
    /// Map of node_id -> latest sequence number seen
    versions: HashMap<String, u64>,
}

impl VersionVector {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the sequence number for a node.
    pub fn get(&self, node_id: &str) -> u64 {
        *self.versions.get(node_id).unwrap_or(&0)
    }

    /// Update the sequence number for a node.
    pub fn update(&mut self, node_id: &str, seq: u64) {
        let current = self.versions.entry(node_id.to_string()).or_insert(0);
        if seq > *current {
            *current = seq;
        }
    }

    /// Check if we've seen this sequence.
    pub fn has_seen(&self, node_id: &str, seq: u64) -> bool {
        self.get(node_id) >= seq
    }

    /// Merge another version vector into this one.
    pub fn merge(&mut self, other: &VersionVector) {
        for (node_id, seq) in &other.versions {
            self.update(node_id, *seq);
        }
    }
}

/// Events emitted by the sync service.
#[derive(Debug, Clone)]
pub enum SyncEvent {
    /// A change was applied locally
    ChangeApplied(Change),
    /// A changeset is ready to broadcast
    BroadcastReady(ChangeSet),
    /// Sync completed with a peer
    PeerSynced { node_id: NodeId, changes_applied: usize },
}

/// Sync service for change management.
pub struct SyncService {
    node_id: NodeId,
    db_path: String,
    sequence: Arc<AtomicU64>,
    version_vector: Arc<RwLock<VersionVector>>,
    pending_changes: Arc<RwLock<Vec<Change>>>,
    last_timestamps: Arc<RwLock<HashMap<String, HLCTimestamp>>>,
    event_tx: mpsc::Sender<SyncEvent>,
    event_rx: Option<mpsc::Receiver<SyncEvent>>,
}

impl SyncService {
    /// Create a new sync service.
    pub fn new(node_id: NodeId, db_path: String) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);

        Self {
            node_id,
            db_path,
            sequence: Arc::new(AtomicU64::new(0)),
            version_vector: Arc::new(RwLock::new(VersionVector::new())),
            pending_changes: Arc::new(RwLock::new(Vec::new())),
            last_timestamps: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Some(event_rx),
        }
    }

    /// Get the event receiver.
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<SyncEvent>> {
        self.event_rx.take()
    }

    /// Get current sequence number.
    pub fn sequence(&self) -> u64 {
        self.sequence.load(Ordering::SeqCst)
    }

    /// Get the version vector.
    pub fn version_vector(&self) -> VersionVector {
        self.version_vector.read().clone()
    }

    /// Initialize the database schema.
    pub fn init_db(&self) -> anyhow::Result<()> {
        let conn = Connection::open(&self.db_path)?;

        // Create changes log table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS __replication_log (
                id INTEGER PRIMARY KEY,
                change_id INTEGER UNIQUE,
                table_name TEXT NOT NULL,
                pk TEXT NOT NULL,
                kind TEXT NOT NULL,
                data TEXT NOT NULL,
                timestamp_wall INTEGER NOT NULL,
                timestamp_counter INTEGER NOT NULL,
                timestamp_node INTEGER NOT NULL,
                origin_node TEXT NOT NULL,
                applied_at INTEGER NOT NULL
            )",
            [],
        )?;

        // Create version vector table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS __replication_versions (
                node_id TEXT PRIMARY KEY,
                sequence INTEGER NOT NULL
            )",
            [],
        )?;

        // Create last-write timestamps table
        conn.execute(
            "CREATE TABLE IF NOT EXISTS __replication_lww (
                table_pk TEXT PRIMARY KEY,
                timestamp_wall INTEGER NOT NULL,
                timestamp_counter INTEGER NOT NULL,
                timestamp_node INTEGER NOT NULL
            )",
            [],
        )?;

        // Create backends table if not exists
        conn.execute(
            "CREATE TABLE IF NOT EXISTS backends (
                id TEXT PRIMARY KEY,
                app TEXT NOT NULL,
                region TEXT NOT NULL,
                country TEXT,
                wg_ip TEXT NOT NULL,
                port INTEGER NOT NULL,
                healthy INTEGER DEFAULT 1,
                weight INTEGER DEFAULT 2,
                soft_limit INTEGER DEFAULT 100,
                hard_limit INTEGER DEFAULT 150,
                deleted INTEGER DEFAULT 0
            )",
            [],
        )?;

        // Load version vector from database
        let mut stmt = conn.prepare("SELECT node_id, sequence FROM __replication_versions")?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, u64>(1)?))
        })?;

        let mut vv = self.version_vector.write();
        for row in rows {
            let (node_id, seq) = row?;
            vv.update(&node_id, seq);
        }

        // Load sequence number
        if let Some(seq) = vv.versions.get(&self.node_id.0) {
            self.sequence.store(*seq, Ordering::SeqCst);
        }

        tracing::info!("sync service initialized, db_path={}", self.db_path);
        Ok(())
    }

    /// Record a local change.
    pub fn record_change(&self, table: &str, pk: &str, kind: ChangeKind, data: &str) -> Change {
        let change = Change::new(table, pk, kind, data, &self.node_id);
        self.pending_changes.write().push(change.clone());
        change
    }

    /// Flush pending changes as a changeset.
    pub async fn flush(&self) -> Option<ChangeSet> {
        // Collect changes without holding lock across await
        let changes: Vec<Change> = {
            let mut pending = self.pending_changes.write();
            if pending.is_empty() {
                return None;
            }
            pending.drain(..).collect()
        };

        let seq = self.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        let changeset = ChangeSet::new(self.node_id.clone(), seq, changes);

        // Update version vector (no await while holding lock)
        {
            self.version_vector.write().update(&self.node_id.0, seq);
        }

        // Persist version vector
        if let Err(e) = self.persist_version(&self.node_id.0, seq) {
            tracing::error!("failed to persist version: {:?}", e);
        }

        // Now we can await safely - no locks held
        let _ = self.event_tx.send(SyncEvent::BroadcastReady(changeset.clone())).await;

        Some(changeset)
    }

    /// Apply a changeset received from a peer.
    pub async fn apply_changeset(&self, changeset: &ChangeSet) -> anyhow::Result<usize> {
        // Verify checksum
        if !changeset.verify() {
            anyhow::bail!("changeset checksum verification failed");
        }

        // Check if we've already seen this
        if self.version_vector.read().has_seen(&changeset.source.0, changeset.seq) {
            tracing::debug!(
                "skipping already-seen changeset seq={} from {}",
                changeset.seq,
                changeset.source
            );
            return Ok(0);
        }

        let mut applied = 0;
        let conn = Connection::open(&self.db_path)?;

        for change in &changeset.changes {
            if self.should_apply_change(&conn, change)? {
                self.apply_single_change(&conn, change)?;
                applied += 1;

                let _ = self.event_tx.send(SyncEvent::ChangeApplied(change.clone())).await;
            }
        }

        // Update version vector
        self.version_vector.write().update(&changeset.source.0, changeset.seq);
        self.persist_version(&changeset.source.0, changeset.seq)?;

        if applied > 0 {
            tracing::info!(
                "applied {} changes from {} (seq={})",
                applied,
                changeset.source,
                changeset.seq
            );
        }

        Ok(applied)
    }

    /// Check if a change should be applied (LWW check).
    fn should_apply_change(&self, conn: &Connection, change: &Change) -> anyhow::Result<bool> {
        let key = format!("{}:{}", change.table, change.pk);

        // Check in-memory cache first
        if let Some(last_ts) = self.last_timestamps.read().get(&key) {
            if change.timestamp <= *last_ts {
                return Ok(false);
            }
        }

        // Check database
        let mut stmt = conn.prepare(
            "SELECT timestamp_wall, timestamp_counter, timestamp_node
             FROM __replication_lww WHERE table_pk = ?"
        )?;

        let result: Option<HLCTimestamp> = stmt.query_row([&key], |row| {
            Ok(HLCTimestamp {
                wall_time: row.get(0)?,
                counter: row.get(1)?,
                node_hash: row.get(2)?,
            })
        }).ok();

        match result {
            Some(existing_ts) => Ok(change.timestamp > existing_ts),
            None => Ok(true), // No existing record, apply the change
        }
    }

    /// Apply a single change to the database.
    fn apply_single_change(&self, conn: &Connection, change: &Change) -> anyhow::Result<()> {
        let key = format!("{}:{}", change.table, change.pk);

        // Update LWW timestamp
        conn.execute(
            "INSERT OR REPLACE INTO __replication_lww
             (table_pk, timestamp_wall, timestamp_counter, timestamp_node)
             VALUES (?, ?, ?, ?)",
            params![
                key,
                change.timestamp.wall_time as i64,
                change.timestamp.counter as i64,
                change.timestamp.node_hash as i64
            ],
        )?;

        // Apply the actual change based on table
        match change.table.as_str() {
            "backends" => self.apply_backend_change(conn, change)?,
            _ => {
                tracing::warn!("unknown table in change: {}", change.table);
            }
        }

        // Update in-memory cache
        self.last_timestamps.write().insert(key, change.timestamp);

        // Log the change
        conn.execute(
            "INSERT OR IGNORE INTO __replication_log
             (change_id, table_name, pk, kind, data, timestamp_wall, timestamp_counter, timestamp_node, origin_node, applied_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![
                change.id as i64,
                change.table,
                change.pk,
                format!("{:?}", change.kind),
                change.data,
                change.timestamp.wall_time as i64,
                change.timestamp.counter as i64,
                change.timestamp.node_hash as i64,
                change.origin.0,
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as i64
            ],
        )?;

        Ok(())
    }

    /// Apply a change to the backends table.
    fn apply_backend_change(&self, conn: &Connection, change: &Change) -> anyhow::Result<()> {
        match change.kind {
            ChangeKind::Insert | ChangeKind::Update => {
                // Parse the JSON data
                let data: serde_json::Value = serde_json::from_str(&change.data)?;

                conn.execute(
                    "INSERT OR REPLACE INTO backends
                     (id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit, deleted)
                     VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
                    params![
                        change.pk,
                        data.get("app").and_then(|v| v.as_str()).unwrap_or(""),
                        data.get("region").and_then(|v| v.as_str()).unwrap_or("us"),
                        data.get("country").and_then(|v| v.as_str()),
                        data.get("wg_ip").and_then(|v| v.as_str()).unwrap_or(""),
                        data.get("port").and_then(|v| v.as_i64()).unwrap_or(0),
                        data.get("healthy").and_then(|v| v.as_i64()).unwrap_or(1),
                        data.get("weight").and_then(|v| v.as_i64()).unwrap_or(2),
                        data.get("soft_limit").and_then(|v| v.as_i64()).unwrap_or(100),
                        data.get("hard_limit").and_then(|v| v.as_i64()).unwrap_or(150),
                        0
                    ],
                )?;
            }
            ChangeKind::Delete => {
                // Soft delete
                conn.execute(
                    "UPDATE backends SET deleted = 1 WHERE id = ?",
                    [&change.pk],
                )?;
            }
        }
        Ok(())
    }

    /// Persist version vector to database.
    fn persist_version(&self, node_id: &str, seq: u64) -> anyhow::Result<()> {
        let conn = Connection::open(&self.db_path)?;
        conn.execute(
            "INSERT OR REPLACE INTO __replication_versions (node_id, sequence) VALUES (?, ?)",
            params![node_id, seq as i64],
        )?;
        Ok(())
    }

    /// Get changes since a given sequence for a node.
    pub fn get_changes_since(&self, node_id: &str, since_seq: u64) -> anyhow::Result<Vec<ChangeSet>> {
        let conn = Connection::open(&self.db_path)?;

        // For now, return empty - full implementation would query __replication_log
        // and reconstruct changesets
        let _ = (node_id, since_seq, conn);
        Ok(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_version_vector_new() {
        let vv = VersionVector::new();
        assert_eq!(vv.get("any-node"), 0);
    }

    #[test]
    fn test_version_vector_default() {
        let vv = VersionVector::default();
        assert_eq!(vv.get("any-node"), 0);
    }

    #[test]
    fn test_version_vector_debug() {
        let vv = VersionVector::new();
        let debug = format!("{:?}", vv);
        assert!(debug.contains("VersionVector"));
    }

    #[test]
    fn test_version_vector_clone() {
        let mut vv = VersionVector::new();
        vv.update("node-1", 5);

        let cloned = vv.clone();
        assert_eq!(cloned.get("node-1"), 5);
    }

    #[test]
    fn test_version_vector_get_update() {
        let mut vv = VersionVector::new();

        assert_eq!(vv.get("node-1"), 0);
        assert!(!vv.has_seen("node-1", 1));

        vv.update("node-1", 5);
        assert_eq!(vv.get("node-1"), 5);
        assert!(vv.has_seen("node-1", 5));
        assert!(vv.has_seen("node-1", 3));
        assert!(!vv.has_seen("node-1", 6));
    }

    #[test]
    fn test_version_vector_update_only_increases() {
        let mut vv = VersionVector::new();

        vv.update("node-1", 10);
        assert_eq!(vv.get("node-1"), 10);

        // Lower value should not decrease
        vv.update("node-1", 5);
        assert_eq!(vv.get("node-1"), 10);

        // Higher value should increase
        vv.update("node-1", 15);
        assert_eq!(vv.get("node-1"), 15);
    }

    #[test]
    fn test_version_vector_has_seen() {
        let mut vv = VersionVector::new();

        // Unknown node - hasn't seen anything
        assert!(!vv.has_seen("unknown", 1));
        assert!(vv.has_seen("unknown", 0));

        vv.update("node-1", 5);

        // Has seen seq <= 5
        assert!(vv.has_seen("node-1", 0));
        assert!(vv.has_seen("node-1", 1));
        assert!(vv.has_seen("node-1", 5));

        // Has not seen seq > 5
        assert!(!vv.has_seen("node-1", 6));
        assert!(!vv.has_seen("node-1", 100));
    }

    #[test]
    fn test_version_vector_merge() {
        let mut vv1 = VersionVector::new();
        vv1.update("node-1", 5);
        vv1.update("node-2", 3);

        let mut vv2 = VersionVector::new();
        vv2.update("node-1", 3);
        vv2.update("node-3", 7);

        vv1.merge(&vv2);

        assert_eq!(vv1.get("node-1"), 5); // Max of 5 and 3
        assert_eq!(vv1.get("node-2"), 3);
        assert_eq!(vv1.get("node-3"), 7);
    }

    #[test]
    fn test_version_vector_merge_empty() {
        let mut vv1 = VersionVector::new();
        vv1.update("node-1", 5);

        let vv2 = VersionVector::new();
        vv1.merge(&vv2);

        assert_eq!(vv1.get("node-1"), 5);
    }

    #[test]
    fn test_sync_event_change_applied_debug() {
        let change = Change::new("backends", "b1", ChangeKind::Insert, "{}", &NodeId::new("node-1"));
        let event = SyncEvent::ChangeApplied(change);
        let debug = format!("{:?}", event);
        assert!(debug.contains("ChangeApplied"));
    }

    #[test]
    fn test_sync_event_broadcast_ready_debug() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let event = SyncEvent::BroadcastReady(cs);
        let debug = format!("{:?}", event);
        assert!(debug.contains("BroadcastReady"));
    }

    #[test]
    fn test_sync_event_peer_synced_debug() {
        let event = SyncEvent::PeerSynced {
            node_id: NodeId::new("peer-1"),
            changes_applied: 5,
        };
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerSynced"));
        assert!(debug.contains("peer-1"));
    }

    #[test]
    fn test_sync_event_clone() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let event = SyncEvent::BroadcastReady(cs);
        let cloned = event.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("BroadcastReady"));
    }

    #[test]
    fn test_sync_service_creation() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        assert_eq!(service.sequence(), 0);
    }

    #[test]
    fn test_sync_service_take_event_rx() {
        let temp = NamedTempFile::new().unwrap();
        let mut service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        // First call should return Some
        let rx = service.take_event_rx();
        assert!(rx.is_some());

        // Second call should return None
        let rx = service.take_event_rx();
        assert!(rx.is_none());
    }

    #[test]
    fn test_sync_service_version_vector() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        let vv = service.version_vector();
        assert_eq!(vv.get("test-node"), 0);
    }

    #[test]
    fn test_sync_service_init_db() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        service.init_db().unwrap();

        // Verify tables were created
        let conn = Connection::open(temp.path()).unwrap();
        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type='table'")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"__replication_log".to_string()));
        assert!(tables.contains(&"__replication_versions".to_string()));
        assert!(tables.contains(&"__replication_lww".to_string()));
        assert!(tables.contains(&"backends".to_string()));
    }

    #[test]
    fn test_sync_service_init_db_loads_version_vector() {
        let temp = NamedTempFile::new().unwrap();

        // First, create and init service, flush some changes
        {
            let service = SyncService::new(
                NodeId::new("node-1"),
                temp.path().to_str().unwrap().to_string(),
            );
            service.init_db().unwrap();

            // Manually insert a version
            let conn = Connection::open(temp.path()).unwrap();
            conn.execute(
                "INSERT INTO __replication_versions (node_id, sequence) VALUES (?, ?)",
                params!["node-1", 10i64],
            ).unwrap();
        }

        // Create new service - should load version from DB
        let service = SyncService::new(
            NodeId::new("node-1"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Should have loaded the sequence
        assert_eq!(service.sequence(), 10);
        assert!(service.version_vector().has_seen("node-1", 10));
    }

    #[test]
    fn test_record_change() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        let change = service.record_change(
            "backends",
            "backend-1",
            ChangeKind::Insert,
            r#"{"app":"myapp","region":"sa","wg_ip":"10.0.0.1","port":8080}"#,
        );

        assert_eq!(change.table, "backends");
        assert_eq!(change.pk, "backend-1");
        assert_eq!(change.kind, ChangeKind::Insert);
        assert_eq!(change.origin.0, "test-node");
    }

    #[test]
    fn test_record_multiple_changes() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        service.record_change("backends", "b1", ChangeKind::Insert, "{}");
        service.record_change("backends", "b2", ChangeKind::Insert, "{}");
        service.record_change("backends", "b1", ChangeKind::Update, "{}");

        // Check pending changes
        let pending = service.pending_changes.read().len();
        assert_eq!(pending, 3);
    }

    #[tokio::test]
    async fn test_flush_empty() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        let result = service.flush().await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_flush_with_changes() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        service.init_db().unwrap();

        service.record_change(
            "backends",
            "backend-1",
            ChangeKind::Insert,
            "{}",
        );

        let result = service.flush().await;
        assert!(result.is_some());

        let cs = result.unwrap();
        assert_eq!(cs.seq, 1);
        assert_eq!(cs.changes.len(), 1);
        assert!(cs.verify());

        // Sequence should have incremented
        assert_eq!(service.sequence(), 1);

        // Version vector should be updated
        assert!(service.version_vector().has_seen("test-node", 1));
    }

    #[tokio::test]
    async fn test_flush_increments_sequence() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        service.init_db().unwrap();

        // First flush
        service.record_change("backends", "b1", ChangeKind::Insert, "{}");
        let cs1 = service.flush().await.unwrap();
        assert_eq!(cs1.seq, 1);

        // Second flush
        service.record_change("backends", "b2", ChangeKind::Insert, "{}");
        let cs2 = service.flush().await.unwrap();
        assert_eq!(cs2.seq, 2);

        // Third flush
        service.record_change("backends", "b3", ChangeKind::Insert, "{}");
        let cs3 = service.flush().await.unwrap();
        assert_eq!(cs3.seq, 3);

        assert_eq!(service.sequence(), 3);
    }

    #[tokio::test]
    async fn test_flush_clears_pending() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        service.init_db().unwrap();

        service.record_change("backends", "b1", ChangeKind::Insert, "{}");
        service.record_change("backends", "b2", ChangeKind::Insert, "{}");

        assert_eq!(service.pending_changes.read().len(), 2);

        service.flush().await;

        // Should be empty after flush
        assert_eq!(service.pending_changes.read().len(), 0);
    }

    #[tokio::test]
    async fn test_apply_changeset_invalid_checksum() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Create changeset with invalid checksum
        let mut cs = ChangeSet::new(NodeId::new("other-node"), 1, vec![]);
        cs.checksum = 12345; // Invalid checksum

        let result = service.apply_changeset(&cs).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("checksum"));
    }

    #[tokio::test]
    async fn test_apply_changeset_already_seen() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Manually set version vector to have seen seq 5
        service.version_vector.write().update("other-node", 5);

        // Create changeset with seq 5 (already seen)
        let cs = ChangeSet::new(NodeId::new("other-node"), 5, vec![]);

        let result = service.apply_changeset(&cs).await.unwrap();
        assert_eq!(result, 0); // Nothing applied
    }

    #[tokio::test]
    async fn test_apply_changeset_with_backend_insert() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");
        let data = r#"{"app":"myapp","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let changes = vec![
            Change::new("backends", "backend-1", ChangeKind::Insert, data, &source_node),
        ];
        let cs = ChangeSet::new(source_node.clone(), 1, changes);

        let applied = service.apply_changeset(&cs).await.unwrap();
        assert_eq!(applied, 1);

        // Verify backend was inserted
        let conn = Connection::open(temp.path()).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM backends WHERE id = 'backend-1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Verify version vector updated
        assert!(service.version_vector().has_seen("other-node", 1));
    }

    #[tokio::test]
    async fn test_apply_changeset_with_backend_update() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // First insert
        let source_node = NodeId::new("other-node");
        let data1 = r#"{"app":"myapp","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let changes1 = vec![
            Change::new("backends", "backend-1", ChangeKind::Insert, data1, &source_node),
        ];
        let cs1 = ChangeSet::new(source_node.clone(), 1, changes1);
        service.apply_changeset(&cs1).await.unwrap();

        // Small delay to ensure different timestamp
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        // Then update
        let data2 = r#"{"app":"myapp","region":"us","wg_ip":"10.0.0.2","port":9000}"#;
        let changes2 = vec![
            Change::new("backends", "backend-1", ChangeKind::Update, data2, &source_node),
        ];
        let cs2 = ChangeSet::new(source_node, 2, changes2);
        let applied = service.apply_changeset(&cs2).await.unwrap();
        assert_eq!(applied, 1);

        // Verify backend was updated
        let conn = Connection::open(temp.path()).unwrap();
        let region: String = conn
            .query_row("SELECT region FROM backends WHERE id = 'backend-1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(region, "us");
    }

    #[tokio::test]
    async fn test_apply_changeset_with_backend_delete() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // First insert
        let source_node = NodeId::new("other-node");
        let data = r#"{"app":"myapp","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let changes1 = vec![
            Change::new("backends", "backend-1", ChangeKind::Insert, data, &source_node),
        ];
        let cs1 = ChangeSet::new(source_node.clone(), 1, changes1);
        service.apply_changeset(&cs1).await.unwrap();

        // Small delay
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        // Then delete
        let changes2 = vec![
            Change::new("backends", "backend-1", ChangeKind::Delete, "{}", &source_node),
        ];
        let cs2 = ChangeSet::new(source_node, 2, changes2);
        let applied = service.apply_changeset(&cs2).await.unwrap();
        assert_eq!(applied, 1);

        // Verify backend was soft-deleted
        let conn = Connection::open(temp.path()).unwrap();
        let deleted: i64 = conn
            .query_row("SELECT deleted FROM backends WHERE id = 'backend-1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(deleted, 1);
    }

    #[tokio::test]
    async fn test_apply_changeset_lww_newer_wins() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");

        // Create first change
        let data1 = r#"{"app":"old","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let change1 = Change::new("backends", "backend-1", ChangeKind::Insert, data1, &source_node);
        let cs1 = ChangeSet::new(source_node.clone(), 1, vec![change1]);
        service.apply_changeset(&cs1).await.unwrap();

        // Small delay to get different timestamp
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        // Create second change with newer timestamp
        let data2 = r#"{"app":"new","region":"us","wg_ip":"10.0.0.2","port":9000}"#;
        let change2 = Change::new("backends", "backend-1", ChangeKind::Update, data2, &source_node);
        let cs2 = ChangeSet::new(source_node, 2, vec![change2]);
        let applied = service.apply_changeset(&cs2).await.unwrap();

        assert_eq!(applied, 1);

        // Verify newer value was applied
        let conn = Connection::open(temp.path()).unwrap();
        let app: String = conn
            .query_row("SELECT app FROM backends WHERE id = 'backend-1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(app, "new");
    }

    #[tokio::test]
    async fn test_apply_changeset_unknown_table() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");
        let changes = vec![
            Change::new("unknown_table", "pk1", ChangeKind::Insert, "{}", &source_node),
        ];
        let cs = ChangeSet::new(source_node, 1, changes);

        // Should not fail, just log warning
        let applied = service.apply_changeset(&cs).await.unwrap();
        assert_eq!(applied, 1); // Change was "applied" (to LWW table)
    }

    #[test]
    fn test_get_changes_since() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Currently returns empty (placeholder implementation)
        let result = service.get_changes_since("other-node", 0).unwrap();
        assert!(result.is_empty());
    }

    #[test]
    fn test_persist_version() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Persist version
        service.persist_version("node-1", 42).unwrap();

        // Verify in database
        let conn = Connection::open(temp.path()).unwrap();
        let seq: i64 = conn
            .query_row(
                "SELECT sequence FROM __replication_versions WHERE node_id = 'node-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(seq, 42);

        // Update version
        service.persist_version("node-1", 100).unwrap();

        let seq: i64 = conn
            .query_row(
                "SELECT sequence FROM __replication_versions WHERE node_id = 'node-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(seq, 100);
    }

    #[tokio::test]
    async fn test_apply_changeset_lww_older_ignored() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");

        // Create and apply newer change first (higher timestamp)
        let data_new = r#"{"app":"new","region":"us","wg_ip":"10.0.0.2","port":9000}"#;
        let change_new = Change::new("backends", "backend-1", ChangeKind::Insert, data_new, &source_node);
        let ts_new = change_new.timestamp;
        let cs_new = ChangeSet::new(source_node.clone(), 2, vec![change_new]);
        service.apply_changeset(&cs_new).await.unwrap();

        // Create older change with lower timestamp
        let data_old = r#"{"app":"old","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let mut change_old = Change::new("backends", "backend-1", ChangeKind::Update, data_old, &source_node);
        // Make it older by setting a lower timestamp
        change_old.timestamp = HLCTimestamp {
            wall_time: ts_new.wall_time - 10000, // 10 seconds earlier
            counter: 0,
            node_hash: 1,
        };

        let cs_old = ChangeSet::new(source_node, 3, vec![change_old]);
        let applied = service.apply_changeset(&cs_old).await.unwrap();

        // Old change should NOT be applied due to LWW
        assert_eq!(applied, 0);

        // Verify value is still "new"
        let conn = Connection::open(temp.path()).unwrap();
        let app: String = conn
            .query_row("SELECT app FROM backends WHERE id = 'backend-1'", [], |row| row.get(0))
            .unwrap();
        assert_eq!(app, "new");
    }

    #[tokio::test]
    async fn test_apply_changeset_lww_from_cache() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");

        // Apply first change to populate cache
        let data1 = r#"{"app":"first","region":"sa","wg_ip":"10.0.0.1","port":8080}"#;
        let change1 = Change::new("backends", "backend-1", ChangeKind::Insert, data1, &source_node);
        let ts1 = change1.timestamp;
        let cs1 = ChangeSet::new(source_node.clone(), 1, vec![change1]);
        service.apply_changeset(&cs1).await.unwrap();

        // Verify cache is populated
        let key = "backends:backend-1";
        assert!(service.last_timestamps.read().contains_key(key));

        // Create older change - should be rejected from cache (fast path)
        let data2 = r#"{"app":"old","region":"us","wg_ip":"10.0.0.2","port":9000}"#;
        let mut change2 = Change::new("backends", "backend-1", ChangeKind::Update, data2, &source_node);
        change2.timestamp = HLCTimestamp {
            wall_time: ts1.wall_time - 5000,
            counter: 0,
            node_hash: 1,
        };
        let cs2 = ChangeSet::new(source_node, 2, vec![change2]);
        let applied = service.apply_changeset(&cs2).await.unwrap();

        // Should be rejected by cache check
        assert_eq!(applied, 0);
    }

    #[tokio::test]
    async fn test_apply_changeset_multiple_changes() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");

        // Create multiple changes in one changeset
        let changes = vec![
            Change::new("backends", "b1", ChangeKind::Insert, r#"{"app":"app1","region":"sa","wg_ip":"10.0.0.1","port":8080}"#, &source_node),
            Change::new("backends", "b2", ChangeKind::Insert, r#"{"app":"app2","region":"us","wg_ip":"10.0.0.2","port":8081}"#, &source_node),
            Change::new("backends", "b3", ChangeKind::Insert, r#"{"app":"app3","region":"eu","wg_ip":"10.0.0.3","port":8082}"#, &source_node),
        ];

        let cs = ChangeSet::new(source_node, 1, changes);
        let applied = service.apply_changeset(&cs).await.unwrap();

        assert_eq!(applied, 3);

        // Verify all backends were created
        let conn = Connection::open(temp.path()).unwrap();
        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM backends", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 3);
    }

    #[tokio::test]
    async fn test_flush_sends_broadcast_ready_event() {
        let temp = NamedTempFile::new().unwrap();
        let mut service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        let mut event_rx = service.take_event_rx().unwrap();

        service.init_db().unwrap();

        // Record a change
        service.record_change("backends", "b1", ChangeKind::Insert, "{}");

        // Flush should send BroadcastReady event
        let _cs = service.flush().await;

        // Check for event
        let event = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            event_rx.recv()
        ).await;

        assert!(event.is_ok());
        if let Ok(Some(SyncEvent::BroadcastReady(cs))) = event {
            assert_eq!(cs.seq, 1);
        } else {
            panic!("expected BroadcastReady event");
        }
    }

    #[tokio::test]
    async fn test_apply_changeset_sends_change_applied_event() {
        let temp = NamedTempFile::new().unwrap();
        let mut service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        let mut event_rx = service.take_event_rx().unwrap();

        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");
        let changes = vec![
            Change::new("backends", "b1", ChangeKind::Insert, r#"{"app":"app1","region":"sa","wg_ip":"10.0.0.1","port":8080}"#, &source_node),
        ];
        let cs = ChangeSet::new(source_node, 1, changes);

        service.apply_changeset(&cs).await.unwrap();

        // Should receive ChangeApplied event
        let event = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            event_rx.recv()
        ).await;

        assert!(event.is_ok());
        if let Ok(Some(SyncEvent::ChangeApplied(change))) = event {
            assert_eq!(change.table, "backends");
            assert_eq!(change.pk, "b1");
        } else {
            panic!("expected ChangeApplied event");
        }
    }

    #[tokio::test]
    async fn test_backend_insert_with_all_fields() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");
        let data = r#"{
            "app": "fullapp",
            "region": "eu",
            "country": "DE",
            "wg_ip": "10.50.1.1",
            "port": 9999,
            "healthy": 1,
            "weight": 5,
            "soft_limit": 200,
            "hard_limit": 300
        }"#;

        let changes = vec![
            Change::new("backends", "full-backend", ChangeKind::Insert, data, &source_node),
        ];
        let cs = ChangeSet::new(source_node, 1, changes);
        service.apply_changeset(&cs).await.unwrap();

        // Verify all fields were inserted correctly
        let conn = Connection::open(temp.path()).unwrap();
        let row: (String, String, Option<String>, String, i64, i64, i64, i64, i64) = conn
            .query_row(
                "SELECT app, region, country, wg_ip, port, weight, soft_limit, hard_limit, healthy FROM backends WHERE id = 'full-backend'",
                [],
                |row| Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                    row.get(6)?,
                    row.get(7)?,
                    row.get(8)?,
                )),
            )
            .unwrap();

        assert_eq!(row.0, "fullapp");
        assert_eq!(row.1, "eu");
        assert_eq!(row.2, Some("DE".to_string()));
        assert_eq!(row.3, "10.50.1.1");
        assert_eq!(row.4, 9999);
        assert_eq!(row.5, 5);
        assert_eq!(row.6, 200);
        assert_eq!(row.7, 300);
        assert_eq!(row.8, 1);
    }

    #[tokio::test]
    async fn test_backend_insert_with_defaults() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        let source_node = NodeId::new("other-node");
        // Minimal data - defaults should be used
        let data = r#"{"app": "minapp"}"#;

        let changes = vec![
            Change::new("backends", "minimal-backend", ChangeKind::Insert, data, &source_node),
        ];
        let cs = ChangeSet::new(source_node, 1, changes);
        service.apply_changeset(&cs).await.unwrap();

        // Verify defaults were used
        let conn = Connection::open(temp.path()).unwrap();
        let row: (String, i64, i64, i64) = conn
            .query_row(
                "SELECT region, weight, soft_limit, hard_limit FROM backends WHERE id = 'minimal-backend'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .unwrap();

        assert_eq!(row.0, "us"); // Default region
        assert_eq!(row.1, 2);    // Default weight
        assert_eq!(row.2, 100);  // Default soft_limit
        assert_eq!(row.3, 150);  // Default hard_limit
    }

    #[test]
    fn test_replication_log_table_created() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );
        service.init_db().unwrap();

        // Verify replication_log table structure
        let conn = Connection::open(temp.path()).unwrap();
        let columns: Vec<String> = conn
            .prepare("PRAGMA table_info(__replication_log)")
            .unwrap()
            .query_map([], |row| row.get(1))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(columns.contains(&"change_id".to_string()));
        assert!(columns.contains(&"table_name".to_string()));
        assert!(columns.contains(&"pk".to_string()));
        assert!(columns.contains(&"kind".to_string()));
        assert!(columns.contains(&"data".to_string()));
        assert!(columns.contains(&"origin_node".to_string()));
    }
}
