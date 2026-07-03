//! Replication Configuration
//!
//! Configuration for the built-in replication system.

use std::net::SocketAddr;
use std::time::Duration;

/// Mode of operation for a replication node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReplicaMode {
    /// Full read-write node, participates in broadcasts
    #[default]
    Primary,
    /// Read-only node, receives changes but doesn't broadcast
    ReadReplica,
}

impl ReplicaMode {
    /// Check if this node can write (record local changes).
    pub fn can_write(&self) -> bool {
        matches!(self, ReplicaMode::Primary)
    }

    /// Check if this node should broadcast changes.
    pub fn should_broadcast(&self) -> bool {
        matches!(self, ReplicaMode::Primary)
    }
}

impl std::str::FromStr for ReplicaMode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "primary" | "master" | "rw" => Ok(ReplicaMode::Primary),
            "replica" | "read-replica" | "readonly" | "ro" => Ok(ReplicaMode::ReadReplica),
            _ => Err(format!("unknown replica mode: '{}'. Use 'primary' or 'replica'", s)),
        }
    }
}

/// Configuration for the replication agent.
#[derive(Debug, Clone)]
pub struct ReplicationConfig {
    /// Unique identifier for this node (e.g., "pop-sa-1")
    pub node_id: String,

    /// Address to bind for gossip protocol (default: 0.0.0.0:4001)
    pub gossip_addr: SocketAddr,

    /// Address to bind for QUIC transport (default: 0.0.0.0:4002)
    pub transport_addr: SocketAddr,

    /// Bootstrap peers to join the cluster (e.g., ["pop-us.example.com:4001"])
    pub bootstrap_peers: Vec<String>,

    /// Path to the SQLite database for replication state
    pub db_path: String,

    /// Cluster name for isolation (default: "edgeproxy")
    pub cluster_name: String,

    /// Gossip protocol interval (default: 500ms)
    pub gossip_interval: Duration,

    /// Sync interval for change broadcast (default: 100ms)
    pub sync_interval: Duration,

    /// Maximum pending changes before forced flush (default: 1000)
    pub max_pending_changes: usize,

    /// Rate limit for broadcasts in bytes/sec (default: 10MB/s)
    pub broadcast_rate_limit: u64,

    /// Enable TLS for transport (default: true)
    pub tls_enabled: bool,

    /// Replica mode (default: Primary)
    pub replica_mode: ReplicaMode,

    /// Enable mDNS auto-discovery (default: true)
    pub mdns_enabled: bool,

    /// mDNS service type (default: "_edgeproxy._udp.local.")
    pub mdns_service_type: String,
}

impl Default for ReplicationConfig {
    fn default() -> Self {
        Self {
            node_id: String::new(),
            gossip_addr: "0.0.0.0:4001".parse().unwrap(),
            transport_addr: "0.0.0.0:4002".parse().unwrap(),
            bootstrap_peers: Vec::new(),
            db_path: "state.db".to_string(),
            cluster_name: "edgeproxy".to_string(),
            gossip_interval: Duration::from_millis(500),
            sync_interval: Duration::from_millis(100),
            max_pending_changes: 1000,
            broadcast_rate_limit: 10 * 1024 * 1024, // 10 MB/s
            tls_enabled: true,
            replica_mode: ReplicaMode::Primary,
            mdns_enabled: true,
            mdns_service_type: "_edgeproxy._udp.local.".to_string(),
        }
    }
}

impl ReplicationConfig {
    /// Create a new configuration with node ID.
    pub fn new(node_id: impl Into<String>) -> Self {
        Self {
            node_id: node_id.into(),
            ..Default::default()
        }
    }

    /// Set the gossip address.
    pub fn gossip_addr(mut self, addr: SocketAddr) -> Self {
        self.gossip_addr = addr;
        self
    }

    /// Set the transport address.
    pub fn transport_addr(mut self, addr: SocketAddr) -> Self {
        self.transport_addr = addr;
        self
    }

    /// Add bootstrap peers.
    pub fn bootstrap_peers(mut self, peers: Vec<String>) -> Self {
        self.bootstrap_peers = peers;
        self
    }

    /// Set the database path.
    pub fn db_path(mut self, path: impl Into<String>) -> Self {
        self.db_path = path.into();
        self
    }

    /// Set the cluster name.
    pub fn cluster_name(mut self, name: impl Into<String>) -> Self {
        self.cluster_name = name.into();
        self
    }

    /// Set the replica mode.
    pub fn replica_mode(mut self, mode: ReplicaMode) -> Self {
        self.replica_mode = mode;
        self
    }

    /// Enable or disable mDNS discovery.
    pub fn mdns_enabled(mut self, enabled: bool) -> Self {
        self.mdns_enabled = enabled;
        self
    }

    /// Set the mDNS service type.
    pub fn mdns_service_type(mut self, service_type: impl Into<String>) -> Self {
        self.mdns_service_type = service_type.into();
        self
    }

    /// Check if mDNS discovery is enabled.
    pub fn is_mdns_enabled(&self) -> bool {
        self.mdns_enabled
    }

    /// Check if this node is a read replica.
    pub fn is_read_replica(&self) -> bool {
        self.replica_mode == ReplicaMode::ReadReplica
    }

    /// Check if this node is a primary.
    pub fn is_primary(&self) -> bool {
        self.replica_mode == ReplicaMode::Primary
    }

    /// Validate the configuration.
    pub fn validate(&self) -> Result<(), ConfigError> {
        if self.node_id.is_empty() {
            return Err(ConfigError::MissingNodeId);
        }
        if self.cluster_name.is_empty() {
            return Err(ConfigError::MissingClusterName);
        }
        Ok(())
    }
}

/// Configuration validation errors.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ConfigError {
    #[error("node_id is required")]
    MissingNodeId,
    #[error("cluster_name is required")]
    MissingClusterName,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = ReplicationConfig::default();
        assert!(config.node_id.is_empty());
        assert_eq!(config.gossip_addr.port(), 4001);
        assert_eq!(config.transport_addr.port(), 4002);
        assert_eq!(config.cluster_name, "edgeproxy");
    }

    #[test]
    fn test_builder_pattern() {
        let config = ReplicationConfig::new("pop-sa-1")
            .gossip_addr("0.0.0.0:5001".parse().unwrap())
            .bootstrap_peers(vec!["peer1:4001".to_string()])
            .cluster_name("myproxy");

        assert_eq!(config.node_id, "pop-sa-1");
        assert_eq!(config.gossip_addr.port(), 5001);
        assert_eq!(config.bootstrap_peers.len(), 1);
        assert_eq!(config.cluster_name, "myproxy");
    }

    #[test]
    fn test_validate_missing_node_id() {
        let config = ReplicationConfig::default();
        let result = config.validate();
        assert!(matches!(result, Err(ConfigError::MissingNodeId)));
    }

    #[test]
    fn test_validate_missing_cluster_name() {
        let config = ReplicationConfig::new("node-1").cluster_name("");
        let result = config.validate();
        assert!(matches!(result, Err(ConfigError::MissingClusterName)));
    }

    #[test]
    fn test_validate_ok() {
        let config = ReplicationConfig::new("node-1");
        assert!(config.validate().is_ok());
    }

    // ==================== ReplicaMode Tests ====================

    #[test]
    fn test_replica_mode_default() {
        let mode = ReplicaMode::default();
        assert_eq!(mode, ReplicaMode::Primary);
    }

    #[test]
    fn test_replica_mode_can_write() {
        assert!(ReplicaMode::Primary.can_write());
        assert!(!ReplicaMode::ReadReplica.can_write());
    }

    #[test]
    fn test_replica_mode_should_broadcast() {
        assert!(ReplicaMode::Primary.should_broadcast());
        assert!(!ReplicaMode::ReadReplica.should_broadcast());
    }

    #[test]
    fn test_replica_mode_from_str_primary() {
        assert_eq!("primary".parse::<ReplicaMode>().unwrap(), ReplicaMode::Primary);
        assert_eq!("master".parse::<ReplicaMode>().unwrap(), ReplicaMode::Primary);
        assert_eq!("rw".parse::<ReplicaMode>().unwrap(), ReplicaMode::Primary);
        assert_eq!("PRIMARY".parse::<ReplicaMode>().unwrap(), ReplicaMode::Primary);
    }

    #[test]
    fn test_replica_mode_from_str_replica() {
        assert_eq!("replica".parse::<ReplicaMode>().unwrap(), ReplicaMode::ReadReplica);
        assert_eq!("read-replica".parse::<ReplicaMode>().unwrap(), ReplicaMode::ReadReplica);
        assert_eq!("readonly".parse::<ReplicaMode>().unwrap(), ReplicaMode::ReadReplica);
        assert_eq!("ro".parse::<ReplicaMode>().unwrap(), ReplicaMode::ReadReplica);
        assert_eq!("REPLICA".parse::<ReplicaMode>().unwrap(), ReplicaMode::ReadReplica);
    }

    #[test]
    fn test_replica_mode_from_str_invalid() {
        let result = "invalid".parse::<ReplicaMode>();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("unknown replica mode"));
    }

    #[test]
    fn test_config_with_replica_mode() {
        let config = ReplicationConfig::new("node-1")
            .replica_mode(ReplicaMode::ReadReplica);

        assert!(config.is_read_replica());
        assert!(!config.is_primary());
        assert!(!config.replica_mode.can_write());
    }

    #[test]
    fn test_config_default_is_primary() {
        let config = ReplicationConfig::new("node-1");
        assert!(config.is_primary());
        assert!(!config.is_read_replica());
    }

    // ==================== mDNS Config Tests ====================

    #[test]
    fn test_mdns_default_enabled() {
        let config = ReplicationConfig::default();
        assert!(config.mdns_enabled);
        assert!(config.is_mdns_enabled());
    }

    #[test]
    fn test_mdns_default_service_type() {
        let config = ReplicationConfig::default();
        assert_eq!(config.mdns_service_type, "_edgeproxy._udp.local.");
    }

    #[test]
    fn test_mdns_enabled_builder() {
        let config = ReplicationConfig::new("node-1")
            .mdns_enabled(false);

        assert!(!config.mdns_enabled);
        assert!(!config.is_mdns_enabled());
    }

    #[test]
    fn test_mdns_service_type_builder() {
        let config = ReplicationConfig::new("node-1")
            .mdns_service_type("_custom._tcp.local.");

        assert_eq!(config.mdns_service_type, "_custom._tcp.local.");
    }

    #[test]
    fn test_mdns_combined_with_other_options() {
        let config = ReplicationConfig::new("node-1")
            .cluster_name("test-cluster")
            .mdns_enabled(true)
            .mdns_service_type("_myapp._udp.local.")
            .replica_mode(ReplicaMode::ReadReplica);

        assert!(config.is_mdns_enabled());
        assert_eq!(config.mdns_service_type, "_myapp._udp.local.");
        assert!(config.is_read_replica());
        assert_eq!(config.cluster_name, "test-cluster");
    }
}
