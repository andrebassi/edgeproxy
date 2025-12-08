//! Gossip Protocol (SWIM-like)
//!
//! Implements cluster membership and failure detection using a simplified
//! SWIM-inspired protocol for peer discovery and heartbeat.
//!
//! Uses Sans-IO pattern: message processing is separated from I/O for testability.

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

impl PartialEq for Member {
    fn eq(&self, other: &Self) -> bool {
        // Compare all fields except last_seen (which is time-based)
        self.node_id == other.node_id
            && self.gossip_addr == other.gossip_addr
            && self.transport_addr == other.transport_addr
            && self.state == other.state
            && self.incarnation == other.incarnation
    }
}

/// Message types for gossip protocol
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
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
#[derive(Debug, Clone, PartialEq)]
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

/// Output action from message processing (Sans-IO pattern).
/// Instead of doing I/O directly, we return actions to be performed.
#[derive(Debug, Clone, PartialEq)]
pub enum GossipAction {
    /// Send a message to a specific address
    Send { to: SocketAddr, message: GossipMessage },
    /// Emit an event
    Emit(GossipEvent),
    /// No action needed
    None,
}

/// Result of processing a gossip message (Sans-IO pattern).
#[derive(Debug, Clone)]
pub struct ProcessResult {
    /// Actions to perform (send messages, emit events)
    pub actions: Vec<GossipAction>,
    /// Whether a new member was discovered
    pub member_discovered: bool,
}

impl ProcessResult {
    /// Create empty result
    pub fn empty() -> Self {
        Self {
            actions: Vec::new(),
            member_discovered: false,
        }
    }

    /// Create result with single send action
    pub fn send(to: SocketAddr, message: GossipMessage) -> Self {
        Self {
            actions: vec![GossipAction::Send { to, message }],
            member_discovered: false,
        }
    }

    /// Add an action
    pub fn with_action(mut self, action: GossipAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Mark as member discovered
    pub fn with_member_discovered(mut self) -> Self {
        self.member_discovered = true;
        self
    }
}

/// Pure function to process a gossip message (Sans-IO pattern).
/// Returns actions to be performed instead of doing I/O directly.
/// This enables unit testing of message processing logic.
pub fn process_message(
    msg: &GossipMessage,
    src: SocketAddr,
    members: &RwLock<HashMap<String, Member>>,
    local_node_id: &str,
    local_gossip_addr: SocketAddr,
    local_transport_addr: SocketAddr,
    local_incarnation: u64,
) -> ProcessResult {
    match msg {
        GossipMessage::Ping { sender_id, sender_gossip_addr, sender_transport_addr, incarnation } => {
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

            let ack = GossipMessage::Ack {
                sender_id: local_node_id.to_string(),
                sender_gossip_addr: local_gossip_addr,
                sender_transport_addr: local_transport_addr,
                incarnation: local_incarnation,
            };

            let mut result = ProcessResult::send(src, ack);
            if is_new {
                result.member_discovered = true;
                result.actions.push(GossipAction::Emit(GossipEvent::MemberJoined(member)));
            }
            result
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
                ProcessResult::empty()
                    .with_member_discovered()
                    .with_action(GossipAction::Emit(GossipEvent::MemberJoined(member)))
            } else {
                ProcessResult::empty()
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

            // Build member list response
            let member_list: Vec<(String, SocketAddr, SocketAddr, u64)> = members
                .read()
                .values()
                .map(|m| (m.node_id.0.clone(), m.gossip_addr, m.transport_addr, m.incarnation))
                .collect();

            let response = GossipMessage::MemberList { members: member_list };
            let mut result = ProcessResult::send(src, response);

            if is_new {
                result.member_discovered = true;
                result.actions.push(GossipAction::Emit(GossipEvent::MemberJoined(member)));
            }
            result
        }

        GossipMessage::MemberList { members: member_list } => {
            let mut result = ProcessResult::empty();

            for (id, gossip_addr, transport_addr, incarnation) in member_list {
                if id == local_node_id {
                    continue;
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
                    result.member_discovered = true;
                    result.actions.push(GossipAction::Emit(GossipEvent::MemberJoined(member)));
                }
            }
            result
        }
    }
}

/// Check members for failures and return dead member events (Sans-IO pattern).
pub fn check_member_failures(
    members: &RwLock<HashMap<String, Member>>,
    timeout: Duration,
) -> Vec<GossipAction> {
    let now = Instant::now();
    let mut actions = Vec::new();
    let mut dead_members = Vec::new();

    {
        let mut guard = members.write();
        for (id, member) in guard.iter_mut() {
            if member.state == MemberState::Alive
                && now.duration_since(member.last_seen) > timeout
            {
                let old_state = member.state.clone();
                member.state = MemberState::Dead;
                dead_members.push((id.clone(), old_state));
            }
        }
    }

    for (id, old_state) in dead_members {
        let node_id = NodeId::new(&id);
        actions.push(GossipAction::Emit(GossipEvent::MemberStateChanged {
            node_id: node_id.clone(),
            old_state,
            new_state: MemberState::Dead,
        }));
        actions.push(GossipAction::Emit(GossipEvent::MemberLeft(node_id)));
    }

    actions
}

/// Select random member for ping (Sans-IO pattern).
pub fn select_ping_target(members: &RwLock<HashMap<String, Member>>) -> Option<SocketAddr> {
    let member_addrs: Vec<SocketAddr> = members
        .read()
        .values()
        .filter(|m| m.state == MemberState::Alive)
        .map(|m| m.gossip_addr)
        .collect();

    if member_addrs.is_empty() {
        return None;
    }

    let idx = rand::random::<usize>() % member_addrs.len();
    Some(member_addrs[idx])
}

/// Create a ping message (Sans-IO pattern).
pub fn create_ping(
    node_id: &str,
    gossip_addr: SocketAddr,
    transport_addr: SocketAddr,
    incarnation: u64,
) -> GossipMessage {
    GossipMessage::Ping {
        sender_id: node_id.to_string(),
        sender_gossip_addr: gossip_addr,
        sender_transport_addr: transport_addr,
        incarnation,
    }
}

/// Create a join message (Sans-IO pattern).
pub fn create_join(
    node_id: &str,
    gossip_addr: SocketAddr,
    transport_addr: SocketAddr,
) -> GossipMessage {
    GossipMessage::Join {
        node_id: node_id.to_string(),
        gossip_addr,
        transport_addr,
    }
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
    #[cfg_attr(coverage_nightly, coverage(off))]
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

    /// Execute gossip actions (Sans-IO pattern).
    /// Sends messages and emits events based on the action list.
    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn execute_actions(
        actions: Vec<GossipAction>,
        socket: &UdpSocket,
        event_tx: &mpsc::Sender<GossipEvent>,
    ) {
        for action in actions {
            match action {
                GossipAction::Send { to, message } => {
                    if let Ok(data) = bincode::serialize(&message) {
                        let _ = socket.send_to(&data, to).await;
                    }
                }
                GossipAction::Emit(event) => {
                    let _ = event_tx.send(event).await;
                }
                GossipAction::None => {}
            }
        }
    }

    /// Handle incoming gossip message using Sans-IO pattern.
    /// Delegates to process_message() and executes returned actions.
    #[cfg_attr(coverage_nightly, coverage(off))]
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
        // Use Sans-IO process_message to get actions
        let result = process_message(
            msg,
            src,
            members,
            local_node_id,
            local_gossip_addr,
            local_transport_addr,
            local_incarnation,
        );

        // Log member discoveries
        if result.member_discovered {
            tracing::info!("member discovered via {:?}", msg);
        }

        // Execute all actions
        Self::execute_actions(result.actions, socket, event_tx).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_member_state() {
        assert_eq!(MemberState::Alive, MemberState::Alive);
        assert_ne!(MemberState::Alive, MemberState::Dead);
        assert_ne!(MemberState::Alive, MemberState::Suspect);
        assert_ne!(MemberState::Dead, MemberState::Suspect);
    }

    #[test]
    fn test_member_state_clone() {
        let state = MemberState::Alive;
        let cloned = state.clone();
        assert_eq!(state, cloned);

        let state = MemberState::Suspect;
        let cloned = state.clone();
        assert_eq!(state, cloned);

        let state = MemberState::Dead;
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }

    #[test]
    fn test_member_state_debug() {
        assert_eq!(format!("{:?}", MemberState::Alive), "Alive");
        assert_eq!(format!("{:?}", MemberState::Suspect), "Suspect");
        assert_eq!(format!("{:?}", MemberState::Dead), "Dead");
    }

    #[test]
    fn test_member_creation() {
        let member = Member {
            node_id: NodeId::new("node-1"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 0,
        };

        assert_eq!(member.node_id.as_str(), "node-1");
        assert_eq!(member.state, MemberState::Alive);
        assert_eq!(member.incarnation, 0);
    }

    #[test]
    fn test_member_clone() {
        let member = Member {
            node_id: NodeId::new("node-1"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 5,
        };

        let cloned = member.clone();
        assert_eq!(cloned.node_id.as_str(), "node-1");
        assert_eq!(cloned.incarnation, 5);
    }

    #[test]
    fn test_member_debug() {
        let member = Member {
            node_id: NodeId::new("node-1"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 0,
        };

        let debug = format!("{:?}", member);
        assert!(debug.contains("node-1"));
        assert!(debug.contains("Alive"));
    }

    #[test]
    fn test_gossip_message_ping_serialization() {
        let msg = GossipMessage::Ping {
            sender_id: "node-1".to_string(),
            sender_gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:4002".parse().unwrap(),
            incarnation: 1,
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::Ping { sender_id, incarnation, sender_gossip_addr, sender_transport_addr } => {
                assert_eq!(sender_id, "node-1");
                assert_eq!(incarnation, 1);
                assert_eq!(sender_gossip_addr, "127.0.0.1:4001".parse::<SocketAddr>().unwrap());
                assert_eq!(sender_transport_addr, "127.0.0.1:4002".parse::<SocketAddr>().unwrap());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_gossip_message_ack_serialization() {
        let msg = GossipMessage::Ack {
            sender_id: "node-2".to_string(),
            sender_gossip_addr: "127.0.0.1:5001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:5002".parse().unwrap(),
            incarnation: 42,
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::Ack { sender_id, incarnation, .. } => {
                assert_eq!(sender_id, "node-2");
                assert_eq!(incarnation, 42);
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_gossip_message_join_serialization() {
        let msg = GossipMessage::Join {
            node_id: "new-node".to_string(),
            gossip_addr: "10.0.0.1:4001".parse().unwrap(),
            transport_addr: "10.0.0.1:4002".parse().unwrap(),
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::Join { node_id, gossip_addr, transport_addr } => {
                assert_eq!(node_id, "new-node");
                assert_eq!(gossip_addr, "10.0.0.1:4001".parse::<SocketAddr>().unwrap());
                assert_eq!(transport_addr, "10.0.0.1:4002".parse::<SocketAddr>().unwrap());
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_gossip_message_member_list_serialization() {
        let members = vec![
            ("node-1".to_string(), "127.0.0.1:4001".parse().unwrap(), "127.0.0.1:4002".parse().unwrap(), 1u64),
            ("node-2".to_string(), "127.0.0.2:4001".parse().unwrap(), "127.0.0.2:4002".parse().unwrap(), 2u64),
        ];

        let msg = GossipMessage::MemberList { members: members.clone() };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::MemberList { members: decoded_members } => {
                assert_eq!(decoded_members.len(), 2);
                assert_eq!(decoded_members[0].0, "node-1");
                assert_eq!(decoded_members[1].0, "node-2");
            }
            _ => panic!("wrong message type"),
        }
    }

    #[test]
    fn test_gossip_message_debug() {
        let msg = GossipMessage::Ping {
            sender_id: "node-1".to_string(),
            sender_gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:4002".parse().unwrap(),
            incarnation: 1,
        };

        let debug = format!("{:?}", msg);
        assert!(debug.contains("Ping"));
        assert!(debug.contains("node-1"));
    }

    #[test]
    fn test_gossip_message_clone() {
        let msg = GossipMessage::Join {
            node_id: "test".to_string(),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
        };

        let cloned = msg.clone();
        match cloned {
            GossipMessage::Join { node_id, .. } => assert_eq!(node_id, "test"),
            _ => panic!("wrong type"),
        }
    }

    #[test]
    fn test_gossip_event_member_joined() {
        let member = Member {
            node_id: NodeId::new("node-1"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 0,
        };

        let event = GossipEvent::MemberJoined(member);
        let debug = format!("{:?}", event);
        assert!(debug.contains("MemberJoined"));
    }

    #[test]
    fn test_gossip_event_member_left() {
        let event = GossipEvent::MemberLeft(NodeId::new("node-1"));
        let debug = format!("{:?}", event);
        assert!(debug.contains("MemberLeft"));
        assert!(debug.contains("node-1"));
    }

    #[test]
    fn test_gossip_event_member_state_changed() {
        let event = GossipEvent::MemberStateChanged {
            node_id: NodeId::new("node-1"),
            old_state: MemberState::Alive,
            new_state: MemberState::Dead,
        };

        let debug = format!("{:?}", event);
        assert!(debug.contains("MemberStateChanged"));
        assert!(debug.contains("Alive"));
        assert!(debug.contains("Dead"));
    }

    #[test]
    fn test_gossip_event_clone() {
        let event = GossipEvent::MemberLeft(NodeId::new("node-1"));
        let cloned = event.clone();
        let debug = format!("{:?}", cloned);
        assert!(debug.contains("MemberLeft"));
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

    #[test]
    fn test_gossip_service_take_event_rx() {
        let config = ReplicationConfig::new("test-node");
        let mut service = GossipService::new(config);

        // First call should return Some
        let rx = service.take_event_rx();
        assert!(rx.is_some());

        // Second call should return None
        let rx = service.take_event_rx();
        assert!(rx.is_none());
    }

    #[test]
    fn test_gossip_service_get_member_none() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        let member = service.get_member("nonexistent");
        assert!(member.is_none());
    }

    #[test]
    fn test_gossip_service_alive_members_empty() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        assert!(service.alive_members().is_empty());
    }

    #[test]
    fn test_gossip_service_members_with_data() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        // Add a member directly to test
        {
            let member = Member {
                node_id: NodeId::new("peer-1"),
                gossip_addr: "127.0.0.1:5001".parse().unwrap(),
                transport_addr: "127.0.0.1:5002".parse().unwrap(),
                state: MemberState::Alive,
                last_seen: Instant::now(),
                incarnation: 1,
            };
            service.members.write().insert("peer-1".to_string(), member);
        }

        assert_eq!(service.members().len(), 1);
        assert_eq!(service.alive_members().len(), 1);

        let member = service.get_member("peer-1");
        assert!(member.is_some());
        assert_eq!(member.unwrap().node_id.as_str(), "peer-1");
    }

    #[test]
    fn test_gossip_service_alive_members_filters_dead() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        // Add alive member
        {
            let member = Member {
                node_id: NodeId::new("alive-node"),
                gossip_addr: "127.0.0.1:5001".parse().unwrap(),
                transport_addr: "127.0.0.1:5002".parse().unwrap(),
                state: MemberState::Alive,
                last_seen: Instant::now(),
                incarnation: 1,
            };
            service.members.write().insert("alive-node".to_string(), member);
        }

        // Add dead member
        {
            let member = Member {
                node_id: NodeId::new("dead-node"),
                gossip_addr: "127.0.0.1:6001".parse().unwrap(),
                transport_addr: "127.0.0.1:6002".parse().unwrap(),
                state: MemberState::Dead,
                last_seen: Instant::now(),
                incarnation: 1,
            };
            service.members.write().insert("dead-node".to_string(), member);
        }

        // Add suspect member
        {
            let member = Member {
                node_id: NodeId::new("suspect-node"),
                gossip_addr: "127.0.0.1:7001".parse().unwrap(),
                transport_addr: "127.0.0.1:7002".parse().unwrap(),
                state: MemberState::Suspect,
                last_seen: Instant::now(),
                incarnation: 1,
            };
            service.members.write().insert("suspect-node".to_string(), member);
        }

        assert_eq!(service.members().len(), 3);
        assert_eq!(service.alive_members().len(), 1);
        assert_eq!(service.alive_members()[0].node_id.as_str(), "alive-node");
    }

    #[tokio::test]
    async fn test_gossip_service_start_and_shutdown() {
        let config = ReplicationConfig::new("test-node")
            .gossip_addr("127.0.0.1:0".parse().unwrap());

        let service = Arc::new(GossipService::new(config));

        // Start should succeed
        let result = service.clone().start().await;
        assert!(result.is_ok());

        // Shutdown
        service.shutdown();
        assert!(service.is_shutdown());
    }

    #[tokio::test]
    async fn test_gossip_service_start_with_bootstrap_peers() {
        let config = ReplicationConfig::new("test-node")
            .gossip_addr("127.0.0.1:0".parse().unwrap())
            .bootstrap_peers(vec!["127.0.0.1:9999".to_string()]);

        let service = Arc::new(GossipService::new(config));
        let result = service.clone().start().await;
        assert!(result.is_ok());

        // Give time for bootstrap messages
        tokio::time::sleep(Duration::from_millis(50)).await;

        service.shutdown();
    }

    // Test member manipulation directly (no network)
    #[test]
    fn test_gossip_service_add_member_directly() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        // Add member directly through Arc<RwLock>
        let member = Member {
            node_id: NodeId::new("direct-member"),
            gossip_addr: "127.0.0.1:5001".parse().unwrap(),
            transport_addr: "127.0.0.1:5002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 10,
        };
        service.members.write().insert("direct-member".to_string(), member);

        let retrieved = service.get_member("direct-member");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().incarnation, 10);
    }

    #[test]
    fn test_gossip_service_member_state_transitions() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        // Add alive member
        let alive_member = Member {
            node_id: NodeId::new("state-member"),
            gossip_addr: "127.0.0.1:5001".parse().unwrap(),
            transport_addr: "127.0.0.1:5002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        };
        service.members.write().insert("state-member".to_string(), alive_member);

        assert_eq!(service.alive_members().len(), 1);

        // Change to suspect
        {
            let mut guard = service.members.write();
            if let Some(m) = guard.get_mut("state-member") {
                m.state = MemberState::Suspect;
            }
        }
        assert_eq!(service.alive_members().len(), 0);

        // Change to dead
        {
            let mut guard = service.members.write();
            if let Some(m) = guard.get_mut("state-member") {
                m.state = MemberState::Dead;
            }
        }
        assert_eq!(service.members().len(), 1);
        assert_eq!(service.alive_members().len(), 0);
    }

    #[test]
    fn test_gossip_service_multiple_members_states() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        // Add multiple members in different states
        let states = vec![
            ("alive-1", MemberState::Alive),
            ("alive-2", MemberState::Alive),
            ("suspect-1", MemberState::Suspect),
            ("dead-1", MemberState::Dead),
        ];

        for (id, state) in &states {
            let member = Member {
                node_id: NodeId::new(*id),
                gossip_addr: "127.0.0.1:5001".parse().unwrap(),
                transport_addr: "127.0.0.1:5002".parse().unwrap(),
                state: state.clone(),
                last_seen: Instant::now(),
                incarnation: 1,
            };
            service.members.write().insert(id.to_string(), member);
        }

        assert_eq!(service.members().len(), 4);
        assert_eq!(service.alive_members().len(), 2);
    }

    #[test]
    fn test_gossip_message_empty_member_list() {
        let msg = GossipMessage::MemberList { members: vec![] };
        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::MemberList { members } => {
                assert!(members.is_empty());
            }
            _ => panic!("wrong type"),
        }
    }

    #[test]
    fn test_member_with_high_incarnation() {
        let member = Member {
            node_id: NodeId::new("high-incarn"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: u64::MAX,
        };

        assert_eq!(member.incarnation, u64::MAX);
    }

    #[test]
    fn test_gossip_message_with_ipv6() {
        let msg = GossipMessage::Ping {
            sender_id: "ipv6-node".to_string(),
            sender_gossip_addr: "[::1]:4001".parse().unwrap(),
            sender_transport_addr: "[::1]:4002".parse().unwrap(),
            incarnation: 1,
        };

        let data = bincode::serialize(&msg).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&data).unwrap();

        match decoded {
            GossipMessage::Ping { sender_gossip_addr, sender_transport_addr, .. } => {
                assert_eq!(sender_gossip_addr.to_string(), "[::1]:4001");
                assert_eq!(sender_transport_addr.to_string(), "[::1]:4002");
            }
            _ => panic!("wrong type"),
        }
    }

    #[test]
    fn test_gossip_event_debug_formats() {
        // MemberJoined with full member
        let member = Member {
            node_id: NodeId::new("debug-node"),
            gossip_addr: "10.0.0.1:4001".parse().unwrap(),
            transport_addr: "10.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 42,
        };
        let event1 = GossipEvent::MemberJoined(member);
        let debug1 = format!("{:?}", event1);
        assert!(debug1.contains("debug-node"));
        assert!(debug1.contains("42"));

        // MemberStateChanged with all states
        let event2 = GossipEvent::MemberStateChanged {
            node_id: NodeId::new("state-node"),
            old_state: MemberState::Suspect,
            new_state: MemberState::Alive,
        };
        let debug2 = format!("{:?}", event2);
        assert!(debug2.contains("Suspect"));
        assert!(debug2.contains("Alive"));
    }

    #[tokio::test]
    async fn test_gossip_service_start_binds_socket() {
        let config = ReplicationConfig::new("bind-test")
            .gossip_addr("127.0.0.1:0".parse().unwrap());
        let service = Arc::new(GossipService::new(config));

        // Start should succeed and bind to an ephemeral port
        let result = service.clone().start().await;
        assert!(result.is_ok());

        // Small delay to ensure socket is bound
        tokio::time::sleep(Duration::from_millis(10)).await;

        service.shutdown();
        assert!(service.is_shutdown());
    }

    #[test]
    fn test_gossip_service_shutdown_idempotent() {
        let config = ReplicationConfig::new("test-node");
        let service = GossipService::new(config);

        assert!(!service.is_shutdown());

        service.shutdown();
        assert!(service.is_shutdown());

        // Calling shutdown again should be fine
        service.shutdown();
        assert!(service.is_shutdown());
    }

    // ==================== Sans-IO Tests ====================

    #[test]
    fn test_process_result_empty() {
        let result = ProcessResult::empty();
        assert!(result.actions.is_empty());
        assert!(!result.member_discovered);
    }

    #[test]
    fn test_process_result_send() {
        let addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let msg = GossipMessage::Ping {
            sender_id: "test".to_string(),
            sender_gossip_addr: addr,
            sender_transport_addr: addr,
            incarnation: 1,
        };

        let result = ProcessResult::send(addr, msg.clone());
        assert_eq!(result.actions.len(), 1);
        match &result.actions[0] {
            GossipAction::Send { to, message: _ } => {
                assert_eq!(*to, addr);
            }
            _ => panic!("expected Send action"),
        }
    }

    #[test]
    fn test_process_result_with_action() {
        let result = ProcessResult::empty()
            .with_action(GossipAction::None);
        assert_eq!(result.actions.len(), 1);
        assert_eq!(result.actions[0], GossipAction::None);
    }

    #[test]
    fn test_process_result_with_member_discovered() {
        let result = ProcessResult::empty().with_member_discovered();
        assert!(result.member_discovered);
    }

    #[test]
    fn test_process_result_chaining() {
        let addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let result = ProcessResult::empty()
            .with_action(GossipAction::None)
            .with_member_discovered()
            .with_action(GossipAction::Send {
                to: addr,
                message: create_ping("test", addr, addr, 1),
            });

        assert_eq!(result.actions.len(), 2);
        assert!(result.member_discovered);
    }

    #[test]
    fn test_gossip_action_debug() {
        let action = GossipAction::None;
        let debug = format!("{:?}", action);
        assert!(debug.contains("None"));

        let addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let action2 = GossipAction::Send {
            to: addr,
            message: create_ping("test", addr, addr, 1),
        };
        let debug2 = format!("{:?}", action2);
        assert!(debug2.contains("Send"));
    }

    #[test]
    fn test_process_message_ping_new_member() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let sender_gossip: SocketAddr = "10.0.0.1:4001".parse().unwrap();
        let sender_transport: SocketAddr = "10.0.0.1:4002".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = GossipMessage::Ping {
            sender_id: "peer-1".to_string(),
            sender_gossip_addr: sender_gossip,
            sender_transport_addr: sender_transport,
            incarnation: 5,
        };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        // Should discover new member
        assert!(result.member_discovered);

        // Should have Send (ack) and Emit (MemberJoined) actions
        assert_eq!(result.actions.len(), 2);

        // First action should be Send (ack)
        match &result.actions[0] {
            GossipAction::Send { to, message } => {
                assert_eq!(*to, src);
                match message {
                    GossipMessage::Ack { sender_id, .. } => {
                        assert_eq!(sender_id, "local-node");
                    }
                    _ => panic!("expected Ack message"),
                }
            }
            _ => panic!("expected Send action"),
        }

        // Second action should be Emit (MemberJoined)
        match &result.actions[1] {
            GossipAction::Emit(GossipEvent::MemberJoined(member)) => {
                assert_eq!(member.node_id.as_str(), "peer-1");
                assert_eq!(member.incarnation, 5);
            }
            _ => panic!("expected Emit MemberJoined"),
        }

        // Member should be added to map
        assert!(members.read().contains_key("peer-1"));
    }

    #[test]
    fn test_process_message_ping_existing_member() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let sender_gossip: SocketAddr = "10.0.0.1:4001".parse().unwrap();
        let sender_transport: SocketAddr = "10.0.0.1:4002".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        // Pre-add member
        members.write().insert("peer-1".to_string(), Member {
            node_id: NodeId::new("peer-1"),
            gossip_addr: sender_gossip,
            transport_addr: sender_transport,
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        let msg = GossipMessage::Ping {
            sender_id: "peer-1".to_string(),
            sender_gossip_addr: sender_gossip,
            sender_transport_addr: sender_transport,
            incarnation: 5,
        };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        // Should NOT discover (existing member)
        assert!(!result.member_discovered);

        // Should only have Send (ack)
        assert_eq!(result.actions.len(), 1);
        match &result.actions[0] {
            GossipAction::Send { .. } => {}
            _ => panic!("expected Send action"),
        }
    }

    #[test]
    fn test_process_message_ack_new_member() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let sender_gossip: SocketAddr = "10.0.0.2:4001".parse().unwrap();
        let sender_transport: SocketAddr = "10.0.0.2:4002".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = GossipMessage::Ack {
            sender_id: "peer-2".to_string(),
            sender_gossip_addr: sender_gossip,
            sender_transport_addr: sender_transport,
            incarnation: 10,
        };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        assert!(result.member_discovered);
        assert_eq!(result.actions.len(), 1);

        match &result.actions[0] {
            GossipAction::Emit(GossipEvent::MemberJoined(member)) => {
                assert_eq!(member.node_id.as_str(), "peer-2");
                assert_eq!(member.incarnation, 10);
            }
            _ => panic!("expected Emit MemberJoined"),
        }
    }

    #[test]
    fn test_process_message_ack_existing_member() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let sender_gossip: SocketAddr = "10.0.0.2:4001".parse().unwrap();
        let sender_transport: SocketAddr = "10.0.0.2:4002".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        // Pre-add member
        members.write().insert("peer-2".to_string(), Member {
            node_id: NodeId::new("peer-2"),
            gossip_addr: sender_gossip,
            transport_addr: sender_transport,
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 5,
        });

        let msg = GossipMessage::Ack {
            sender_id: "peer-2".to_string(),
            sender_gossip_addr: sender_gossip,
            sender_transport_addr: sender_transport,
            incarnation: 10,
        };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        // Not a new member
        assert!(!result.member_discovered);
        assert!(result.actions.is_empty());
    }

    #[test]
    fn test_process_message_join_new_member() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.3:5000".parse().unwrap();
        let joiner_gossip: SocketAddr = "10.0.0.3:4001".parse().unwrap();
        let joiner_transport: SocketAddr = "10.0.0.3:4002".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = GossipMessage::Join {
            node_id: "new-peer".to_string(),
            gossip_addr: joiner_gossip,
            transport_addr: joiner_transport,
        };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        assert!(result.member_discovered);
        assert_eq!(result.actions.len(), 2);

        // First should be Send (MemberList)
        match &result.actions[0] {
            GossipAction::Send { to, message } => {
                assert_eq!(*to, src);
                match message {
                    GossipMessage::MemberList { members: list } => {
                        // Should contain the newly added member
                        assert!(!list.is_empty());
                    }
                    _ => panic!("expected MemberList"),
                }
            }
            _ => panic!("expected Send action"),
        }

        // Second should be Emit (MemberJoined)
        match &result.actions[1] {
            GossipAction::Emit(GossipEvent::MemberJoined(member)) => {
                assert_eq!(member.node_id.as_str(), "new-peer");
            }
            _ => panic!("expected Emit MemberJoined"),
        }
    }

    #[test]
    fn test_process_message_member_list() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let member_list = vec![
            ("peer-1".to_string(), "10.0.0.1:4001".parse().unwrap(), "10.0.0.1:4002".parse().unwrap(), 1u64),
            ("peer-2".to_string(), "10.0.0.2:4001".parse().unwrap(), "10.0.0.2:4002".parse().unwrap(), 2u64),
            ("local-node".to_string(), local_gossip, local_transport, 1u64), // Should be skipped
        ];

        let msg = GossipMessage::MemberList { members: member_list };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        // Should discover 2 members (local-node is skipped)
        assert!(result.member_discovered);
        assert_eq!(result.actions.len(), 2);

        // All actions should be Emit (MemberJoined)
        for action in &result.actions {
            match action {
                GossipAction::Emit(GossipEvent::MemberJoined(_)) => {}
                _ => panic!("expected Emit MemberJoined"),
            }
        }

        // Members map should have 2 entries
        assert_eq!(members.read().len(), 2);
        assert!(members.read().contains_key("peer-1"));
        assert!(members.read().contains_key("peer-2"));
        assert!(!members.read().contains_key("local-node"));
    }

    #[test]
    fn test_process_message_member_list_empty() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let src: SocketAddr = "10.0.0.5:5000".parse().unwrap();
        let local_gossip: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let local_transport: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = GossipMessage::MemberList { members: vec![] };

        let result = process_message(
            &msg,
            src,
            &members,
            "local-node",
            local_gossip,
            local_transport,
            1,
        );

        assert!(!result.member_discovered);
        assert!(result.actions.is_empty());
    }

    #[test]
    fn test_check_member_failures_no_failures() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add recent member
        members.write().insert("alive".to_string(), Member {
            node_id: NodeId::new("alive"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        let actions = check_member_failures(&members, Duration::from_secs(30));
        assert!(actions.is_empty());
    }

    #[test]
    fn test_check_member_failures_with_failure() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add stale member (last seen long ago)
        members.write().insert("stale".to_string(), Member {
            node_id: NodeId::new("stale"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now() - Duration::from_secs(60), // 60s ago
            incarnation: 1,
        });

        let actions = check_member_failures(&members, Duration::from_secs(30));

        // Should have 2 actions: StateChanged and MemberLeft
        assert_eq!(actions.len(), 2);

        match &actions[0] {
            GossipAction::Emit(GossipEvent::MemberStateChanged { node_id, old_state, new_state }) => {
                assert_eq!(node_id.as_str(), "stale");
                assert_eq!(*old_state, MemberState::Alive);
                assert_eq!(*new_state, MemberState::Dead);
            }
            _ => panic!("expected MemberStateChanged"),
        }

        match &actions[1] {
            GossipAction::Emit(GossipEvent::MemberLeft(node_id)) => {
                assert_eq!(node_id.as_str(), "stale");
            }
            _ => panic!("expected MemberLeft"),
        }

        // Member state should be updated
        assert_eq!(members.read().get("stale").unwrap().state, MemberState::Dead);
    }

    #[test]
    fn test_check_member_failures_already_dead() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add already dead member
        members.write().insert("dead".to_string(), Member {
            node_id: NodeId::new("dead"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Dead,
            last_seen: Instant::now() - Duration::from_secs(60),
            incarnation: 1,
        });

        let actions = check_member_failures(&members, Duration::from_secs(30));

        // Should not generate actions for already dead member
        assert!(actions.is_empty());
    }

    #[test]
    fn test_select_ping_target_empty() {
        let members = Arc::new(RwLock::new(HashMap::new()));
        let target = select_ping_target(&members);
        assert!(target.is_none());
    }

    #[test]
    fn test_select_ping_target_with_members() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        let addr: SocketAddr = "10.0.0.1:4001".parse().unwrap();
        members.write().insert("peer-1".to_string(), Member {
            node_id: NodeId::new("peer-1"),
            gossip_addr: addr,
            transport_addr: "10.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        let target = select_ping_target(&members);
        assert!(target.is_some());
        assert_eq!(target.unwrap(), addr);
    }

    #[test]
    fn test_select_ping_target_skips_dead() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add dead member
        members.write().insert("dead".to_string(), Member {
            node_id: NodeId::new("dead"),
            gossip_addr: "10.0.0.1:4001".parse().unwrap(),
            transport_addr: "10.0.0.1:4002".parse().unwrap(),
            state: MemberState::Dead,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        // Add alive member
        let alive_addr: SocketAddr = "10.0.0.2:4001".parse().unwrap();
        members.write().insert("alive".to_string(), Member {
            node_id: NodeId::new("alive"),
            gossip_addr: alive_addr,
            transport_addr: "10.0.0.2:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        let target = select_ping_target(&members);
        assert!(target.is_some());
        // Should select the alive member, not the dead one
        assert_eq!(target.unwrap(), alive_addr);
    }

    #[test]
    fn test_create_ping() {
        let gossip_addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let transport_addr: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = create_ping("my-node", gossip_addr, transport_addr, 42);

        match msg {
            GossipMessage::Ping { sender_id, sender_gossip_addr, sender_transport_addr, incarnation } => {
                assert_eq!(sender_id, "my-node");
                assert_eq!(sender_gossip_addr, gossip_addr);
                assert_eq!(sender_transport_addr, transport_addr);
                assert_eq!(incarnation, 42);
            }
            _ => panic!("expected Ping"),
        }
    }

    #[test]
    fn test_create_join() {
        let gossip_addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let transport_addr: SocketAddr = "127.0.0.1:4002".parse().unwrap();

        let msg = create_join("joining-node", gossip_addr, transport_addr);

        match msg {
            GossipMessage::Join { node_id, gossip_addr: ga, transport_addr: ta } => {
                assert_eq!(node_id, "joining-node");
                assert_eq!(ga, gossip_addr);
                assert_eq!(ta, transport_addr);
            }
            _ => panic!("expected Join"),
        }
    }

    #[test]
    fn test_member_partial_eq() {
        let m1 = Member {
            node_id: NodeId::new("test"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        };

        let m2 = Member {
            node_id: NodeId::new("test"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now() - Duration::from_secs(10), // Different time
            incarnation: 1,
        };

        // Should be equal (last_seen is ignored in comparison)
        assert_eq!(m1, m2);
    }

    #[test]
    fn test_member_partial_eq_different() {
        let m1 = Member {
            node_id: NodeId::new("test-1"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        };

        let m2 = Member {
            node_id: NodeId::new("test-2"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        };

        assert_ne!(m1, m2);
    }

    #[test]
    fn test_gossip_event_partial_eq() {
        let m = Member {
            node_id: NodeId::new("test"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 1,
        };

        let e1 = GossipEvent::MemberJoined(m.clone());
        let e2 = GossipEvent::MemberJoined(m);

        assert_eq!(e1, e2);

        let e3 = GossipEvent::MemberLeft(NodeId::new("test"));
        assert_ne!(e1, e3);
    }

    #[test]
    fn test_process_result_debug() {
        let result = ProcessResult::empty();
        let debug = format!("{:?}", result);
        assert!(debug.contains("ProcessResult"));
    }

    #[test]
    fn test_process_result_clone() {
        let result = ProcessResult::empty()
            .with_member_discovered()
            .with_action(GossipAction::None);

        let cloned = result.clone();
        assert_eq!(cloned.member_discovered, result.member_discovered);
        assert_eq!(cloned.actions.len(), result.actions.len());
    }

    #[test]
    fn test_gossip_action_clone() {
        let addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let action = GossipAction::Send {
            to: addr,
            message: create_ping("test", addr, addr, 1),
        };

        let cloned = action.clone();
        assert_eq!(action, cloned);
    }

    #[test]
    fn test_gossip_service_get_member_not_found() {
        let config = ReplicationConfig::default();
        let service = GossipService::new(config);

        // Member not found
        assert!(service.get_member("unknown").is_none());

        // Add a member
        service.members.write().insert("found".to_string(), Member {
            node_id: NodeId::new("found"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Alive,
            last_seen: Instant::now(),
            incarnation: 5,
        });

        // Member found
        let member = service.get_member("found");
        assert!(member.is_some());
        assert_eq!(member.unwrap().incarnation, 5);
    }

    #[test]
    fn test_member_state_eq_variants() {
        let state1 = MemberState::Alive;
        let state2 = state1.clone();
        assert_eq!(state1, state2);

        let state3 = MemberState::Suspect;
        assert_ne!(state1, state3);

        let state4 = MemberState::Dead;
        assert_ne!(state3, state4);
    }

    #[test]
    fn test_gossip_message_bincode_all_variants() {
        // Test Ping serialization
        let ping = GossipMessage::Ping {
            sender_id: "node-1".to_string(),
            sender_gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:4002".parse().unwrap(),
            incarnation: 1,
        };
        let bytes = bincode::serialize(&ping).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(ping, decoded);

        // Test Ack serialization
        let ack = GossipMessage::Ack {
            sender_id: "node-1".to_string(),
            sender_gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            sender_transport_addr: "127.0.0.1:4002".parse().unwrap(),
            incarnation: 1,
        };
        let bytes = bincode::serialize(&ack).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(ack, decoded);

        // Test Join serialization
        let join = GossipMessage::Join {
            node_id: "node-1".to_string(),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
        };
        let bytes = bincode::serialize(&join).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(join, decoded);

        // Test MemberList serialization
        let member_list = GossipMessage::MemberList {
            members: vec![
                ("node-1".to_string(), "127.0.0.1:4001".parse().unwrap(), "127.0.0.1:4002".parse().unwrap(), 1),
            ],
        };
        let bytes = bincode::serialize(&member_list).unwrap();
        let decoded: GossipMessage = bincode::deserialize(&bytes).unwrap();
        assert_eq!(member_list, decoded);
    }

    #[test]
    fn test_process_result_chain_builder() {
        let addr: SocketAddr = "127.0.0.1:4001".parse().unwrap();
        let result = ProcessResult::empty()
            .with_action(GossipAction::None)
            .with_action(GossipAction::Send { to: addr, message: create_ping("test", addr, addr, 1) })
            .with_member_discovered();

        assert!(result.member_discovered);
        assert_eq!(result.actions.len(), 2);
    }

    #[test]
    fn test_select_ping_target_all_members_dead() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add only dead members
        members.write().insert("dead1".to_string(), Member {
            node_id: NodeId::new("dead1"),
            gossip_addr: "10.0.0.1:4001".parse().unwrap(),
            transport_addr: "10.0.0.1:4002".parse().unwrap(),
            state: MemberState::Dead,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        members.write().insert("dead2".to_string(), Member {
            node_id: NodeId::new("dead2"),
            gossip_addr: "10.0.0.2:4001".parse().unwrap(),
            transport_addr: "10.0.0.2:4002".parse().unwrap(),
            state: MemberState::Dead,
            last_seen: Instant::now(),
            incarnation: 1,
        });

        // Should return None because all are dead
        let target = select_ping_target(&members);
        assert!(target.is_none());
    }

    #[test]
    fn test_check_member_failures_suspect_not_transitioned() {
        let members = Arc::new(RwLock::new(HashMap::new()));

        // Add suspect member
        members.write().insert("suspect".to_string(), Member {
            node_id: NodeId::new("suspect"),
            gossip_addr: "127.0.0.1:4001".parse().unwrap(),
            transport_addr: "127.0.0.1:4002".parse().unwrap(),
            state: MemberState::Suspect,
            last_seen: Instant::now() - Duration::from_secs(60),
            incarnation: 1,
        });

        let actions = check_member_failures(&members, Duration::from_secs(30));

        // Suspect members are not marked as dead (only Alive -> Dead transition)
        assert!(actions.is_empty());
    }
}
