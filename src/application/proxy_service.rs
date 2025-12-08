//! Proxy Service - Main application use case
//!
//! Orchestrates the proxy logic: resolving backends, managing bindings,
//! and recording metrics. This is the primary interface for the inbound adapter.

use crate::domain::entities::{Backend, Binding, ClientKey, GeoInfo};
use crate::domain::ports::{BackendRepository, BindingRepository, GeoResolver, MetricsStore};
use crate::domain::services::LoadBalancer;
use crate::domain::value_objects::RegionCode;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

/// Proxy service - main application use case.
///
/// This service orchestrates the proxy logic:
/// 1. Resolves the best backend for a client
/// 2. Manages client-to-backend bindings (session affinity)
/// 3. Records connection metrics
pub struct ProxyService {
    backend_repo: Arc<dyn BackendRepository>,
    binding_repo: Arc<dyn BindingRepository>,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    metrics: Arc<dyn MetricsStore>,
    local_region: RegionCode,
}

impl ProxyService {
    /// Create a new proxy service.
    pub fn new(
        backend_repo: Arc<dyn BackendRepository>,
        binding_repo: Arc<dyn BindingRepository>,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        metrics: Arc<dyn MetricsStore>,
        local_region: RegionCode,
    ) -> Self {
        Self {
            backend_repo,
            binding_repo,
            geo_resolver,
            metrics,
            local_region,
        }
    }

    /// Resolve the best backend for a client IP.
    ///
    /// This is the main entry point for routing decisions. It:
    /// 1. Checks for existing binding (session affinity)
    /// 2. If no binding, uses load balancer to select best backend
    /// 3. Creates a new binding for the client
    ///
    /// # Arguments
    /// * `client_ip` - The client's IP address
    ///
    /// # Returns
    /// The selected backend, or None if no backends are available
    #[allow(dead_code)]
    pub async fn resolve_backend(&self, client_ip: IpAddr) -> Option<Backend> {
        let client_key = ClientKey::new(client_ip);

        // 1. Check for existing binding
        if let Some(binding) = self.binding_repo.get(&client_key).await {
            // Update last_seen
            self.binding_repo.touch(&client_key).await;

            // Verify backend is still healthy
            if let Some(backend) = self.backend_repo.get_by_id(&binding.backend_id).await {
                if backend.healthy {
                    tracing::debug!(
                        "using existing binding for {} -> {}",
                        client_ip,
                        backend.id
                    );
                    return Some(backend);
                }
            }

            // Backend unhealthy or gone - remove stale binding
            self.binding_repo.remove(&client_key).await;
            tracing::debug!("removed stale binding for {}", client_ip);
        }

        // 2. Resolve client geo location
        let client_geo = self.resolve_geo(client_ip);

        // 3. Get healthy backends
        let backends = self.backend_repo.get_healthy().await;
        if backends.is_empty() {
            tracing::warn!("no healthy backends available");
            return None;
        }

        // 4. Use load balancer to pick best backend
        let metrics = self.metrics.clone();
        let backend = LoadBalancer::pick_backend(
            &backends,
            &self.local_region,
            client_geo.as_ref(),
            |id| metrics.get_connection_count(id),
        )?;

        // 5. Create binding for session affinity
        self.binding_repo
            .set(client_key, Binding::new(backend.id.clone()))
            .await;

        tracing::debug!(
            "new binding for {} -> {} (geo: {:?})",
            client_ip,
            backend.id,
            client_geo
        );

        Some(backend)
    }

    /// Resolve the best backend using a specific GeoInfo.
    ///
    /// This is useful when the caller has already resolved the geo info
    /// (e.g., from a public IP lookup for localhost connections).
    pub async fn resolve_backend_with_geo(
        &self,
        client_ip: IpAddr,
        client_geo: Option<GeoInfo>,
    ) -> Option<Backend> {
        let client_key = ClientKey::new(client_ip);

        // Check for existing binding first
        if let Some(binding) = self.binding_repo.get(&client_key).await {
            self.binding_repo.touch(&client_key).await;
            if let Some(backend) = self.backend_repo.get_by_id(&binding.backend_id).await {
                if backend.healthy {
                    return Some(backend);
                }
            }
            self.binding_repo.remove(&client_key).await;
        }

        // Get healthy backends
        let backends = self.backend_repo.get_healthy().await;
        if backends.is_empty() {
            return None;
        }

        // Use load balancer with provided geo
        let metrics = self.metrics.clone();
        let backend = LoadBalancer::pick_backend(
            &backends,
            &self.local_region,
            client_geo.as_ref(),
            |id| metrics.get_connection_count(id),
        )?;

        // Create binding
        self.binding_repo
            .set(client_key, Binding::new(backend.id.clone()))
            .await;

        Some(backend)
    }

    /// Clear the binding for a client.
    ///
    /// Useful when detecting VPN changes or other scenarios
    /// where the binding should be invalidated.
    pub async fn clear_binding(&self, client_ip: IpAddr) {
        let client_key = ClientKey::new(client_ip);
        self.binding_repo.remove(&client_key).await;
    }

    /// Resolve geographic information for an IP address.
    pub fn resolve_geo(&self, ip: IpAddr) -> Option<GeoInfo> {
        self.geo_resolver.as_ref().and_then(|g| g.resolve(ip))
    }

    /// Record the start of a connection to a backend.
    pub fn record_connection_start(&self, backend_id: &str) {
        self.metrics.increment_connections(backend_id);
    }

    /// Record the end of a connection to a backend.
    pub fn record_connection_end(&self, backend_id: &str) {
        self.metrics.decrement_connections(backend_id);
    }

    /// Record the round-trip time for connecting to a backend.
    pub fn record_rtt(&self, backend_id: &str, rtt_ms: u64) {
        self.metrics.record_rtt(backend_id, rtt_ms);
    }

    /// Get the current connection count for a backend.
    #[allow(dead_code)]
    pub fn get_connection_count(&self, backend_id: &str) -> usize {
        self.metrics.get_connection_count(backend_id)
    }

    /// Get the local region for this proxy.
    #[allow(dead_code)]
    pub fn local_region(&self) -> &RegionCode {
        &self.local_region
    }

    /// Run periodic cleanup of expired bindings.
    #[allow(dead_code)]
    pub async fn run_binding_cleanup(&self, ttl: Duration, interval: Duration) {
        loop {
            let removed = self.binding_repo.cleanup_expired(ttl).await;
            if removed > 0 {
                tracing::debug!("cleaned up {} expired bindings", removed);
            }
            tokio::time::sleep(interval).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::entities::Backend;
    use async_trait::async_trait;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use std::time::Instant;

    // ===== Mock Implementations =====

    struct MockBackendRepo {
        backends: Vec<Backend>,
    }

    #[async_trait]
    impl BackendRepository for MockBackendRepo {
        async fn get_all(&self) -> Vec<Backend> {
            self.backends.clone()
        }

        async fn get_by_id(&self, id: &str) -> Option<Backend> {
            self.backends.iter().find(|b| b.id == id).cloned()
        }

        async fn get_healthy(&self) -> Vec<Backend> {
            self.backends.iter().filter(|b| b.healthy).cloned().collect()
        }

        async fn get_version(&self) -> u64 {
            1
        }
    }

    struct MockBindingRepo {
        bindings: Mutex<HashMap<IpAddr, Binding>>,
    }

    impl MockBindingRepo {
        fn new() -> Self {
            Self {
                bindings: Mutex::new(HashMap::new()),
            }
        }
    }

    #[async_trait]
    impl BindingRepository for MockBindingRepo {
        async fn get(&self, key: &ClientKey) -> Option<Binding> {
            self.bindings.lock().unwrap().get(&key.client_ip).cloned()
        }

        async fn set(&self, key: ClientKey, binding: Binding) {
            self.bindings.lock().unwrap().insert(key.client_ip, binding);
        }

        async fn remove(&self, key: &ClientKey) {
            self.bindings.lock().unwrap().remove(&key.client_ip);
        }

        async fn touch(&self, key: &ClientKey) {
            if let Some(b) = self.bindings.lock().unwrap().get_mut(&key.client_ip) {
                b.last_seen = Instant::now();
            }
        }

        async fn cleanup_expired(&self, ttl: Duration) -> usize {
            let now = Instant::now();
            let mut bindings = self.bindings.lock().unwrap();
            let before = bindings.len();
            bindings.retain(|_, b| now.duration_since(b.last_seen) <= ttl);
            before - bindings.len()
        }

        async fn count(&self) -> usize {
            self.bindings.lock().unwrap().len()
        }
    }

    struct MockMetrics {
        counts: Mutex<HashMap<String, usize>>,
        rtts: Mutex<HashMap<String, u64>>,
    }

    impl MockMetrics {
        fn new() -> Self {
            Self {
                counts: Mutex::new(HashMap::new()),
                rtts: Mutex::new(HashMap::new()),
            }
        }
    }

    impl MetricsStore for MockMetrics {
        fn get_connection_count(&self, backend_id: &str) -> usize {
            *self.counts.lock().unwrap().get(backend_id).unwrap_or(&0)
        }

        fn increment_connections(&self, backend_id: &str) {
            *self
                .counts
                .lock()
                .unwrap()
                .entry(backend_id.to_string())
                .or_insert(0) += 1;
        }

        fn decrement_connections(&self, backend_id: &str) {
            if let Some(count) = self.counts.lock().unwrap().get_mut(backend_id) {
                *count = count.saturating_sub(1);
            }
        }

        fn record_rtt(&self, backend_id: &str, rtt_ms: u64) {
            self.rtts
                .lock()
                .unwrap()
                .insert(backend_id.to_string(), rtt_ms);
        }

        fn get_last_rtt(&self, backend_id: &str) -> Option<u64> {
            self.rtts.lock().unwrap().get(backend_id).copied()
        }
    }

    struct MockGeoResolver {
        geo_map: HashMap<IpAddr, GeoInfo>,
    }

    impl MockGeoResolver {
        fn new() -> Self {
            Self {
                geo_map: HashMap::new(),
            }
        }

        fn with_geo(mut self, ip: IpAddr, country: &str, region: RegionCode) -> Self {
            self.geo_map
                .insert(ip, GeoInfo::new(country.to_string(), region));
            self
        }
    }

    impl GeoResolver for MockGeoResolver {
        fn resolve(&self, ip: IpAddr) -> Option<GeoInfo> {
            self.geo_map.get(&ip).cloned()
        }
    }

    // ===== Test Helpers =====

    fn create_test_backend(id: &str, region: &str, country: &str) -> Backend {
        Backend {
            id: id.to_string(),
            app: "test".to_string(),
            region: RegionCode::from_str(region),
            country: country.to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 1,
            soft_limit: 100,
            hard_limit: 200,
        }
    }

    fn create_unhealthy_backend(id: &str, region: &str, country: &str) -> Backend {
        let mut backend = create_test_backend(id, region, country);
        backend.healthy = false;
        backend
    }

    // ===== resolve_backend Tests =====

    #[tokio::test]
    async fn test_resolve_backend_creates_binding() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let binding_repo = Arc::new(MockBindingRepo::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo.clone(),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let result = service.resolve_backend(client_ip).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-1");

        // Verify binding was created
        let binding = binding_repo.get(&ClientKey::new(client_ip)).await;
        assert!(binding.is_some());
        assert_eq!(binding.unwrap().backend_id, "br-1");
    }

    #[tokio::test]
    async fn test_resolve_backend_uses_existing_binding() {
        let backends = vec![
            create_test_backend("br-1", "sa", "BR"),
            create_test_backend("br-2", "sa", "BR"),
        ];

        let binding_repo = Arc::new(MockBindingRepo::new());
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Pre-create a binding to br-2
        binding_repo
            .set(
                ClientKey::new(client_ip),
                Binding::new("br-2".to_string()),
            )
            .await;

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo,
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let result = service.resolve_backend(client_ip).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-2");
    }

    #[tokio::test]
    async fn test_resolve_backend_removes_stale_binding_for_unhealthy() {
        let backends = vec![
            create_unhealthy_backend("br-1", "sa", "BR"), // unhealthy
            create_test_backend("br-2", "sa", "BR"),
        ];

        let binding_repo = Arc::new(MockBindingRepo::new());
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Pre-create a binding to unhealthy br-1
        binding_repo
            .set(
                ClientKey::new(client_ip),
                Binding::new("br-1".to_string()),
            )
            .await;

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo.clone(),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let result = service.resolve_backend(client_ip).await;

        // Should pick healthy br-2
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-2");

        // Binding should now point to br-2
        let binding = binding_repo.get(&ClientKey::new(client_ip)).await;
        assert_eq!(binding.unwrap().backend_id, "br-2");
    }

    #[tokio::test]
    async fn test_resolve_backend_no_healthy_backends() {
        let backends = vec![
            create_unhealthy_backend("br-1", "sa", "BR"),
            create_unhealthy_backend("br-2", "sa", "BR"),
        ];

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let result = service.resolve_backend(client_ip).await;

        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_resolve_backend_empty_backends() {
        let backends: Vec<Backend> = vec![];

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let result = service.resolve_backend(client_ip).await;

        assert!(result.is_none());
    }

    // ===== resolve_backend_with_geo Tests =====

    #[tokio::test]
    async fn test_resolve_backend_with_geo() {
        let backends = vec![
            create_test_backend("br-1", "sa", "BR"),
            create_test_backend("us-1", "us", "US"),
        ];

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::NorthAmerica,
        );

        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let client_geo = Some(GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica));

        let result = service.resolve_backend_with_geo(client_ip, client_geo).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-1");
    }

    #[tokio::test]
    async fn test_resolve_backend_with_geo_respects_existing_binding() {
        let backends = vec![
            create_test_backend("br-1", "sa", "BR"),
            create_test_backend("us-1", "us", "US"),
        ];

        let binding_repo = Arc::new(MockBindingRepo::new());
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Pre-create binding to us-1
        binding_repo
            .set(
                ClientKey::new(client_ip),
                Binding::new("us-1".to_string()),
            )
            .await;

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo,
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::NorthAmerica,
        );

        // Even with BR geo, should use existing binding
        let client_geo = Some(GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica));
        let result = service.resolve_backend_with_geo(client_ip, client_geo).await;

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "us-1");
    }

    // ===== clear_binding Tests =====

    #[tokio::test]
    async fn test_clear_binding() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let binding_repo = Arc::new(MockBindingRepo::new());
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Create binding
        binding_repo
            .set(
                ClientKey::new(client_ip),
                Binding::new("br-1".to_string()),
            )
            .await;

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo.clone(),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        // Clear binding
        service.clear_binding(client_ip).await;

        // Verify binding is gone
        let binding = binding_repo.get(&ClientKey::new(client_ip)).await;
        assert!(binding.is_none());
    }

    // ===== resolve_geo Tests =====

    #[tokio::test]
    async fn test_resolve_geo_with_resolver() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        let geo_resolver = MockGeoResolver::new().with_geo(
            client_ip,
            "FR",
            RegionCode::Europe,
        );

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            Some(Arc::new(geo_resolver)),
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let geo = service.resolve_geo(client_ip);
        assert!(geo.is_some());
        assert_eq!(geo.unwrap().country, "FR");
    }

    #[tokio::test]
    async fn test_resolve_geo_without_resolver() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None, // No geo resolver
            Arc::new(MockMetrics::new()),
            RegionCode::SouthAmerica,
        );

        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();
        let geo = service.resolve_geo(client_ip);
        assert!(geo.is_none());
    }

    // ===== Metrics Recording Tests =====

    #[tokio::test]
    async fn test_record_connection_start() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let metrics = Arc::new(MockMetrics::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            metrics.clone(),
            RegionCode::SouthAmerica,
        );

        service.record_connection_start("br-1");
        service.record_connection_start("br-1");

        assert_eq!(metrics.get_connection_count("br-1"), 2);
    }

    #[tokio::test]
    async fn test_record_connection_end() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let metrics = Arc::new(MockMetrics::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            metrics.clone(),
            RegionCode::SouthAmerica,
        );

        service.record_connection_start("br-1");
        service.record_connection_start("br-1");
        service.record_connection_end("br-1");

        assert_eq!(metrics.get_connection_count("br-1"), 1);
    }

    #[tokio::test]
    async fn test_record_rtt() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let metrics = Arc::new(MockMetrics::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            metrics.clone(),
            RegionCode::SouthAmerica,
        );

        service.record_rtt("br-1", 42);

        assert_eq!(metrics.get_last_rtt("br-1"), Some(42));
    }

    // ===== get_connection_count Tests =====

    #[tokio::test]
    async fn test_get_connection_count() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];
        let metrics = Arc::new(MockMetrics::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            metrics.clone(),
            RegionCode::SouthAmerica,
        );

        metrics.increment_connections("br-1");
        metrics.increment_connections("br-1");

        assert_eq!(service.get_connection_count("br-1"), 2);
        assert_eq!(service.get_connection_count("nonexistent"), 0);
    }

    // ===== local_region Tests =====

    #[tokio::test]
    async fn test_local_region() {
        let backends = vec![create_test_backend("br-1", "sa", "BR")];

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            Arc::new(MockBindingRepo::new()),
            None,
            Arc::new(MockMetrics::new()),
            RegionCode::Europe,
        );

        assert_eq!(*service.local_region(), RegionCode::Europe);
    }

    // ===== Multiple Clients Tests =====

    #[tokio::test]
    async fn test_multiple_clients_different_backends() {
        let backends = vec![
            create_test_backend("br-1", "sa", "BR"),
            create_test_backend("us-1", "us", "US"),
        ];

        let client_ip_br: IpAddr = "192.168.1.1".parse().unwrap();
        let client_ip_us: IpAddr = "192.168.1.2".parse().unwrap();

        let geo_resolver = MockGeoResolver::new()
            .with_geo(client_ip_br, "BR", RegionCode::SouthAmerica)
            .with_geo(client_ip_us, "US", RegionCode::NorthAmerica);

        let binding_repo = Arc::new(MockBindingRepo::new());

        let service = ProxyService::new(
            Arc::new(MockBackendRepo { backends }),
            binding_repo.clone(),
            Some(Arc::new(geo_resolver)),
            Arc::new(MockMetrics::new()),
            RegionCode::Europe,
        );

        // Client from BR should get BR backend
        let result_br = service.resolve_backend(client_ip_br).await;
        assert_eq!(result_br.unwrap().id, "br-1");

        // Client from US should get US backend
        let result_us = service.resolve_backend(client_ip_us).await;
        assert_eq!(result_us.unwrap().id, "us-1");

        // Verify separate bindings
        let binding_br = binding_repo.get(&ClientKey::new(client_ip_br)).await;
        let binding_us = binding_repo.get(&ClientKey::new(client_ip_us)).await;
        assert_eq!(binding_br.unwrap().backend_id, "br-1");
        assert_eq!(binding_us.unwrap().backend_id, "us-1");
    }
}
