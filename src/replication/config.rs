//! Replication Configuration
//!
//! Configuration for the built-in replication system.

use std::net::SocketAddr;
use std::time::Duration;

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
}
