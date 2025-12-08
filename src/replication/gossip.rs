//! Gossip Protocol (SWIM-like)
//!
//! Implements cluster membership and failure detection using a simplified
//! SWIM-inspired protocol for peer discovery and heartbeat.

use crate::replication::types::NodeId;
use crate::replication::config::ReplicationConfig;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::UdpSocket;
use tokio::sync::mpsc;

/// State of a cluster member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemberState {
    /// Member is alive and reachable
    Alive,
    /// Member is suspected of failure
    Suspect,
    /// Member has been declared dead
    Dead,
}

/// Information about a cluster member.
#[derive(Debug, Clone)]
pub struct Member {
    /// Node identifier
    pub node_id: NodeId,
    /// Address for gossip communication
    pub gossip_addr: SocketAddr,
    /// Address for QUIC transport
    pub transport_addr: SocketAddr,
    /// Current state
    pub state: MemberState,
    /// When the member was last seen
    pub last_seen: Instant,
    /// Incarnation number for state reconciliation
    pub incarnation: u64,
}

/// Message types for gossip protocol
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GossipMessage {
    /// Ping - check if node is alive
    Ping {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    /// Ack - response to ping
    Ack {
        sender_id: String,
        sender_gossip_addr: SocketAddr,
        sender_transport_addr: SocketAddr,
        incarnation: u64,
    },
    /// Join - announce joining the cluster
    Join {
        node_id: String,
        gossip_addr: SocketAddr,
        transport_addr: SocketAddr,
    },
    /// Announce member list
    MemberList {
        members: Vec<(String, SocketAddr, SocketAddr, u64)>, // (id, gossip_addr, transport_addr, incarnation)
    },
}

/// Events emitted by the gossip service.
#[derive(Debug, Clone)]
pub enum GossipEvent {
    /// A new member joined the cluster
    MemberJoined(Member),
    /// A member left or was declared dead
    MemberLeft(NodeId),
    /// A member's state changed
    MemberStateChanged {
        node_id: NodeId,
        old_state: MemberState,
        new_state: MemberState,
    },
}

/// Gossip service for cluster membership.
pub struct GossipService {
    config: ReplicationConfig,
    members: Arc<RwLock<HashMap<String, Member>>>,
    event_tx: mpsc::Sender<GossipEvent>,
    event_rx: Option<mpsc::Receiver<GossipEvent>>,
    shutdown: Arc<RwLock<bool>>,
}

impl GossipService {
    /// Create a new gossip service.
    pub fn new(config: ReplicationConfig) -> Self {
        let (event_tx, event_rx) = mpsc::channel(1024);

        Self {
            config,
            members: Arc::new(RwLock::new(HashMap::new())),
            event_tx,
            event_rx: Some(event_rx),
            shutdown: Arc::new(RwLock::new(false)),
        }
    }

    /// Get the event receiver (can only be called once).
    pub fn take_event_rx(&mut self) -> Option<mpsc::Receiver<GossipEvent>> {
        self.event_rx.take()
    }

    /// Get all current members.
    pub fn members(&self) -> Vec<Member> {
        self.members.read().values().cloned().collect()
    }

    /// Get alive members only.
    pub fn alive_members(&self) -> Vec<Member> {
        self.members
            .read()
            .values()
            .filter(|m| m.state == MemberState::Alive)
            .cloned()
            .collect()
    }

    /// Get a specific member by ID.
    pub fn get_member(&self, node_id: &str) -> Option<Member> {
        self.members.read().get(node_id).cloned()
    }

    /// Check if we should shutdown.
    pub fn is_shutdown(&self) -> bool {
        *self.shutdown.read()
    }

    /// Signal shutdown.
    pub fn shutdown(&self) {
        *self.shutdown.write() = true;
    }

    /// Start the gossip service.
    pub async fn start(self: Arc<Self>) -> anyhow::Result<()> {
        // Bind UDP socket for gossip
        let socket = Arc::new(UdpSocket::bind(self.config.gossip_addr).await?);
        tracing::info!("gossip listening on {}", self.config.gossip_addr);

        let members = self.members.clone();
        let event_tx = self.event_tx.clone();
        let shutdown = self.shutdown.clone();
        let gossip_interval = self.config.gossip_interval;
        let node_id = self.config.node_id.clone();
        let gossip_addr = self.config.gossip_addr;
        let transport_addr = self.config.transport_addr;
        let bootstrap_peers = self.config.bootstrap_peers.clone();

        // Send join messages to bootstrap peers
        let socket_clone = socket.clone();
        tokio::spawn(async move {
            for peer in &bootstrap_peers {
                if let Ok(addr) = peer.parse::<SocketAddr>() {
                    let join_msg = GossipMessage::Join {
                        node_id: node_id.clone(),
                        gossip_addr,
                        transport_addr,
                    };
                    if let Ok(data) = bincode::serialize(&join_msg) {
                        let _ = socket_clone.send_to(&data, addr).await;
                        tracing::info!("sent join message to bootstrap peer {}", addr);
                    }
                }
            }
        });

        // Spawn gossip loop
        let socket_recv = socket.clone();
        let node_id_recv = self.config.node_id.clone();
        let gossip_addr_recv = self.config.gossip_addr;
        let transport_addr_recv = self.config.transport_addr;

        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            let mut gossip_timer = tokio::time::interval(gossip_interval);
            let mut failure_timer = tokio::time::interval(Duration::from_secs(10));
            let incarnation: u64 = 0;

            loop {
                if *shutdown.read() {
                    tracing::info!("gossip service shutting down");
                    break;
                }

                tokio::select! {
                    // Handle incoming messages
                    result = socket_recv.recv_from(&mut buf) => {
                        match result {
                            Ok((len, src)) => {
                                let data = &buf[..len];
                                if let Ok(msg) = bincode::deserialize::<GossipMessage>(data) {
                                    Self::handle_message(
                                        &msg,
                                        src,
                                        &members,
                                        &event_tx,
                                        &socket_recv,
                                        &node_id_recv,
                                        gossip_addr_recv,
                                        transport_addr_recv,
                                        incarnation,
                                    ).await;
                                }
                            }
                            Err(e) => {
                                tracing::error!("gossip recv error: {:?}", e);
                            }
                        }
                    }

                    // Periodic ping to random member
                    _ = gossip_timer.tick() => {
                        let member_addrs: Vec<SocketAddr> = members.read()
                            .values()
                            .filter(|m| m.state == MemberState::Alive)
                            .map(|m| m.gossip_addr)
                            .collect();

                        if !member_addrs.is_empty() {
                            let idx = rand::random::<usize>() % member_addrs.len();
                            let target = member_addrs[idx];

                            let ping = GossipMessage::Ping {
                                sender_id: node_id_recv.clone(),
                                sender_gossip_addr: gossip_addr_recv,
                                sender_transport_addr: transport_addr_recv,
                                incarnation,
                            };

                            if let Ok(data) = bincode::serialize(&ping) {
                                let _ = socket_recv.send_to(&data, target).await;
                            }
                        }
                    }

                    // Check for dead members
                    _ = failure_timer.tick() => {
                        let now = Instant::now();
                        let mut dead_members = Vec::new();

                        {
                            let mut guard = members.write();
                            for (id, member) in guard.iter_mut() {
                                if member.state == MemberState::Alive
                                    && now.duration_since(member.last_seen) > Duration::from_secs(30)
                                {
                                    let old_state = member.state.clone();
                                    member.state = MemberState::Dead;
                                    dead_members.push((id.clone(), old_state));
                                }
                            }
                        }

                        for (id, old_state) in dead_members {
                            let node_id = NodeId::new(&id);
                            let _ = event_tx.send(GossipEvent::MemberStateChanged {
                                node_id: node_id.clone(),
                                old_state,
                                new_state: MemberState::Dead,
                            }).await;
                            let _ = event_tx.send(GossipEvent::MemberLeft(node_id)).await;
                        }
                    }
                }
            }
        });

        Ok(())
    }

    async fn handle_message(
        msg: &GossipMessage,
        src: SocketAddr,
        members: &RwLock<HashMap<String, Member>>,
        event_tx: &mpsc::Sender<GossipEvent>,
        socket: &UdpSocket,
        local_node_id: &str,
        local_gossip_addr: SocketAddr,
        local_transport_addr: SocketAddr,
        local_incarnation: u64,
    ) {
        match msg {
            GossipMessage::Ping { sender_id, sender_gossip_addr, sender_transport_addr, incarnation } => {
                // Update member or add new one
                let member = Member {
                    node_id: NodeId::new(sender_id),
                    gossip_addr: *sender_gossip_addr,
                    transport_addr: *sender_transport_addr,
                    state: MemberState::Alive,
                    last_seen: Instant::now(),
                    incarnation: *incarnation,
                };

                let is_new = {
                    let mut guard = members.write();
                    let is_new = !guard.contains_key(sender_id);
                    guard.insert(sender_id.clone(), member.clone());
                    is_new
                };

                if is_new {
                    tracing::info!("member discovered via ping: {} at {}", sender_id, sender_gossip_addr);
                    let _ = event_tx.send(GossipEvent::MemberJoined(member)).await;
                }

                // Send ack
                let ack = GossipMessage::Ack {
                    sender_id: local_node_id.to_string(),
                    sender_gossip_addr: local_gossip_addr,
                    sender_transport_addr: local_transport_addr,
                    incarnation: local_incarnation,
                };

                if let Ok(data) = bincode::serialize(&ack) {
                    let _ = socket.send_to(&data, src).await;
                }
            }

            GossipMessage::Ack { sender_id, sender_gossip_addr, sender_transport_addr, incarnation } => {
                let member = Member {
                    node_id: NodeId::new(sender_id),
                    gossip_addr: *sender_gossip_addr,
                    transport_addr: *sender_transport_addr,
                    state: MemberState::Alive,
                    last_seen: Instant::now(),
                    incarnation: *incarnation,
                };

                let is_new = {
                    let mut guard = members.write();
                    let is_new = !guard.contains_key(sender_id);
                    guard.insert(sender_id.clone(), member.clone());
                    is_new
                };

                if is_new {
                    tracing::info!("member discovered via ack: {} at {}", sender_id, sender_gossip_addr);
                    let _ = event_tx.send(GossipEvent::MemberJoined(member)).await;
                }
            }

            GossipMessage::Join { node_id, gossip_addr, transport_addr } => {
                let member = Member {
                    node_id: NodeId::new(node_id),
                    gossip_addr: *gossip_addr,
                    transport_addr: *transport_addr,
                    state: MemberState::Alive,
                    last_seen: Instant::now(),
                    incarnation: 0,
                };

                let is_new = {
                    let mut guard = members.write();
                    let is_new = !guard.contains_key(node_id);
                    guard.insert(node_id.clone(), member.clone());
                    is_new
                };

                if is_new {
                    tracing::info!("member joined: {} at {}", node_id, gossip_addr);
                    let _ = event_tx.send(GossipEvent::MemberJoined(member)).await;
                }

                // Send member list back
                let member_list: Vec<(String, SocketAddr, SocketAddr, u64)> = members
                    .read()
                    .values()
                    .map(|m| (m.node_id.0.clone(), m.gossip_addr, m.transport_addr, m.incarnation))
                    .collect();

                let response = GossipMessage::MemberList { members: member_list };
                if let Ok(data) = bincode::serialize(&response) {
                    let _ = socket.send_to(&data, src).await;
                }
            }

            GossipMessage::MemberList { members: member_list } => {
                for (id, gossip_addr, transport_addr, incarnation) in member_list {
                    if id == local_node_id {
                        continue; // Skip self
                    }

                    let member = Member {
                        node_id: NodeId::new(id),
                        gossip_addr: *gossip_addr,
                        transport_addr: *transport_addr,
                        state: MemberState::Alive,
                        last_seen: Instant::now(),
                        incarnation: *incarnation,
                    };

                    let is_new = {
                        let mut guard = members.write();
                        let is_new = !guard.contains_key(id);
                        guard.insert(id.clone(), member.clone());
                        is_new
                    };

                    if is_new {
                        tracing::info!("member discovered from list: {} at {}", id, gossip_addr);
                        let _ = event_tx.send(GossipEvent::MemberJoined(member)).await;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_state() {
        assert_eq!(MemberState::Alive, MemberState::Alive);
        assert_ne!(MemberState::Alive, MemberState::Dead);
    }

    #[test]
    fn test_gossip_message_serialization() {
        let msg = GossipMessage::Ping {
            sender_id: "node-1".to_string(),
            sender_gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:4002".parse().unwrap(),
            incarnation: 1,
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::Ping { sender_id, incarnation, .. } => {
                assert_eq!(sender_id, "node-1");
                assert_eq!(incarnation, 1);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_gossip_service_creation() {
        let config = ReplicationConfig::new("test-node")
            .gossip_addr("127.0.0.1:4001".parse().unwrap());

        let service = GossipService::new(config);
        assert!(service.members().is_empty());
        assert!(!service.is_shutdown());
    }

    #[test]
    fn test_gossip_service_shutdown() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        assert!(!service.is_shutdown());
        service.shutdown();
        assert!(service.is_shutdown());
    }
}
