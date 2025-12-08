//! DashMap Binding Repository
//!
//! Implements BindingRepository using DashMap for lock-free concurrent access.

use crate::domain::entities::{Binding, ClientKey};
use crate::domain::ports::BindingRepository;
use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// DashMap-backed binding repository.
///
/// Uses DashMap for lock-free concurrent access to bindings.
/// Supports periodic garbage collection of expired bindings.
pub struct DashMapBindingRepository {
    bindings: Arc<DashMap<ClientKey, Binding>>,
}

impl DashMapBindingRepository {
    /// Create a new repository.
    pub fn new() -> Self {
        Self {
            bindings: Arc::new(DashMap::new()),
        }
    }

    /// Start the background garbage collection task.
    ///
    /// Removes bindings that have not been seen within the TTL.
    pub fn start_gc(&self, ttl: Duration, interval: Duration) {
        let bindings = self.bindings.clone();

        tokio::spawn(async move {
            loop {
                let now = Instant::now();
                let mut to_remove = Vec::new();

                for entry in bindings.iter() {
                    if now.duration_since(entry.value().last_seen) > ttl {
                        to_remove.push(entry.key().clone());
                    }
                }

                let removed_count = to_remove.len();
                for key in to_remove {
                    bindings.remove(&key);
                }

                if removed_count > 0 {
                    tracing::debug!("binding GC removed {} expired entries", removed_count);
                }

                tokio::time::sleep(interval).await;
            }
        });
    }

    /// Get the underlying DashMap (for advanced use cases).
    #[allow(dead_code)]
    pub fn inner(&self) -> &Arc<DashMap<ClientKey, Binding>> {
        &self.bindings
    }
}

impl Default for DashMapBindingRepository {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl BindingRepository for DashMapBindingRepository {
    async fn get(&self, key: &ClientKey) -> Option<Binding> {
        self.bindings.get(key).map(|e| e.value().clone())
    }

    async fn set(&self, key: ClientKey, binding: Binding) {
        self.bindings.insert(key, binding);
    }

    async fn remove(&self, key: &ClientKey) {
        self.bindings.remove(key);
    }

    async fn touch(&self, key: &ClientKey) {
        if let Some(mut entry) = self.bindings.get_mut(key) {
            entry.last_seen = Instant::now();
        }
    }

    async fn cleanup_expired(&self, ttl: Duration) -> usize {
        let now = Instant::now();
        let mut to_remove = Vec::new();

        for entry in self.bindings.iter() {
            if now.duration_since(entry.value().last_seen) > ttl {
                to_remove.push(entry.key().clone());
            }
        }

        let count = to_remove.len();
        for key in to_remove {
            self.bindings.remove(&key);
        }

        count
    }

    async fn count(&self) -> usize {
        self.bindings.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

    // ===== Set and Get Tests =====

    #[tokio::test]
    async fn test_set_and_get() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let binding = Binding::new("backend-1".to_string());

        repo.set(key.clone(), binding).await;

        let result = repo.get(&key).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().backend_id, "backend-1");
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        let result = repo.get(&key).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_set_overwrites() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        repo.set(key.clone(), Binding::new("backend-1".to_string()))
            .await;
        repo.set(key.clone(), Binding::new("backend-2".to_string()))
            .await;

        let result = repo.get(&key).await;
        assert_eq!(result.unwrap().backend_id, "backend-2");
    }

    #[tokio::test]
    async fn test_ipv6_key() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1)));
        let binding = Binding::new("backend-v6".to_string());

        repo.set(key.clone(), binding).await;

        let result = repo.get(&key).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().backend_id, "backend-v6");
    }

    // ===== Remove Tests =====

    #[tokio::test]
    async fn test_remove() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        repo.set(key.clone(), Binding::new("backend-1".to_string()))
            .await;
        assert!(repo.get(&key).await.is_some());

        repo.remove(&key).await;
        assert!(repo.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_remove_nonexistent_does_not_panic() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        // Should not panic
        repo.remove(&key).await;
    }

    // ===== Touch Tests =====

    #[tokio::test]
    async fn test_touch_updates_last_seen() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        let mut binding = Binding::new("backend-1".to_string());
        let old_last_seen = Instant::now() - Duration::from_secs(100);
        binding.last_seen = old_last_seen;

        repo.set(key.clone(), binding).await;

        // Touch should update last_seen
        repo.touch(&key).await;

        let result = repo.get(&key).await.unwrap();
        assert!(result.last_seen > old_last_seen);
    }

    #[tokio::test]
    async fn test_touch_nonexistent_does_not_panic() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        // Should not panic
        repo.touch(&key).await;
    }

    // ===== Cleanup Expired Tests =====

    #[tokio::test]
    async fn test_cleanup_expired() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        // Create a binding with an old last_seen
        let mut binding = Binding::new("backend-1".to_string());
        binding.last_seen = Instant::now() - Duration::from_secs(100);

        repo.set(key.clone(), binding).await;

        // Cleanup with 50 second TTL should remove the binding
        let removed = repo.cleanup_expired(Duration::from_secs(50)).await;
        assert_eq!(removed, 1);
        assert!(repo.get(&key).await.is_none());
    }

    #[tokio::test]
    async fn test_cleanup_keeps_fresh_bindings() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        // Fresh binding
        let binding = Binding::new("backend-1".to_string());
        repo.set(key.clone(), binding).await;

        // Cleanup with 100 second TTL should NOT remove the binding
        let removed = repo.cleanup_expired(Duration::from_secs(100)).await;
        assert_eq!(removed, 0);
        assert!(repo.get(&key).await.is_some());
    }

    #[tokio::test]
    async fn test_cleanup_mixed_bindings() {
        let repo = DashMapBindingRepository::new();

        // Old binding
        let key1 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let mut old_binding = Binding::new("backend-1".to_string());
        old_binding.last_seen = Instant::now() - Duration::from_secs(100);
        repo.set(key1.clone(), old_binding).await;

        // Fresh binding
        let key2 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)));
        let fresh_binding = Binding::new("backend-2".to_string());
        repo.set(key2.clone(), fresh_binding).await;

        // Cleanup with 50 second TTL
        let removed = repo.cleanup_expired(Duration::from_secs(50)).await;

        assert_eq!(removed, 1);
        assert!(repo.get(&key1).await.is_none()); // old removed
        assert!(repo.get(&key2).await.is_some()); // fresh kept
    }

    // ===== Count Tests =====

    #[tokio::test]
    async fn test_count_empty() {
        let repo = DashMapBindingRepository::new();
        assert_eq!(repo.count().await, 0);
    }

    #[tokio::test]
    async fn test_count_after_additions() {
        let repo = DashMapBindingRepository::new();

        for i in 0..5 {
            let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, i)));
            repo.set(key, Binding::new(format!("backend-{}", i))).await;
        }

        assert_eq!(repo.count().await, 5);
    }

    #[tokio::test]
    async fn test_count_after_removal() {
        let repo = DashMapBindingRepository::new();

        let key1 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));
        let key2 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 2)));

        repo.set(key1.clone(), Binding::new("backend-1".to_string()))
            .await;
        repo.set(key2.clone(), Binding::new("backend-2".to_string()))
            .await;
        assert_eq!(repo.count().await, 2);

        repo.remove(&key1).await;
        assert_eq!(repo.count().await, 1);
    }

    // ===== Inner Access Tests =====

    #[tokio::test]
    async fn test_inner_access() {
        let repo = DashMapBindingRepository::new();
        let key = ClientKey::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1)));

        repo.set(key.clone(), Binding::new("backend-1".to_string()))
            .await;

        // Access inner DashMap directly
        let inner = repo.inner();
        assert_eq!(inner.len(), 1);
    }

    // ===== Default Trait Tests =====

    #[tokio::test]
    async fn test_default() {
        let repo = DashMapBindingRepository::default();
        assert_eq!(repo.count().await, 0);
    }

    // ===== Multiple Clients Tests =====

    #[tokio::test]
    async fn test_multiple_clients_isolated() {
        let repo = DashMapBindingRepository::new();

        let key1 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1)));
        let key2 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 2)));
        let key3 = ClientKey::new(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 3)));

        repo.set(key1.clone(), Binding::new("backend-1".to_string()))
            .await;
        repo.set(key2.clone(), Binding::new("backend-2".to_string()))
            .await;
        repo.set(key3.clone(), Binding::new("backend-1".to_string()))
            .await;

        assert_eq!(repo.get(&key1).await.unwrap().backend_id, "backend-1");
        assert_eq!(repo.get(&key2).await.unwrap().backend_id, "backend-2");
        assert_eq!(repo.get(&key3).await.unwrap().backend_id, "backend-1");
    }
}
