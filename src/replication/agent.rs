//! Replication Agent
//!
//! Orchestrates all replication components (gossip, sync, transport) to provide
//! a unified interface for distributed state management.

use crate::replication::config::ReplicationConfig;
use crate::replication::gossip::{GossipService, Member};
use crate::replication::sync::SyncService;
use crate::replication::transport::TransportService;
use crate::replication::types::{Change, ChangeKind, ChangeSet, Message, NodeId};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use tokio::sync::{mpsc, RwLock};
use tokio::time::interval;

/// Events emitted by the replication agent.
#[derive(Debug, Clone)]
pub enum ReplicationEvent {
    /// Successfully joined the cluster
    ClusterJoined { members: usize },
    /// A peer joined the cluster
    PeerJoined(NodeId),
    /// A peer left the cluster
    PeerLeft(NodeId),
    /// A change was applied locally
    ChangeApplied(Change),
    /// Replication error occurred
    Error(String),
}

/// Replication agent that orchestrates all components.
pub struct ReplicationAgent {
    config: ReplicationConfig,
    node_id: NodeId,
    gossip: Arc<GossipService>,
    sync: Arc<SyncService>,
    transport: Arc<RwLock<TransportService>>,
    event_tx: mpsc::Sender<ReplicationEvent>,
    event_rx: Option<mpsc::Receiver<ReplicationEvent>>,
    shutdown: Arc<AtomicBool>,
}

impl ReplicationAgent {
    /// Create a new replication agent.
    pub fn new(config: ReplicationConfig) -> anyhow::Result<Self> {
        config.validate()?;

        let node_id = NodeId::new(&config.node_id);
        let (event_tx, event_rx) = mpsc::channel(1024);

        let gossip = Arc::new(GossipService::new(config.clone()));
        let sync = Arc::new(SyncService::new(node_id.clone(), config.db_path.clone()));
        let transport = Arc::new(RwLock::new(TransportService::new(config.clone())));

        Ok(Self {
            config,
            node_id,
            gossip,
            sync,
            transport,
            event_tx,
            event_rx: Some(event_rx),
            shutdown: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Get the event receiver.
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<ReplicationEvent>> {
        self.event_rx.take()
    }

    /// Get the node ID.
    pub fn node_id(&self) -> &NodeId {
        &self.node_id
    }

    /// Get current cluster members.
    pub fn members(&self) -> Vec<Member> {
        self.gossip.members()
    }

    /// Get alive cluster members.
    pub fn alive_members(&self) -> Vec<Member> {
        self.gossip.alive_members()
    }

    /// Check if replication is enabled/running.
    pub fn is_running(&self) -> bool {
        !self.shutdown.load(Ordering::SeqCst)
    }

    /// Start the replication agent.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            "starting replication agent node_id={} gossip={} transport={}",
            self.config.node_id,
            self.config.gossip_addr,
            self.config.transport_addr
        );

        // Initialize sync database
        self.sync.init_db()?;

        // Start transport
        {
            let mut transport = self.transport.write().await;
            transport.start().await?;
        }

        // Start gossip
        self.gossip.clone().start().await?;

        // Start event processing
        self.start_event_loop();

        // Start periodic flush
        self.start_flush_loop();

        // Notify joined
        let members = self.gossip.alive_members().len();
        let _ = self.event_tx.send(ReplicationEvent::ClusterJoined { members }).await;

        tracing::info!("replication agent started");
        Ok(())
    }

    /// Stop the replication agent.
    pub async fn stop(&self) {
        tracing::info!("stopping replication agent");
        self.shutdown.store(true, Ordering::SeqCst);
        self.gossip.shutdown();
        self.transport.read().await.shutdown();
    }

    /// Record a backend change for replication.
    pub fn record_backend_change(&self, id: &str, kind: ChangeKind, data: &str) {
        self.sync.record_change("backends", id, kind, data);
    }

    /// Flush pending changes and broadcast.
    pub async fn flush(&self) -> Option<ChangeSet> {
        let changeset = self.sync.flush().await?;

        // Broadcast to all peers - read lock, collect peers, drop lock, then broadcast
        let sent = {
            let transport = self.transport.read().await;
            transport.broadcast_changeset(&changeset).await
        };

        tracing::debug!(
            "flushed changeset seq={} changes={} sent_to={}",
            changeset.seq,
            changeset.changes.len(),
            sent
        );

        Some(changeset)
    }

    /// Apply a received changeset.
    pub async fn apply_changeset(&self, changeset: &ChangeSet) -> anyhow::Result<usize> {
        self.sync.apply_changeset(changeset).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn start_event_loop(&self) {
        let gossip = self.gossip.clone();
        let transport = self.transport.clone();
        let shutdown = self.shutdown.clone();

        tokio::spawn(async move {
            let mut check_interval = interval(Duration::from_millis(100));

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                check_interval.tick().await;

                // In a full implementation, we would:
                // 1. Process gossip events (member joins/leaves)
                // 2. Process transport events (received messages)
                // 3. Process sync events (applied changes)
                // 4. Connect to newly discovered peers

                // For now, just check gossip members periodically
                let members = gossip.alive_members();
                for member in members {
                    // Ensure we have transport connections to alive members
                    let has_connection = transport.read().await.get_peer(&member.node_id.0).await.is_some();
                    if !has_connection && member.node_id.0 != "self" {
                        tracing::debug!("peer {} not yet connected", member.node_id);
                    }
                }
            }
        });
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    fn start_flush_loop(&self) {
        let sync = self.sync.clone();
        let transport = self.transport.clone();
        let shutdown = self.shutdown.clone();
        let flush_interval = self.config.sync_interval;

        tokio::spawn(async move {
            let mut timer = interval(flush_interval);

            loop {
                if shutdown.load(Ordering::SeqCst) {
                    break;
                }

                timer.tick().await;

                // Flush pending changes
                if let Some(changeset) = sync.flush().await {
                    // Broadcast to all peers
                    let sent = {
                        let transport_guard = transport.read().await;
                        transport_guard.broadcast_changeset(&changeset).await
                    };

                    if sent > 0 {
                        tracing::debug!(
                            "periodic flush: seq={} changes={} sent_to={}",
                            changeset.seq,
                            changeset.changes.len(),
                            sent
                        );
                    }
                }
            }
        });
    }
}

/// Builder for ReplicationAgent with fluent API.
pub struct ReplicationAgentBuilder {
    config: ReplicationConfig,
}

impl ReplicationAgentBuilder {
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            config: ReplicationConfig::new(node_id),
        }
    }

    pub fn gossip_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.config = self.config.gossip_addr(addr);
        self
    }

    pub fn transport_addr(mut self, addr: std::net::SocketAddr) -> Self {
        self.config = self.config.transport_addr(addr);
        self
    }

    pub fn bootstrap_peers(mut self, peers: Vec<String>) -> Self {
        self.config = self.config.bootstrap_peers(peers);
        self
    }

    pub fn db_path(mut self, path: impl Into<String>) -> Self {
        self.config = self.config.db_path(path);
        self
    }

    pub fn cluster_name(mut self, name: impl Into<String>) -> Self {
        self.config = self.config.cluster_name(name);
        self
    }

    pub fn build(self) -> anyhow::Result<ReplicationAgent> {
        ReplicationAgent::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_agent_creation() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config);
        assert!(agent.is_ok());

        let agent = agent.unwrap();
        assert_eq!(agent.node_id().as_str(), "test-node");
        assert!(agent.is_running());
    }

    #[tokio::test]
    async fn test_agent_stop() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        assert!(agent.is_running());

        agent.stop().await;
        assert!(!agent.is_running());
    }

    #[test]
    fn test_agent_take_event_rx() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let mut agent = ReplicationAgent::new(config).unwrap();

        // First call should return Some
        let rx = agent.take_event_rx();
        assert!(rx.is_some());

        // Second call should return None
        let rx = agent.take_event_rx();
        assert!(rx.is_none());
    }

    #[test]
    fn test_builder_pattern() {
        let temp = NamedTempFile::new().unwrap();

        let agent = ReplicationAgentBuilder::new("pop-sa-1")
            .gossip_addr("127.0.0.1:4001".parse().unwrap())
            .transport_addr("127.0.0.1:4002".parse().unwrap())
            .db_path(temp.path().to_str().unwrap())
            .cluster_name("my-cluster")
            .build();

        assert!(agent.is_ok());
        let agent = agent.unwrap();
        assert_eq!(agent.node_id().as_str(), "pop-sa-1");
    }

    #[test]
    fn test_builder_with_bootstrap_peers() {
        let temp = NamedTempFile::new().unwrap();

        let agent = ReplicationAgentBuilder::new("pop-sa-1")
            .gossip_addr("127.0.0.1:4001".parse().unwrap())
            .transport_addr("127.0.0.1:4002".parse().unwrap())
            .db_path(temp.path().to_str().unwrap())
            .bootstrap_peers(vec!["10.0.0.1:4001".to_string(), "10.0.0.2:4001".to_string()])
            .build();

        assert!(agent.is_ok());
    }

    #[test]
    fn test_agent_validation_fails_without_node_id() {
        let config = ReplicationConfig::default();
        let result = ReplicationAgent::new(config);
        assert!(result.is_err());
    }

    #[test]
    fn test_agent_validation_fails_without_cluster_name() {
        let config = ReplicationConfig::new("test-node")
            .cluster_name("");
        let result = ReplicationAgent::new(config);
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_record_and_flush() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        agent.sync.init_db().unwrap();

        // Record a change
        agent.record_backend_change(
            "backend-1",
            ChangeKind::Insert,
            r#"{"app":"myapp","region":"sa","wg_ip":"10.0.0.1","port":8080}"#,
        );

        // Flush
        let changeset = agent.flush().await;
        assert!(changeset.is_some());

        let cs = changeset.unwrap();
        assert_eq!(cs.changes.len(), 1);
        assert_eq!(cs.changes[0].pk, "backend-1");
    }

    #[tokio::test]
    async fn test_record_multiple_changes_and_flush() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        agent.sync.init_db().unwrap();

        // Record multiple changes
        agent.record_backend_change("b1", ChangeKind::Insert, r#"{"app":"app1"}"#);
        agent.record_backend_change("b2", ChangeKind::Insert, r#"{"app":"app2"}"#);
        agent.record_backend_change("b1", ChangeKind::Update, r#"{"app":"app1-updated"}"#);

        // Flush
        let changeset = agent.flush().await;
        assert!(changeset.is_some());

        let cs = changeset.unwrap();
        assert_eq!(cs.changes.len(), 3);
    }

    #[tokio::test]
    async fn test_flush_empty() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        agent.sync.init_db().unwrap();

        // Flush with no changes
        let changeset = agent.flush().await;
        assert!(changeset.is_none());
    }

    #[test]
    fn test_members_empty_initially() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        assert!(agent.members().is_empty());
        assert!(agent.alive_members().is_empty());
    }

    #[tokio::test]
    async fn test_apply_changeset() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        agent.sync.init_db().unwrap();

        // Create a changeset from another node
        let source_node = NodeId::new("other-node");
        let data = r#"{"app":"remote-app","region":"us","wg_ip":"10.0.0.5","port":9000}"#;
        let changes = vec![
            Change::new("backends", "remote-backend", ChangeKind::Insert, data, &source_node),
        ];
        let cs = ChangeSet::new(source_node, 1, changes);

        // Apply it
        let applied = agent.apply_changeset(&cs).await.unwrap();
        assert_eq!(applied, 1);
    }

    #[tokio::test]
    async fn test_agent_start_and_stop() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap())
            .gossip_addr("127.0.0.1:0".parse().unwrap())
            .transport_addr("127.0.0.1:0".parse().unwrap());

        let mut agent = ReplicationAgent::new(config).unwrap();

        // Start should succeed
        let result = agent.start().await;
        assert!(result.is_ok());

        // Should still be running
        assert!(agent.is_running());

        // Stop
        agent.stop().await;
        assert!(!agent.is_running());
    }

    #[test]
    fn test_replication_event_cluster_joined_debug() {
        let event = ReplicationEvent::ClusterJoined { members: 5 };
        let debug = format!("{:?}", event);
        assert!(debug.contains("ClusterJoined"));
        assert!(debug.contains("5"));
    }

    #[test]
    fn test_replication_event_peer_joined_debug() {
        let event = ReplicationEvent::PeerJoined(NodeId::new("peer-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerJoined"));
        assert!(debug.contains("peer-1"));
    }

    #[test]
    fn test_replication_event_peer_left_debug() {
        let event = ReplicationEvent::PeerLeft(NodeId::new("peer-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerLeft"));
        assert!(debug.contains("peer-1"));
    }

    #[test]
    fn test_replication_event_change_applied_debug() {
        let change = Change::new("backends", "b1", ChangeKind::Insert, "{}", &NodeId::new("node-1"));
        let event = ReplicationEvent::ChangeApplied(change);
        let debug = format!("{:?}", event);
        assert!(debug.contains("ChangeApplied"));
    }

    #[test]
    fn test_replication_event_error_debug() {
        let event = ReplicationEvent::Error("test error".to_string());
        let debug = format!("{:?}", event);
        assert!(debug.contains("Error"));
        assert!(debug.contains("test error"));
    }

    #[test]
    fn test_replication_event_clone() {
        let event = ReplicationEvent::ClusterJoined { members: 3 };
        let cloned = event.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("ClusterJoined"));
    }

    #[test]
    fn test_builder_default_values() {
        let temp = NamedTempFile::new().unwrap();

        // Builder with minimal config
        let agent = ReplicationAgentBuilder::new("minimal-node")
            .db_path(temp.path().to_str().unwrap())
            .build();

        assert!(agent.is_ok());
        let agent = agent.unwrap();
        assert_eq!(agent.node_id().as_str(), "minimal-node");
    }

    #[tokio::test]
    async fn test_agent_with_bootstrap_peers_start() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap())
            .gossip_addr("127.0.0.1:0".parse().unwrap())
            .transport_addr("127.0.0.1:0".parse().unwrap())
            .bootstrap_peers(vec!["127.0.0.1:9999".to_string()]);

        let mut agent = ReplicationAgent::new(config).unwrap();
        let result = agent.start().await;
        assert!(result.is_ok());

        // Give it a moment
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        agent.stop().await;
    }

    #[tokio::test]
    async fn test_agent_record_change_kinds() {
        let temp = NamedTempFile::new().unwrap();
        let config = ReplicationConfig::new("test-node")
            .db_path(temp.path().to_str().unwrap());

        let agent = ReplicationAgent::new(config).unwrap();
        agent.sync.init_db().unwrap();

        // Test all change kinds
        agent.record_backend_change("b1", ChangeKind::Insert, r#"{"app":"a"}"#);
        agent.record_backend_change("b1", ChangeKind::Update, r#"{"app":"b"}"#);
        agent.record_backend_change("b1", ChangeKind::Delete, "{}");

        let cs = agent.flush().await.unwrap();
        assert_eq!(cs.changes.len(), 3);
        assert_eq!(cs.changes[0].kind, ChangeKind::Insert);
        assert_eq!(cs.changes[1].kind, ChangeKind::Update);
        assert_eq!(cs.changes[2].kind, ChangeKind::Delete);
    }
}
