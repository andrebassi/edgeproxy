use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::time::Instant;

/// Backend conhecido pelo edgeProxy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    pub id: String,
    pub app: String,
    pub region: String,    // "sa", "us", "eu", "ap"
    pub country: String,   // "BR", "FR", "US", etc (ISO 3166-1 alpha-2)
    pub wg_ip: String,     // IP WireGuard
    pub port: u16,
    pub healthy: bool,
    pub weight: u8,        // peso relativo
    pub soft_limit: u32,   // conexões "confortáveis"
    pub hard_limit: u32,   // máximo de conexões
}

/// Snapshot em memória
#[derive(Debug, Clone, Default)]
pub struct RoutingState {
    pub version: u64,
    pub backends: Vec<Backend>,
}

/// Afinidade por IP
#[derive(Debug, Clone, Eq)]
pub struct ClientKey {
    pub client_ip: IpAddr,
}

impl PartialEq for ClientKey {
    fn eq(&self, other: &Self) -> bool {
        self.client_ip == other.client_ip
    }
}

impl Hash for ClientKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.client_ip.hash(state);
    }
}

/// Binding local client_ip -> backend_id
#[derive(Debug, Clone)]
pub struct Binding {
    pub backend_id: String,
    #[allow(dead_code)]
    pub created_at: Instant,
    pub last_seen: Instant,
}
