//! Domain Entities - Core business objects
//!
//! These entities represent the core concepts of the edgeProxy domain.
//! They have no external dependencies and contain only business logic.

use crate::domain::value_objects::RegionCode;
use serde::{Deserialize, Serialize};
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::time::Instant;

/// A backend server that can receive proxied connections.
///
/// Backends are distributed across regions and have capacity limits.
/// The load balancer uses this information to route clients optimally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Backend {
    /// Unique identifier for this backend
    pub id: String,
    /// Application name this backend serves
    pub app: String,
    /// Geographic region code (sa, us, eu, ap)
    pub region: RegionCode,
    /// Country code (ISO 3166-1 alpha-2: BR, US, FR, etc)
    pub country: String,
    /// WireGuard overlay IP address
    pub wg_ip: String,
    /// Port number for the backend service
    pub port: u16,
    /// Whether this backend is currently healthy
    pub healthy: bool,
    /// Relative weight for load balancing (higher = preferred)
    pub weight: u8,
    /// Comfortable number of connections
    pub soft_limit: u32,
    /// Maximum number of connections (hard cap)
    pub hard_limit: u32,
}

/// Client-to-backend binding for session affinity.
///
/// Once a client is assigned to a backend, subsequent connections
/// from the same IP will be routed to the same backend until the
/// binding expires (TTL-based).
#[derive(Debug, Clone)]
pub struct Binding {
    /// ID of the backend this client is bound to
    pub backend_id: String,
    /// When the binding was created
    #[allow(dead_code)]
    pub created_at: Instant,
    /// Last time this binding was used
    pub last_seen: Instant,
}

impl Binding {
    /// Create a new binding
    pub fn new(backend_id: String) -> Self {
        let now = Instant::now();
        Self {
            backend_id,
            created_at: now,
            last_seen: now,
        }
    }

    /// Touch the binding to update last_seen
    #[allow(dead_code)]
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }
}

/// Key for identifying clients for session affinity.
///
/// Currently uses only the client IP address.
#[derive(Debug, Clone, Eq)]
pub struct ClientKey {
    /// Client's IP address
    pub client_ip: IpAddr,
}

impl ClientKey {
    pub fn new(client_ip: IpAddr) -> Self {
        Self { client_ip }
    }
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

/// Geographic information resolved from an IP address.
#[derive(Debug, Clone)]
pub struct GeoInfo {
    /// Country code (ISO 3166-1 alpha-2)
    pub country: String,
    /// Region code (sa, us, eu, ap)
    pub region: RegionCode,
}

impl GeoInfo {
    pub fn new(country: String, region: RegionCode) -> Self {
        Self { country, region }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::net::{Ipv4Addr, Ipv6Addr};

    // ===== ClientKey Tests =====

    #[test]
    fn test_client_key_equality_ipv4() {
        let key1 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let key2 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let key3 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)));

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_client_key_equality_ipv6() {
        let key1 = ClientKey::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
        let key2 = ClientKey::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
        let key3 = ClientKey::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 2)));

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_client_key_hash_consistency() {
        let key1 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let key2 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));

        let mut set = HashSet::new();
        set.insert(key1.clone());

        assert!(set.contains(&key2));
    }

    #[test]
    fn test_client_key_ipv4_vs_ipv6() {
        let ipv4 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)));
        let ipv6 = ClientKey::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));

        assert_ne!(ipv4, ipv6);
    }

    // ===== Binding Tests =====

    #[test]
    fn test_binding_new() {
        let binding = Binding::new("backend-1".to_string());

        assert_eq!(binding.backend_id, "backend-1");
        assert!(binding.created_at <= Instant::now());
        assert!(binding.last_seen <= Instant::now());
    }

    #[test]
    fn test_binding_created_at_equals_last_seen_on_new() {
        let binding = Binding::new("backend-1".to_string());

        // created_at and last_seen should be the same initially
        assert_eq!(binding.created_at, binding.last_seen);
    }

    #[test]
    fn test_binding_touch_updates_last_seen() {
        let mut binding = Binding::new("backend-1".to_string());
        let initial_seen = binding.last_seen;

        std::thread::sleep(std::time::Duration::from_millis(10));
        binding.touch();

        assert!(binding.last_seen > initial_seen);
    }

    #[test]
    fn test_binding_touch_does_not_update_created_at() {
        let mut binding = Binding::new("backend-1".to_string());
        let initial_created = binding.created_at;

        std::thread::sleep(std::time::Duration::from_millis(10));
        binding.touch();

        assert_eq!(binding.created_at, initial_created);
    }

    // ===== GeoInfo Tests =====

    #[test]
    fn test_geo_info_new() {
        let geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        assert_eq!(geo.country, "BR");
        assert_eq!(geo.region, RegionCode::SouthAmerica);
    }

    #[test]
    fn test_geo_info_different_regions() {
        let tests = vec![
            ("US", RegionCode::NorthAmerica),
            ("FR", RegionCode::Europe),
            ("JP", RegionCode::AsiaPacific),
            ("BR", RegionCode::SouthAmerica),
        ];

        for (country, expected_region) in tests {
            let geo = GeoInfo::new(country.to_string(), expected_region.clone());
            assert_eq!(geo.country, country);
            assert_eq!(geo.region, expected_region);
        }
    }

    // ===== Backend Tests =====

    #[test]
    fn test_backend_struct_fields() {
        let backend = Backend {
            id: "fly-gru-1".to_string(),
            app: "myapp".to_string(),
            region: RegionCode::SouthAmerica,
            country: "BR".to_string(),
            wg_ip: "10.50.1.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 5,
            soft_limit: 100,
            hard_limit: 200,
        };

        assert_eq!(backend.id, "fly-gru-1");
        assert_eq!(backend.app, "myapp");
        assert_eq!(backend.region, RegionCode::SouthAmerica);
        assert_eq!(backend.country, "BR");
        assert_eq!(backend.wg_ip, "10.50.1.1");
        assert_eq!(backend.port, 8080);
        assert!(backend.healthy);
        assert_eq!(backend.weight, 5);
        assert_eq!(backend.soft_limit, 100);
        assert_eq!(backend.hard_limit, 200);
    }

    #[test]
    fn test_backend_clone() {
        let backend = Backend {
            id: "test-1".to_string(),
            app: "app".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 9000,
            healthy: false,
            weight: 1,
            soft_limit: 50,
            hard_limit: 100,
        };

        let cloned = backend.clone();

        assert_eq!(cloned.id, backend.id);
        assert_eq!(cloned.healthy, backend.healthy);
    }
}
