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
    fn test_version_vector() {
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
    fn test_sync_service_creation() {
        let temp = NamedTempFile::new().unwrap();
        let service = SyncService::new(
            NodeId::new("test-node"),
            temp.path().to_str().unwrap().to_string(),
        );

        assert_eq!(service.sequence(), 0);
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
}
