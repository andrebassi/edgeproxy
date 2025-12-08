//! QUIC Transport
//!
//! Provides secure peer-to-peer communication using QUIC (via Quinn).
//! Used for reliable delivery of changesets between nodes.
//!
//! Uses Sans-IO pattern: message encoding/decoding is separated from I/O for testability.

use crate::replication::types::{ChangeSet, Message, NodeId};
use crate::replication::config::ReplicationConfig;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use quinn::{Endpoint, ServerConfig, ClientConfig, Connection as QuinnConnection};

// ==================== Sans-IO Functions ====================

/// Encode a message for transport (Sans-IO pattern).
/// Returns length-prefixed binary data ready for sending.
pub fn encode_message(msg: &Message) -> anyhow::Result<Vec<u8>> {
    let data = bincode::serialize(msg)?;
    let len = data.len() as u32;

    let mut result = Vec::with_capacity(4 + data.len());
    result.extend_from_slice(&len.to_be_bytes());
    result.extend_from_slice(&data);

    Ok(result)
}

/// Decode a length from the first 4 bytes.
pub fn decode_length(buf: &[u8; 4]) -> u32 {
    u32::from_be_bytes(*buf)
}

/// Decode a message from binary data (Sans-IO pattern).
pub fn decode_message(data: &[u8]) -> anyhow::Result<Message> {
    let msg: Message = bincode::deserialize(data)?;
    Ok(msg)
}

/// Validate a changeset broadcast message (Sans-IO pattern).
pub fn validate_broadcast(changeset: &ChangeSet) -> bool {
    changeset.verify()
}

/// Create a broadcast message from a changeset (Sans-IO pattern).
pub fn create_broadcast(changeset: ChangeSet) -> Message {
    Message::Broadcast(changeset)
}

/// Create a sync request message (Sans-IO pattern).
pub fn create_sync_request(from_seq: u64, table: Option<String>) -> Message {
    Message::SyncRequest { from_seq, table }
}

/// Create a sync response message (Sans-IO pattern).
pub fn create_sync_response(changesets: Vec<ChangeSet>) -> Message {
    Message::SyncResponse(changesets)
}

/// Extract changesets from a broadcast message (Sans-IO pattern).
pub fn extract_broadcast(msg: &Message) -> Option<&ChangeSet> {
    match msg {
        Message::Broadcast(cs) => Some(cs),
        _ => None,
    }
}

/// Extract sync request parameters (Sans-IO pattern).
pub fn extract_sync_request(msg: &Message) -> Option<(u64, Option<&String>)> {
    match msg {
        Message::SyncRequest { from_seq, table } => Some((*from_seq, table.as_ref())),
        _ => None,
    }
}

/// Extract sync response changesets (Sans-IO pattern).
pub fn extract_sync_response(msg: &Message) -> Option<&Vec<ChangeSet>> {
    match msg {
        Message::SyncResponse(changesets) => Some(changesets),
        _ => None,
    }
}

/// Check if a message is a broadcast (Sans-IO pattern).
pub fn is_broadcast(msg: &Message) -> bool {
    matches!(msg, Message::Broadcast(_))
}

/// Check if a message is a sync request (Sans-IO pattern).
pub fn is_sync_request(msg: &Message) -> bool {
    matches!(msg, Message::SyncRequest { .. })
}

/// Check if a message is a sync response (Sans-IO pattern).
pub fn is_sync_response(msg: &Message) -> bool {
    matches!(msg, Message::SyncResponse(_))
}

/// Get the message type as a string (Sans-IO pattern).
pub fn message_type_name(msg: &Message) -> &'static str {
    match msg {
        Message::Broadcast(_) => "Broadcast",
        Message::SyncRequest { .. } => "SyncRequest",
        Message::SyncResponse(_) => "SyncResponse",
        Message::Ack { .. } => "Ack",
        Message::Ping => "Ping",
        Message::Pong => "Pong",
    }
}

/// Validate a sync request (Sans-IO pattern).
pub fn validate_sync_request(_from_seq: u64) -> bool {
    // from_seq must be valid (we accept 0 as "sync everything")
    true // Any u64 is valid
}

/// Count changesets in a sync response (Sans-IO pattern).
pub fn count_sync_response_changesets(msg: &Message) -> usize {
    match msg {
        Message::SyncResponse(changesets) => changesets.len(),
        _ => 0,
    }
}

/// Count changes in a broadcast (Sans-IO pattern).
pub fn count_broadcast_changes(msg: &Message) -> usize {
    match msg {
        Message::Broadcast(cs) => cs.changes.len(),
        _ => 0,
    }
}

/// Get the sequence number from a broadcast (Sans-IO pattern).
pub fn get_broadcast_seq(msg: &Message) -> Option<u64> {
    match msg {
        Message::Broadcast(cs) => Some(cs.seq),
        _ => None,
    }
}

/// Get from_seq from a sync request (Sans-IO pattern).
pub fn get_sync_request_from_seq(msg: &Message) -> Option<u64> {
    match msg {
        Message::SyncRequest { from_seq, .. } => Some(*from_seq),
        _ => None,
    }
}

/// A connection to a peer node.
pub struct PeerConnection {
    pub node_id: NodeId,
    pub addr: SocketAddr,
    connection: QuinnConnection,
}

impl PeerConnection {
    /// Send a message to this peer.
    pub async fn send(&self, msg: &Message) -> anyhow::Result<()> {
        let mut send = self.connection.open_uni().await?;
        let data = bincode::serialize(msg)?;

        // Write length prefix + data
        let len = data.len() as u32;
        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&data).await?;
        send.finish()?;

        Ok(())
    }

    /// Send and wait for a response.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn request(&self, msg: &Message) -> anyhow::Result<Message> {
        let (mut send, mut recv) = self.connection.open_bi().await?;

        // Send request
        let data = bincode::serialize(msg)?;
        let len = data.len() as u32;
        send.write_all(&len.to_be_bytes()).await?;
        send.write_all(&data).await?;
        send.finish()?;

        // Read response
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;

        let mut data = vec![0u8; len];
        recv.read_exact(&mut data).await?;

        let response: Message = bincode::deserialize(&data)?;
        Ok(response)
    }

    /// Check if the connection is still alive.
    pub fn is_alive(&self) -> bool {
        self.connection.close_reason().is_none()
    }
}

/// Events from the transport layer.
#[derive(Debug)]
pub enum TransportEvent {
    /// Received a message from a peer
    MessageReceived {
        from: NodeId,
        message: Message,
    },
    /// A peer connected
    PeerConnected(NodeId),
    /// A peer disconnected
    PeerDisconnected(NodeId),
}

/// Transport service for peer communication.
pub struct TransportService {
    config: ReplicationConfig,
    endpoint: Option<Endpoint>,
    peers: Arc<RwLock<HashMap<String, Arc<PeerConnection>>>>,
    event_tx: mpsc::Sender<TransportEvent>,
    event_rx: Option<mpsc::Receiver<TransportEvent>>,
    shutdown: Arc<std::sync::atomic::AtomicBool>,
}

impl TransportService {
    /// Create a new transport service.
    pub fn new(config: ReplicationConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);

        Self {
            config,
            endpoint: None,
            peers: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Some(event_rx),
            shutdown: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        }
    }

    /// Get the event receiver.
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<TransportEvent>> {
        self.event_rx.take()
    }

    /// Get all connected peers.
    pub async fn peers(&self) -> Vec<Arc<PeerConnection>> {
        self.peers.read().await.values().cloned().collect()
    }

    /// Get a specific peer connection.
    pub async fn get_peer(&self, node_id: &str) -> Option<Arc<PeerConnection>> {
        self.peers.read().await.get(node_id).cloned()
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        self.shutdown.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Check if shutdown was signaled.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Start the transport service.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn start(&mut self) -> anyhow::Result<()> {
        // Generate self-signed certificate for QUIC
        let cert = rcgen::generate_simple_self_signed(vec![
            self.config.node_id.clone(),
            "localhost".to_string(),
        ])?;

        let cert_der = cert.cert.der().to_vec();
        let key_der = cert.key_pair.serialize_der();

        let cert_chain = vec![rustls::pki_types::CertificateDer::from(cert_der.clone())];
        let private_key = rustls::pki_types::PrivateKeyDer::try_from(key_der)
            .map_err(|e| anyhow::anyhow!("failed to parse private key: {:?}", e))?;

        // Server config using quinn's rustls
        let server_crypto = quinn::rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(cert_chain.clone(), private_key.clone_key())?;

        let server_config = ServerConfig::with_crypto(Arc::new(
            quinn::crypto::rustls::QuicServerConfig::try_from(server_crypto)?
        ));

        // Client config - use SkipServerVerification for self-signed certs in cluster
        let client_crypto = quinn::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let client_config = ClientConfig::new(Arc::new(
            quinn::crypto::rustls::QuicClientConfig::try_from(client_crypto)?
        ));

        // Create endpoint
        let mut endpoint = Endpoint::server(server_config, self.config.transport_addr)?;
        endpoint.set_default_client_config(client_config);
        let endpoint_clone = endpoint.clone();

        tracing::info!("transport listening on {}", self.config.transport_addr);

        self.endpoint = Some(endpoint);

        // Spawn accept loop
        let peers = self.peers.clone();
        let event_tx = self.event_tx.clone();
        let shutdown = self.shutdown.clone();
        let node_id = self.config.node_id.clone();

        tokio::spawn(async move {
            loop {
                if shutdown.load(std::sync::atomic::Ordering::SeqCst) {
                    break;
                }

                match endpoint_clone.accept().await {
                    Some(incoming) => {
                        let peers = peers.clone();
                        let event_tx = event_tx.clone();
                        let local_node_id = node_id.clone();

                        tokio::spawn(async move {
                            match incoming.await {
                                Ok(conn) => {
                                    let remote_addr = conn.remote_address();
                                    let peer_node_id = NodeId::new(format!("peer-{}", remote_addr));

                                    let peer = Arc::new(PeerConnection {
                                        node_id: peer_node_id.clone(),
                                        addr: remote_addr,
                                        connection: conn.clone(),
                                    });

                                    peers.write().await.insert(peer_node_id.0.clone(), peer);

                                    let _ = event_tx
                                        .send(TransportEvent::PeerConnected(peer_node_id.clone()))
                                        .await;

                                    // Handle incoming streams
                                    Self::handle_connection(
                                        conn,
                                        peer_node_id,
                                        local_node_id,
                                        event_tx,
                                    ).await;
                                }
                                Err(e) => {
                                    tracing::warn!("failed to accept connection: {:?}", e);
                                }
                            }
                        });
                    }
                    None => break,
                }
            }
        });

        Ok(())
    }

    /// Connect to a peer.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn connect(&self, addr: SocketAddr, node_id: &str) -> anyhow::Result<Arc<PeerConnection>> {
        let endpoint = self.endpoint.as_ref().ok_or_else(|| {
            anyhow::anyhow!("transport not started")
        })?;

        let conn = endpoint.connect(addr, &self.config.node_id)?
            .await?;

        let peer_node_id = NodeId::new(node_id);
        let peer = Arc::new(PeerConnection {
            node_id: peer_node_id.clone(),
            addr,
            connection: conn,
        });

        self.peers.write().await.insert(node_id.to_string(), peer.clone());

        let _ = self.event_tx
            .send(TransportEvent::PeerConnected(peer_node_id))
            .await;

        Ok(peer)
    }

    /// Broadcast a message to all peers.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn broadcast(&self, msg: &Message) -> usize {
        let peers: Vec<_> = self.peers.read().await.values().cloned().collect();
        let mut sent = 0;

        for peer in peers {
            if peer.is_alive() {
                if peer.send(msg).await.is_ok() {
                    sent += 1;
                }
            }
        }

        sent
    }

    /// Broadcast a changeset to all peers.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn broadcast_changeset(&self, changeset: &ChangeSet) -> usize {
        self.broadcast(&Message::Broadcast(changeset.clone())).await
    }

    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn handle_connection(
        conn: QuinnConnection,
        peer_node_id: NodeId,
        _local_node_id: String,
        event_tx: mpsc::Sender<TransportEvent>,
    ) {
        loop {
            // Accept uni streams
            match conn.accept_uni().await {
                Ok(mut recv) => {
                    let event_tx = event_tx.clone();
                    let peer_id = peer_node_id.clone();

                    tokio::spawn(async move {
                        // Read length prefix
                        let mut len_buf = [0u8; 4];
                        if recv.read_exact(&mut len_buf).await.is_err() {
                            return;
                        }
                        let len = u32::from_be_bytes(len_buf) as usize;

                        if len > 10 * 1024 * 1024 {
                            // Max 10MB message
                            tracing::warn!("message too large: {} bytes", len);
                            return;
                        }

                        // Read message
                        let mut data = vec![0u8; len];
                        if recv.read_exact(&mut data).await.is_err() {
                            return;
                        }

                        // Deserialize
                        match bincode::deserialize::<Message>(&data) {
                            Ok(msg) => {
                                let _ = event_tx
                                    .send(TransportEvent::MessageReceived {
                                        from: peer_id,
                                        message: msg,
                                    })
                                    .await;
                            }
                            Err(e) => {
                                tracing::debug!("failed to deserialize message: {:?}", e);
                            }
                        }
                    });
                }
                Err(quinn::ConnectionError::ApplicationClosed(_)) => {
                    break;
                }
                Err(e) => {
                    tracing::debug!("connection error: {:?}", e);
                    break;
                }
            }
        }

        let _ = event_tx
            .send(TransportEvent::PeerDisconnected(peer_node_id))
            .await;
    }
}

/// Skip server certificate verification for self-signed certs in cluster.
#[derive(Debug)]
struct SkipServerVerification;

#[cfg_attr(coverage_nightly, coverage(off))]
impl quinn::rustls::client::danger::ServerCertVerifier for SkipServerVerification {
    fn verify_server_cert(
        &self,
        _end_entity: &rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[rustls::pki_types::CertificateDer<'_>],
        _server_name: &rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: rustls::pki_types::UnixTime,
    ) -> Result<quinn::rustls::client::danger::ServerCertVerified, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &rustls::pki_types::CertificateDer<'_>,
        _dss: &quinn::rustls::DigitallySignedStruct,
    ) -> Result<quinn::rustls::client::danger::HandshakeSignatureValid, quinn::rustls::Error> {
        Ok(quinn::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<quinn::rustls::SignatureScheme> {
        vec![
            quinn::rustls::SignatureScheme::RSA_PKCS1_SHA256,
            quinn::rustls::SignatureScheme::RSA_PKCS1_SHA384,
            quinn::rustls::SignatureScheme::RSA_PKCS1_SHA512,
            quinn::rustls::SignatureScheme::ECDSA_NISTP256_SHA256,
            quinn::rustls::SignatureScheme::ECDSA_NISTP384_SHA384,
            quinn::rustls::SignatureScheme::ECDSA_NISTP521_SHA512,
            quinn::rustls::SignatureScheme::RSA_PSS_SHA256,
            quinn::rustls::SignatureScheme::RSA_PSS_SHA384,
            quinn::rustls::SignatureScheme::RSA_PSS_SHA512,
            quinn::rustls::SignatureScheme::ED25519,
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replication::types::{Change, ChangeKind};
    use quinn::rustls::client::danger::ServerCertVerifier;

    #[test]
    fn test_transport_service_creation() {
        let config = ReplicationConfig::new("test-node")
            .transport_addr("127.0.0.1:0".parse().unwrap());

        let service = TransportService::new(config);
        assert!(!service.is_shutdown());
    }

    #[test]
    fn test_transport_shutdown() {
        let config = ReplicationConfig::new("test-node");
        let service = TransportService::new(config);

        assert!(!service.is_shutdown());
        service.shutdown();
        assert!(service.is_shutdown());
    }

    #[test]
    fn test_transport_service_take_event_rx() {
        let config = ReplicationConfig::new("test-node");
        let mut service = TransportService::new(config);

        // First call should return Some
        let rx = service.take_event_rx();
        assert!(rx.is_some());

        // Second call should return None
        let rx = service.take_event_rx();
        assert!(rx.is_none());
    }

    #[tokio::test]
    async fn test_transport_service_peers_empty() {
        let config = ReplicationConfig::new("test-node");
        let service = TransportService::new(config);

        let peers = service.peers().await;
        assert!(peers.is_empty());
    }

    #[tokio::test]
    async fn test_transport_service_get_peer_none() {
        let config = ReplicationConfig::new("test-node");
        let service = TransportService::new(config);

        let peer = service.get_peer("nonexistent").await;
        assert!(peer.is_none());
    }

    #[tokio::test]
    async fn test_message_broadcast_serialization() {
        let node_id = NodeId::new("node-1");
        let changeset = ChangeSet::new(node_id, 1, vec![]);
        let msg = Message::Broadcast(changeset);

        let data = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&data).unwrap();

        match decoded {
            Message::Broadcast(cs) => {
                assert_eq!(cs.seq, 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[tokio::test]
    async fn test_message_broadcast_with_changes() {
        let node_id = NodeId::new("node-1");
        let changes = vec![
            Change::new("backends", "b1", ChangeKind::Insert, "{}", &node_id),
            Change::new("backends", "b2", ChangeKind::Update, "{}", &node_id),
        ];
        let changeset = ChangeSet::new(node_id, 5, changes);
        let msg = Message::Broadcast(changeset);

        let data = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&data).unwrap();

        match decoded {
            Message::Broadcast(cs) => {
                assert_eq!(cs.seq, 5);
                assert_eq!(cs.changes.len(), 2);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[tokio::test]
    async fn test_message_sync_request_serialization() {
        let msg = Message::SyncRequest {
            from_seq: 10,
            table: Some("backends".to_string()),
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&data).unwrap();

        match decoded {
            Message::SyncRequest { from_seq, table } => {
                assert_eq!(from_seq, 10);
                assert_eq!(table, Some("backends".to_string()));
            }
            _ => panic!("wrong message type"),
        }
    }

    #[tokio::test]
    async fn test_message_sync_response_serialization() {
        let node_id = NodeId::new("responder");
        let changesets = vec![
            ChangeSet::new(node_id.clone(), 1, vec![]),
            ChangeSet::new(node_id, 2, vec![]),
        ];

        let msg = Message::SyncResponse(changesets);

        let data = bincode::serialize(&msg).unwrap();
        let decoded: Message = bincode::deserialize(&data).unwrap();

        match decoded {
            Message::SyncResponse(changesets) => {
                assert_eq!(changesets.len(), 2);
                assert_eq!(changesets[0].seq, 1);
                assert_eq!(changesets[1].seq, 2);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_transport_event_message_received_debug() {
        let event = TransportEvent::MessageReceived {
            from: NodeId::new("peer-1"),
            message: Message::Broadcast(ChangeSet::new(NodeId::new("peer-1"), 1, vec![])),
        };

        let debug = format!("{:?}", event);
        assert!(debug.contains("MessageReceived"));
    }

    #[test]
    fn test_transport_event_peer_connected_debug() {
        let event = TransportEvent::PeerConnected(NodeId::new("peer-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerConnected"));
        assert!(debug.contains("peer-1"));
    }

    #[test]
    fn test_transport_event_peer_disconnected_debug() {
        let event = TransportEvent::PeerDisconnected(NodeId::new("peer-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerDisconnected"));
        assert!(debug.contains("peer-1"));
    }

    #[tokio::test]
    async fn test_transport_service_start() {
        let config = ReplicationConfig::new("test-node")
            .transport_addr("127.0.0.1:0".parse().unwrap());

        let mut service = TransportService::new(config);
        let result = service.start().await;
        assert!(result.is_ok());

        // Endpoint should be set
        assert!(service.endpoint.is_some());

        service.shutdown();
    }

    #[tokio::test]
    async fn test_transport_service_broadcast_no_peers() {
        let config = ReplicationConfig::new("test-node")
            .transport_addr("127.0.0.1:0".parse().unwrap());

        let mut service = TransportService::new(config);
        service.start().await.unwrap();

        let node_id = NodeId::new("test-node");
        let changeset = ChangeSet::new(node_id, 1, vec![]);

        // Should return 0 when no peers
        let sent = service.broadcast_changeset(&changeset).await;
        assert_eq!(sent, 0);

        service.shutdown();
    }

    #[tokio::test]
    async fn test_transport_service_connect_without_start() {
        let config = ReplicationConfig::new("test-node");
        let service = TransportService::new(config);

        // Should fail since transport not started
        let result = service.connect("127.0.0.1:9999".parse().unwrap(), "peer").await;
        assert!(result.is_err());
    }

    #[test]
    fn test_skip_server_verification_supported_schemes() {
        let verifier = SkipServerVerification;
        let schemes = verifier.supported_verify_schemes();

        assert!(!schemes.is_empty());
        assert!(schemes.contains(&quinn::rustls::SignatureScheme::ECDSA_NISTP256_SHA256));
        assert!(schemes.contains(&quinn::rustls::SignatureScheme::RSA_PKCS1_SHA256));
        assert!(schemes.contains(&quinn::rustls::SignatureScheme::ED25519));
    }

    #[test]
    fn test_skip_server_verification_debug() {
        let verifier = SkipServerVerification;
        let debug = format!("{:?}", verifier);
        assert!(debug.contains("SkipServerVerification"));
    }

    #[tokio::test]
    async fn test_transport_two_services_communicate() {
        // Start first service
        let config1 = ReplicationConfig::new("node-1")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service1 = TransportService::new(config1);
        service1.start().await.unwrap();

        // Get the actual bound address
        let addr1 = service1.endpoint.as_ref().unwrap().local_addr().unwrap();

        // Start second service
        let config2 = ReplicationConfig::new("node-2")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service2 = TransportService::new(config2);
        service2.start().await.unwrap();

        // Connect service2 to service1
        let result = service2.connect(addr1, "node-1").await;
        assert!(result.is_ok());

        // Should have the peer now
        let peer = service2.get_peer("node-1").await;
        assert!(peer.is_some());
        assert!(peer.unwrap().is_alive());

        // Peers count
        let peers = service2.peers().await;
        assert_eq!(peers.len(), 1);

        service1.shutdown();
        service2.shutdown();
    }

    #[tokio::test]
    async fn test_peer_connection_is_alive() {
        // Start first service
        let config1 = ReplicationConfig::new("node-1")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service1 = TransportService::new(config1);
        service1.start().await.unwrap();
        let addr1 = service1.endpoint.as_ref().unwrap().local_addr().unwrap();

        // Start second service and connect
        let config2 = ReplicationConfig::new("node-2")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service2 = TransportService::new(config2);
        service2.start().await.unwrap();

        let peer = service2.connect(addr1, "node-1").await.unwrap();
        assert!(peer.is_alive());

        service1.shutdown();
        service2.shutdown();
    }

    #[tokio::test]
    async fn test_peer_connection_send() {
        // Start receiver service
        let config1 = ReplicationConfig::new("receiver")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service1 = TransportService::new(config1);
        service1.start().await.unwrap();
        let addr1 = service1.endpoint.as_ref().unwrap().local_addr().unwrap();

        // Start sender service and connect
        let config2 = ReplicationConfig::new("sender")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service2 = TransportService::new(config2);
        service2.start().await.unwrap();

        let peer = service2.connect(addr1, "receiver").await.unwrap();

        // Send a message
        let msg = Message::Broadcast(ChangeSet::new(NodeId::new("sender"), 1, vec![]));
        let result = peer.send(&msg).await;
        assert!(result.is_ok());

        service1.shutdown();
        service2.shutdown();
    }

    #[tokio::test]
    async fn test_broadcast_to_connected_peers() {
        // Start receiver
        let config1 = ReplicationConfig::new("receiver")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service1 = TransportService::new(config1);
        service1.start().await.unwrap();
        let addr1 = service1.endpoint.as_ref().unwrap().local_addr().unwrap();

        // Start broadcaster and connect
        let config2 = ReplicationConfig::new("broadcaster")
            .transport_addr("127.0.0.1:0".parse().unwrap());
        let mut service2 = TransportService::new(config2);
        service2.start().await.unwrap();

        service2.connect(addr1, "receiver").await.unwrap();

        // Broadcast
        let changeset = ChangeSet::new(NodeId::new("broadcaster"), 1, vec![]);
        let sent = service2.broadcast_changeset(&changeset).await;
        assert_eq!(sent, 1);

        service1.shutdown();
        service2.shutdown();
    }

    // ==================== Sans-IO Tests ====================

    #[test]
    fn test_encode_message_broadcast() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let msg = Message::Broadcast(cs);

        let encoded = encode_message(&msg).unwrap();

        // First 4 bytes are length
        assert!(encoded.len() > 4);
        let len = decode_length(&[encoded[0], encoded[1], encoded[2], encoded[3]]);
        assert_eq!(len as usize, encoded.len() - 4);
    }

    #[test]
    fn test_encode_decode_roundtrip() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 42, vec![
            Change::new("backends", "b1", ChangeKind::Insert, "{}", &NodeId::new("node-1")),
        ]);
        let original = Message::Broadcast(cs);

        let encoded = encode_message(&original).unwrap();
        let decoded = decode_message(&encoded[4..]).unwrap();

        match decoded {
            Message::Broadcast(cs) => {
                assert_eq!(cs.seq, 42);
                assert_eq!(cs.changes.len(), 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_decode_length() {
        let len_bytes: [u8; 4] = [0x00, 0x00, 0x01, 0x00]; // 256 in big-endian
        assert_eq!(decode_length(&len_bytes), 256);

        let len_bytes: [u8; 4] = [0x00, 0x00, 0x00, 0x10]; // 16 in big-endian
        assert_eq!(decode_length(&len_bytes), 16);

        let len_bytes: [u8; 4] = [0x00, 0x01, 0x00, 0x00]; // 65536 in big-endian
        assert_eq!(decode_length(&len_bytes), 65536);
    }

    #[test]
    fn test_decode_message_invalid() {
        let invalid_data = [0, 1, 2, 3];
        let result = decode_message(&invalid_data);
        assert!(result.is_err());
    }

    #[test]
    fn test_validate_broadcast_valid() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        assert!(validate_broadcast(&cs));
    }

    #[test]
    fn test_validate_broadcast_invalid() {
        let mut cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        cs.checksum = 12345; // Invalid checksum
        assert!(!validate_broadcast(&cs));
    }

    #[test]
    fn test_create_broadcast() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 5, vec![]);
        let msg = create_broadcast(cs);

        match msg {
            Message::Broadcast(cs) => assert_eq!(cs.seq, 5),
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_create_sync_request() {
        let msg = create_sync_request(100, Some("backends".to_string()));

        match msg {
            Message::SyncRequest { from_seq, table } => {
                assert_eq!(from_seq, 100);
                assert_eq!(table, Some("backends".to_string()));
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_create_sync_request_no_table() {
        let msg = create_sync_request(50, None);

        match msg {
            Message::SyncRequest { from_seq, table } => {
                assert_eq!(from_seq, 50);
                assert!(table.is_none());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_create_sync_response() {
        let changesets = vec![
            ChangeSet::new(NodeId::new("node-1"), 1, vec![]),
            ChangeSet::new(NodeId::new("node-1"), 2, vec![]),
        ];
        let msg = create_sync_response(changesets);

        match msg {
            Message::SyncResponse(cs) => {
                assert_eq!(cs.len(), 2);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_create_sync_response_empty() {
        let msg = create_sync_response(vec![]);

        match msg {
            Message::SyncResponse(cs) => {
                assert!(cs.is_empty());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_extract_broadcast() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 10, vec![]);
        let msg = Message::Broadcast(cs);

        let extracted = extract_broadcast(&msg);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().seq, 10);
    }

    #[test]
    fn test_extract_broadcast_wrong_type() {
        let msg = Message::SyncRequest { from_seq: 1, table: None };
        let extracted = extract_broadcast(&msg);
        assert!(extracted.is_none());
    }

    #[test]
    fn test_extract_sync_request() {
        let msg = Message::SyncRequest {
            from_seq: 42,
            table: Some("backends".to_string()),
        };

        let extracted = extract_sync_request(&msg);
        assert!(extracted.is_some());

        let (seq, table) = extracted.unwrap();
        assert_eq!(seq, 42);
        assert_eq!(table, Some(&"backends".to_string()));
    }

    #[test]
    fn test_extract_sync_request_no_table() {
        let msg = Message::SyncRequest {
            from_seq: 10,
            table: None,
        };

        let extracted = extract_sync_request(&msg);
        assert!(extracted.is_some());

        let (seq, table) = extracted.unwrap();
        assert_eq!(seq, 10);
        assert!(table.is_none());
    }

    #[test]
    fn test_extract_sync_request_wrong_type() {
        let msg = Message::Broadcast(ChangeSet::new(NodeId::new("node-1"), 1, vec![]));
        let extracted = extract_sync_request(&msg);
        assert!(extracted.is_none());
    }

    #[test]
    fn test_extract_sync_response() {
        let changesets = vec![
            ChangeSet::new(NodeId::new("node-1"), 1, vec![]),
            ChangeSet::new(NodeId::new("node-1"), 2, vec![]),
            ChangeSet::new(NodeId::new("node-1"), 3, vec![]),
        ];
        let msg = Message::SyncResponse(changesets);

        let extracted = extract_sync_response(&msg);
        assert!(extracted.is_some());
        assert_eq!(extracted.unwrap().len(), 3);
    }

    #[test]
    fn test_extract_sync_response_wrong_type() {
        let msg = Message::SyncRequest { from_seq: 1, table: None };
        let extracted = extract_sync_response(&msg);
        assert!(extracted.is_none());
    }

    #[test]
    fn test_encode_message_sync_request() {
        let msg = create_sync_request(100, Some("backends".to_string()));
        let encoded = encode_message(&msg).unwrap();

        let len = decode_length(&[encoded[0], encoded[1], encoded[2], encoded[3]]);
        let decoded = decode_message(&encoded[4..]).unwrap();

        assert!(len > 0);
        match decoded {
            Message::SyncRequest { from_seq, table } => {
                assert_eq!(from_seq, 100);
                assert_eq!(table, Some("backends".to_string()));
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_encode_message_sync_response() {
        let changesets = vec![
            ChangeSet::new(NodeId::new("node-1"), 1, vec![]),
        ];
        let msg = create_sync_response(changesets);
        let encoded = encode_message(&msg).unwrap();

        let decoded = decode_message(&encoded[4..]).unwrap();

        match decoded {
            Message::SyncResponse(cs) => {
                assert_eq!(cs.len(), 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_encode_large_changeset() {
        let node_id = NodeId::new("node-1");
        let changes: Vec<Change> = (0..100)
            .map(|i| Change::new("backends", &format!("b{}", i), ChangeKind::Insert, "{}", &node_id))
            .collect();

        let cs = ChangeSet::new(node_id, 999, changes);
        let msg = create_broadcast(cs);

        let encoded = encode_message(&msg).unwrap();
        let decoded = decode_message(&encoded[4..]).unwrap();

        match decoded {
            Message::Broadcast(cs) => {
                assert_eq!(cs.seq, 999);
                assert_eq!(cs.changes.len(), 100);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_is_broadcast() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let broadcast = Message::Broadcast(cs);
        let sync_req = Message::SyncRequest { from_seq: 0, table: None };
        let sync_resp = Message::SyncResponse(vec![]);

        assert!(is_broadcast(&broadcast));
        assert!(!is_broadcast(&sync_req));
        assert!(!is_broadcast(&sync_resp));
    }

    #[test]
    fn test_is_sync_request() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let broadcast = Message::Broadcast(cs);
        let sync_req = Message::SyncRequest { from_seq: 0, table: None };
        let sync_resp = Message::SyncResponse(vec![]);

        assert!(!is_sync_request(&broadcast));
        assert!(is_sync_request(&sync_req));
        assert!(!is_sync_request(&sync_resp));
    }

    #[test]
    fn test_is_sync_response() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let broadcast = Message::Broadcast(cs);
        let sync_req = Message::SyncRequest { from_seq: 0, table: None };
        let sync_resp = Message::SyncResponse(vec![]);

        assert!(!is_sync_response(&broadcast));
        assert!(!is_sync_response(&sync_req));
        assert!(is_sync_response(&sync_resp));
    }

    #[test]
    fn test_message_type_name() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let broadcast = Message::Broadcast(cs);
        let sync_req = Message::SyncRequest { from_seq: 0, table: None };
        let sync_resp = Message::SyncResponse(vec![]);
        let ack = Message::Ack { source: NodeId::new("node-1"), seq: 1 };
        let ping = Message::Ping;
        let pong = Message::Pong;

        assert_eq!(message_type_name(&broadcast), "Broadcast");
        assert_eq!(message_type_name(&sync_req), "SyncRequest");
        assert_eq!(message_type_name(&sync_resp), "SyncResponse");
        assert_eq!(message_type_name(&ack), "Ack");
        assert_eq!(message_type_name(&ping), "Ping");
        assert_eq!(message_type_name(&pong), "Pong");
    }

    #[test]
    fn test_validate_sync_request() {
        assert!(validate_sync_request(0));
        assert!(validate_sync_request(1));
        assert!(validate_sync_request(u64::MAX));
    }

    #[test]
    fn test_count_sync_response_changesets() {
        let cs1 = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let cs2 = ChangeSet::new(NodeId::new("node-1"), 2, vec![]);
        let cs3 = ChangeSet::new(NodeId::new("node-1"), 3, vec![]);

        let empty_resp = Message::SyncResponse(vec![]);
        let one_resp = Message::SyncResponse(vec![cs1.clone()]);
        let three_resp = Message::SyncResponse(vec![cs1.clone(), cs2, cs3]);
        let wrong_type = Message::SyncRequest { from_seq: 0, table: None };

        assert_eq!(count_sync_response_changesets(&empty_resp), 0);
        assert_eq!(count_sync_response_changesets(&one_resp), 1);
        assert_eq!(count_sync_response_changesets(&three_resp), 3);
        assert_eq!(count_sync_response_changesets(&wrong_type), 0);
    }

    #[test]
    fn test_count_broadcast_changes() {
        let node_id = NodeId::new("node-1");
        let empty_cs = ChangeSet::new(node_id.clone(), 1, vec![]);
        let changes: Vec<Change> = (0..5)
            .map(|i| Change::new("backends", &format!("b{}", i), ChangeKind::Insert, "{}", &node_id))
            .collect();
        let cs_with_changes = ChangeSet::new(node_id, 2, changes);

        let empty_broadcast = Message::Broadcast(empty_cs);
        let broadcast_with_changes = Message::Broadcast(cs_with_changes);
        let wrong_type = Message::SyncRequest { from_seq: 0, table: None };

        assert_eq!(count_broadcast_changes(&empty_broadcast), 0);
        assert_eq!(count_broadcast_changes(&broadcast_with_changes), 5);
        assert_eq!(count_broadcast_changes(&wrong_type), 0);
    }

    #[test]
    fn test_get_broadcast_seq() {
        let cs = ChangeSet::new(NodeId::new("node-1"), 42, vec![]);
        let broadcast = Message::Broadcast(cs);
        let sync_req = Message::SyncRequest { from_seq: 100, table: None };

        assert_eq!(get_broadcast_seq(&broadcast), Some(42));
        assert_eq!(get_broadcast_seq(&sync_req), None);
    }

    #[test]
    fn test_get_sync_request_from_seq() {
        let sync_req = Message::SyncRequest { from_seq: 100, table: None };
        let cs = ChangeSet::new(NodeId::new("node-1"), 1, vec![]);
        let broadcast = Message::Broadcast(cs);

        assert_eq!(get_sync_request_from_seq(&sync_req), Some(100));
        assert_eq!(get_sync_request_from_seq(&broadcast), None);
    }

    #[test]
    fn test_transport_event_debug() {
        let event = TransportEvent::PeerConnected(NodeId::new("peer-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("PeerConnected"));
        assert!(debug.contains("peer-1"));

        let event2 = TransportEvent::PeerDisconnected(NodeId::new("peer-2"));
        let debug2 = format!("{:?}", event2);
        assert!(debug2.contains("PeerDisconnected"));
        assert!(debug2.contains("peer-2"));
    }

    #[test]
    fn test_transport_event_message_received() {
        let msg = Message::SyncRequest { from_seq: 50, table: Some("backends".to_string()) };
        let event = TransportEvent::MessageReceived {
            from: NodeId::new("sender"),
            message: msg,
        };
        let debug = format!("{:?}", event);
        assert!(debug.contains("MessageReceived"));
        assert!(debug.contains("sender"));
    }
}
