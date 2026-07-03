//! mDNS Discovery for Automatic Cluster Formation
//!
//! Uses multicast DNS (mDNS) to automatically discover peers on the local network.
//! Peers register themselves as mDNS services and browse for other peers.
//!
//! ## How It Works
//!
//! 1. Each node registers itself as an mDNS service with gossip/transport addresses
//! 2. Each node browses for other services matching the cluster name
//! 3. Discovered peers are added to the bootstrap list automatically
//!
//! ## Service Properties
//!
//! - Service Type: `_edgeproxy._udp.local.` (configurable)
//! - Service Name: `{node_id}.{cluster_name}._edgeproxy._udp.local.`
//! - TXT Records:
//!   - `node_id`: Unique node identifier
//!   - `cluster`: Cluster name for isolation
//!   - `gossip`: Gossip address (host:port)
//!   - `transport`: Transport address (host:port)

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::mpsc;

use super::config::ReplicationConfig;

/// A peer discovered via mDNS.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DiscoveredPeer {
    /// Unique node identifier
    pub node_id: String,
    /// Cluster name
    pub cluster: String,
    /// Gossip protocol address
    pub gossip_addr: SocketAddr,
    /// Transport (QUIC) address
    pub transport_addr: SocketAddr,
}

impl DiscoveredPeer {
    /// Create a new discovered peer.
    pub fn new(
        node_id: impl Into<String>,
        cluster: impl Into<String>,
        gossip_addr: SocketAddr,
        transport_addr: SocketAddr,
    ) -> Self {
        Self {
            node_id: node_id.into(),
            cluster: cluster.into(),
            gossip_addr,
            transport_addr,
        }
    }
}

/// mDNS discovery service for automatic peer discovery.
///
/// Uses the mdns-sd crate to register this node and discover peers.
pub struct MdnsDiscovery {
    config: ReplicationConfig,
    discovered_tx: mpsc::Sender<DiscoveredPeer>,
    #[allow(dead_code)]
    discovered_rx: Option<mpsc::Receiver<DiscoveredPeer>>,
}

impl MdnsDiscovery {
    /// Create a new mDNS discovery service.
    ///
    /// Returns the service and a receiver for discovered peers.
    pub fn new(config: ReplicationConfig) -> (Self, mpsc::Receiver<DiscoveredPeer>) {
        let (discovered_tx, discovered_rx) = mpsc::channel(100);
        let service = Self {
            config,
            discovered_tx,
            discovered_rx: None,
        };
        (service, discovered_rx)
    }

    /// Get the service type for mDNS registration.
    pub fn service_type(&self) -> &str {
        &self.config.mdns_service_type
    }

    /// Get the service name for this node.
    pub fn service_name(&self) -> String {
        format!(
            "{}.{}",
            self.config.node_id,
            self.config.cluster_name
        )
    }

    /// Build TXT record properties for service registration.
    pub fn txt_properties(&self) -> Vec<(String, String)> {
        vec![
            ("node_id".to_string(), self.config.node_id.clone()),
            ("cluster".to_string(), self.config.cluster_name.clone()),
            ("gossip".to_string(), self.config.gossip_addr.to_string()),
            ("transport".to_string(), self.config.transport_addr.to_string()),
        ]
    }

    /// Parse a discovered peer from TXT record properties.
    pub fn parse_peer(properties: &[(String, String)]) -> Option<DiscoveredPeer> {
        let mut node_id = None;
        let mut cluster = None;
        let mut gossip_addr = None;
        let mut transport_addr = None;

        for (key, value) in properties {
            match key.as_str() {
                "node_id" => node_id = Some(value.clone()),
                "cluster" => cluster = Some(value.clone()),
                "gossip" => gossip_addr = value.parse().ok(),
                "transport" => transport_addr = value.parse().ok(),
                _ => {}
            }
        }

        Some(DiscoveredPeer {
            node_id: node_id?,
            cluster: cluster?,
            gossip_addr: gossip_addr?,
            transport_addr: transport_addr?,
        })
    }

    /// Notify about a discovered peer (for integration with mdns-sd callbacks).
    pub async fn notify_discovered(&self, peer: DiscoveredPeer) {
        // Filter out self
        if peer.node_id == self.config.node_id {
            return;
        }

        // Filter out peers from different clusters
        if peer.cluster != self.config.cluster_name {
            tracing::debug!(
                "ignoring peer from different cluster: {} (ours: {})",
                peer.cluster,
                self.config.cluster_name
            );
            return;
        }

        tracing::info!(
            "discovered peer via mDNS: {} at gossip={}, transport={}",
            peer.node_id,
            peer.gossip_addr,
            peer.transport_addr
        );

        if let Err(e) = self.discovered_tx.send(peer).await {
            tracing::warn!("failed to send discovered peer: {}", e);
        }
    }

    /// Get a sender for discovered peers (for use in callbacks).
    pub fn discovered_sender(&self) -> mpsc::Sender<DiscoveredPeer> {
        self.discovered_tx.clone()
    }

    /// Check if mDNS is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.mdns_enabled
    }

    /// Get the node ID.
    pub fn node_id(&self) -> &str {
        &self.config.node_id
    }

    /// Get the cluster name.
    pub fn cluster_name(&self) -> &str {
        &self.config.cluster_name
    }

    /// Get the gossip address.
    pub fn gossip_addr(&self) -> SocketAddr {
        self.config.gossip_addr
    }

    /// Get the transport address.
    pub fn transport_addr(&self) -> SocketAddr {
        self.config.transport_addr
    }
}

/// Handle to control the mDNS service.
#[derive(Clone)]
pub struct MdnsHandle {
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
}

impl MdnsHandle {
    /// Create a new handle.
    pub fn new() -> (Self, tokio::sync::watch::Receiver<bool>) {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        (
            Self {
                shutdown_tx: Arc::new(shutdown_tx),
            },
            shutdown_rx,
        )
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }
}

impl Default for MdnsHandle {
    fn default() -> Self {
        Self::new().0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> ReplicationConfig {
        ReplicationConfig::new("test-node")
            .cluster_name("test-cluster")
            .mdns_enabled(true)
    }

    #[test]
    fn test_discovered_peer_new() {
        let peer = DiscoveredPeer::new(
            "node-1",
            "cluster-1",
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );

        assert_eq!(peer.node_id, "node-1");
        assert_eq!(peer.cluster, "cluster-1");
        assert_eq!(peer.gossip_addr.port(), 4001);
        assert_eq!(peer.transport_addr.port(), 4002);
    }

    #[test]
    fn test_discovered_peer_eq() {
        let peer1 = DiscoveredPeer::new(
            "node-1",
            "cluster-1",
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );
        let peer2 = peer1.clone();
        let peer3 = DiscoveredPeer::new(
            "node-2",
            "cluster-1",
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );

        assert_eq!(peer1, peer2);
        assert_ne!(peer1, peer3);
    }

    #[test]
    fn test_mdns_discovery_new() {
        let config = test_config();
        let (discovery, _rx) = MdnsDiscovery::new(config);

        assert!(discovery.is_enabled());
        assert_eq!(discovery.node_id(), "test-node");
        assert_eq!(discovery.cluster_name(), "test-cluster");
    }

    #[test]
    fn test_service_type() {
        let config = test_config();
        let (discovery, _rx) = MdnsDiscovery::new(config);

        assert_eq!(discovery.service_type(), "_edgeproxy._udp.local.");
    }

    #[test]
    fn test_service_name() {
        let config = test_config();
        let (discovery, _rx) = MdnsDiscovery::new(config);

        assert_eq!(discovery.service_name(), "test-node.test-cluster");
    }

    #[test]
    fn test_txt_properties() {
        let config = ReplicationConfig::new("node-1")
            .cluster_name("my-cluster")
            .gossip_addr("192.168.1.1:4001".parse().unwrap())
            .transport_addr("192.168.1.1:4002".parse().unwrap());

        let (discovery, _rx) = MdnsDiscovery::new(config);
        let props = discovery.txt_properties();

        assert_eq!(props.len(), 4);
        assert!(props.contains(&("node_id".to_string(), "node-1".to_string())));
        assert!(props.contains(&("cluster".to_string(), "my-cluster".to_string())));
        assert!(props.contains(&("gossip".to_string(), "192.168.1.1:4001".to_string())));
        assert!(props.contains(&("transport".to_string(), "192.168.1.1:4002".to_string())));
    }

    #[test]
    fn test_parse_peer_success() {
        let props = vec![
            ("node_id".to_string(), "peer-1".to_string()),
            ("cluster".to_string(), "cluster-1".to_string()),
            ("gossip".to_string(), "10.0.0.1:4001".to_string()),
            ("transport".to_string(), "10.0.0.1:4002".to_string()),
        ];

        let peer = MdnsDiscovery::parse_peer(&props).unwrap();

        assert_eq!(peer.node_id, "peer-1");
        assert_eq!(peer.cluster, "cluster-1");
        assert_eq!(peer.gossip_addr.to_string(), "10.0.0.1:4001");
        assert_eq!(peer.transport_addr.to_string(), "10.0.0.1:4002");
    }

    #[test]
    fn test_parse_peer_missing_field() {
        let props = vec![
            ("node_id".to_string(), "peer-1".to_string()),
            ("cluster".to_string(), "cluster-1".to_string()),
            // Missing gossip and transport
        ];

        let peer = MdnsDiscovery::parse_peer(&props);
        assert!(peer.is_none());
    }

    #[test]
    fn test_parse_peer_invalid_address() {
        let props = vec![
            ("node_id".to_string(), "peer-1".to_string()),
            ("cluster".to_string(), "cluster-1".to_string()),
            ("gossip".to_string(), "invalid-address".to_string()),
            ("transport".to_string(), "10.0.0.1:4002".to_string()),
        ];

        let peer = MdnsDiscovery::parse_peer(&props);
        assert!(peer.is_none());
    }

    #[test]
    fn test_parse_peer_extra_fields() {
        let props = vec![
            ("node_id".to_string(), "peer-1".to_string()),
            ("cluster".to_string(), "cluster-1".to_string()),
            ("gossip".to_string(), "10.0.0.1:4001".to_string()),
            ("transport".to_string(), "10.0.0.1:4002".to_string()),
            ("extra".to_string(), "ignored".to_string()),
        ];

        let peer = MdnsDiscovery::parse_peer(&props).unwrap();
        assert_eq!(peer.node_id, "peer-1");
    }

    #[tokio::test]
    async fn test_notify_discovered_filters_self() {
        let config = ReplicationConfig::new("self-node")
            .cluster_name("test-cluster");

        let (discovery, mut rx) = MdnsDiscovery::new(config);

        // Create a peer with same node_id as self
        let self_peer = DiscoveredPeer::new(
            "self-node",
            "test-cluster",
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );

        discovery.notify_discovered(self_peer).await;

        // Should not receive anything (filtered out)
        tokio::select! {
            _ = rx.recv() => panic!("should not receive self"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    #[tokio::test]
    async fn test_notify_discovered_filters_different_cluster() {
        let config = ReplicationConfig::new("node-1")
            .cluster_name("cluster-a");

        let (discovery, mut rx) = MdnsDiscovery::new(config);

        // Create a peer from different cluster
        let other_cluster_peer = DiscoveredPeer::new(
            "node-2",
            "cluster-b",  // Different cluster
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );

        discovery.notify_discovered(other_cluster_peer).await;

        // Should not receive anything (different cluster)
        tokio::select! {
            _ = rx.recv() => panic!("should not receive peer from different cluster"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    #[tokio::test]
    async fn test_notify_discovered_accepts_valid_peer() {
        let config = ReplicationConfig::new("node-1")
            .cluster_name("test-cluster");

        let (discovery, mut rx) = MdnsDiscovery::new(config);

        let valid_peer = DiscoveredPeer::new(
            "node-2",  // Different node
            "test-cluster",  // Same cluster
            "127.0.0.1:4001".parse().unwrap(),
            "127.0.0.1:4002".parse().unwrap(),
        );

        discovery.notify_discovered(valid_peer.clone()).await;

        let received = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            rx.recv(),
        )
        .await
        .expect("timeout")
        .expect("channel closed");

        assert_eq!(received.node_id, "node-2");
    }

    #[tokio::test]
    async fn test_mdns_handle_new() {
        let (handle, mut rx) = MdnsHandle::new();

        // Initially not shutdown
        assert!(!*rx.borrow());

        // After shutdown
        handle.shutdown();
        let _ = rx.changed().await;
        assert!(*rx.borrow());
    }

    #[test]
    fn test_mdns_handle_default() {
        let handle = MdnsHandle::default();
        // Just verify it doesn't panic
        handle.shutdown();
    }

    #[test]
    fn test_mdns_handle_clone() {
        let (handle1, _rx) = MdnsHandle::new();
        let handle2 = handle1.clone();

        // Both should work
        handle1.shutdown();
        handle2.shutdown();
    }

    #[test]
    fn test_discovered_sender() {
        let config = test_config();
        let (discovery, _rx) = MdnsDiscovery::new(config);

        let sender = discovery.discovered_sender();
        // Verify we can clone and use the sender
        let _sender2 = sender.clone();
    }

    #[test]
    fn test_mdns_disabled() {
        let config = ReplicationConfig::new("node-1")
            .mdns_enabled(false);

        let (discovery, _rx) = MdnsDiscovery::new(config);
        assert!(!discovery.is_enabled());
    }

    #[test]
    fn test_custom_service_type() {
        let config = ReplicationConfig::new("node-1")
            .mdns_service_type("_custom._tcp.local.");

        let (discovery, _rx) = MdnsDiscovery::new(config);
        assert_eq!(discovery.service_type(), "_custom._tcp.local.");
    }

    // ==================== Additional Coverage Tests ====================

    #[test]
    fn test_gossip_addr() {
        let config = ReplicationConfig::new("node-1")
            .gossip_addr("192.168.1.100:4001".parse().unwrap());

        let (discovery, _rx) = MdnsDiscovery::new(config);
        assert_eq!(discovery.gossip_addr().to_string(), "192.168.1.100:4001");
    }

    #[test]
    fn test_transport_addr() {
        let config = ReplicationConfig::new("node-1")
            .transport_addr("192.168.1.100:4002".parse().unwrap());

        let (discovery, _rx) = MdnsDiscovery::new(config);
        assert_eq!(discovery.transport_addr().to_string(), "192.168.1.100:4002");
    }

    #[test]
    fn test_gossip_and_transport_addrs_default() {
        let config = ReplicationConfig::new("node-1");
        let (discovery, _rx) = MdnsDiscovery::new(config);

        assert_eq!(discovery.gossip_addr().port(), 4001);
        assert_eq!(discovery.transport_addr().port(), 4002);
    }

    #[tokio::test]
    async fn test_notify_discovered_logs_valid_peer() {
        // This test exercises the info! logging path when a valid peer is discovered
        let config = ReplicationConfig::new("node-1")
            .cluster_name("test-cluster");

        let (discovery, mut rx) = MdnsDiscovery::new(config);

        let valid_peer = DiscoveredPeer::new(
            "node-2",
            "test-cluster",
            "10.0.0.2:4001".parse().unwrap(),
            "10.0.0.2:4002".parse().unwrap(),
        );

        // This will trigger the info! log and send the peer
        discovery.notify_discovered(valid_peer).await;

        // Verify peer was received
        let received = rx.recv().await.expect("should receive peer");
        assert_eq!(received.node_id, "node-2");
    }

    #[tokio::test]
    async fn test_notify_discovered_different_cluster_logs_debug() {
        // This test exercises the debug! logging path for different cluster
        let config = ReplicationConfig::new("node-1")
            .cluster_name("cluster-a");

        let (discovery, mut rx) = MdnsDiscovery::new(config);

        let other_peer = DiscoveredPeer::new(
            "node-2",
            "cluster-b",  // Different cluster - triggers debug! log
            "10.0.0.2:4001".parse().unwrap(),
            "10.0.0.2:4002".parse().unwrap(),
        );

        discovery.notify_discovered(other_peer).await;

        // Should not receive - different cluster
        tokio::select! {
            _ = rx.recv() => panic!("should not receive peer from different cluster"),
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => {}
        }
    }

    #[tokio::test]
    async fn test_notify_discovered_channel_closed() {
        // This test exercises the warn! logging path when channel is closed
        let config = ReplicationConfig::new("node-1")
            .cluster_name("test-cluster");

        let (discovery, rx) = MdnsDiscovery::new(config);

        // Drop the receiver to close the channel
        drop(rx);

        let valid_peer = DiscoveredPeer::new(
            "node-2",
            "test-cluster",
            "10.0.0.2:4001".parse().unwrap(),
            "10.0.0.2:4002".parse().unwrap(),
        );

        // This will trigger the warn! log because channel is closed
        // Should not panic
        discovery.notify_discovered(valid_peer).await;
    }
}
