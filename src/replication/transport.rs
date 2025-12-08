//! QUIC Transport
//!
//! Provides secure peer-to-peer communication using QUIC (via Quinn).
//! Used for reliable delivery of changesets between nodes.

use crate::replication::types::{ChangeSet, Message, NodeId};
use crate::replication::config::ReplicationConfig;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use quinn::{Endpoint, ServerConfig, ClientConfig, Connection as QuinnConnection};

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
    pub async fn broadcast_changeset(&self, changeset: &ChangeSet) -> usize {
        self.broadcast(&Message::Broadcast(changeset.clone())).await
    }

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

    #[test]
    fn test_transport_service_creation() {
        let config = ReplicationConfig::new("test-node")
            .transport_addr("127.0.0.1:0".parse().unwrap());

        let service = TransportService::new(config);
        assert!(!service.is_shutdown());
    }

    #[tokio::test]
    async fn test_message_serialization() {
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

    #[test]
    fn test_transport_shutdown() {
        let config = ReplicationConfig::new("test-node");
        let service = TransportService::new(config);

        assert!(!service.is_shutdown());
        service.shutdown();
        assert!(service.is_shutdown());
    }
}
