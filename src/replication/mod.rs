//! Built-in Replication Module
//!
//! This module provides distributed SQLite replication inspired by Corrosion.
//! Instead of running Corrosion as a separate sidecar, this module embeds
//! the replication logic directly into edgeProxy.
//!
//! ## Architecture
//!
//! - **Gossip (SWIM)**: Cluster membership and failure detection using Foca
//! - **Transport (QUIC)**: Secure peer-to-peer communication using Quinn
//! - **Sync**: Change detection and application using LWW (Last-Write-Wins)
//! - **Agent**: Orchestrates all components
//!
//! ## How It Works
//!
//! 1. Nodes discover each other via gossip protocol on port 4001
//! 2. When a backend is added/updated/deleted, the change is broadcast
//! 3. Changes are serialized with timestamps for conflict resolution (LWW)
//! 4. QUIC streams handle reliable delivery with retransmission
//!
//! ## Usage
//!
//! ```rust,ignore
//! use edgeproxy::replication::{ReplicationAgent, ReplicationConfig};
//!
//! let config = ReplicationConfig {
//!     node_id: "pop-sa-1".to_string(),
//!     gossip_addr: "0.0.0.0:4001".parse().unwrap(),
//!     bootstrap_peers: vec!["pop-us.example.com:4001".to_string()],
//!     db_path: "/var/lib/edgeproxy/state.db".to_string(),
//! };
//!
//! let agent = ReplicationAgent::new(config).await?;
//! agent.start().await?;
//! ```

mod config;
mod types;
mod gossip;
mod sync;
mod transport;
mod agent;

pub use config::ReplicationConfig;
pub use types::{Change, ChangeKind, ChangeSet, NodeId};
pub use gossip::{GossipService, Member, MemberState};
pub use sync::{SyncService, VersionVector};
pub use transport::{TransportService, PeerConnection};
pub use agent::ReplicationAgent;
