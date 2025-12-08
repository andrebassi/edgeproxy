//! DashMap Metrics Store
//!
//! Implements MetricsStore using DashMap for lock-free concurrent access.

use crate::domain::ports::MetricsStore;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

/// Metrics for a single backend.
#[derive(Debug)]
pub struct BackendMetrics {
    /// Current number of active connections
    pub current_conns: AtomicUsize,
    /// Last recorded round-trip time in milliseconds
    pub last_rtt_ms: AtomicU64,
}

impl BackendMetrics {
    fn new() -> Self {
        Self {
            current_conns: AtomicUsize::new(0),
            last_rtt_ms: AtomicU64::new(0),
        }
    }
}

impl Default for BackendMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// DashMap-backed metrics store.
///
/// Uses DashMap for lock-free concurrent access to metrics.
/// Each backend has its own metrics entry.
pub struct DashMapMetricsStore {
    metrics: DashMap<String, BackendMetrics>,
}

impl DashMapMetricsStore {
    /// Create a new metrics store.
    pub fn new() -> Self {
        Self {
            metrics: DashMap::new(),
        }
    }

    /// Get all backend IDs with metrics.
    #[allow(dead_code)]
    pub fn backend_ids(&self) -> Vec<String> {
        self.metrics.iter().map(|e| e.key().clone()).collect()
    }

    /// Get metrics for a specific backend (for debugging).
    #[allow(dead_code)]
    pub fn get_metrics(&self, backend_id: &str) -> Option<(usize, u64)> {
        self.metrics.get(backend_id).map(|m| {
            (
                m.current_conns.load(Ordering::Relaxed),
                m.last_rtt_ms.load(Ordering::Relaxed),
            )
        })
    }
}

impl Default for DashMapMetricsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MetricsStore for DashMapMetricsStore {
    fn get_connection_count(&self, backend_id: &str) -> usize {
        self.metrics
            .get(backend_id)
            .map(|m| m.current_conns.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn increment_connections(&self, backend_id: &str) {
        self.metrics
            .entry(backend_id.to_string())
            .or_default()
            .current_conns
            .fetch_add(1, Ordering::Relaxed);
    }

    fn decrement_connections(&self, backend_id: &str) {
        if let Some(m) = self.metrics.get(backend_id) {
            // Use compare_exchange loop to prevent underflow
            let mut current = m.current_conns.load(Ordering::Relaxed);
            while current > 0 {
                match m.current_conns.compare_exchange_weak(
                    current,
                    current - 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break,
                    Err(c) => current = c,
                }
            }
        }
    }

    fn record_rtt(&self, backend_id: &str, rtt_ms: u64) {
        self.metrics
            .entry(backend_id.to_string())
            .or_default()
            .last_rtt_ms
            .store(rtt_ms, Ordering::Relaxed);
    }

    fn get_last_rtt(&self, backend_id: &str) -> Option<u64> {
        self.metrics
            .get(backend_id)
            .map(|m| m.last_rtt_ms.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    // ===== Connection Count Tests =====

    #[test]
    fn test_connection_count_starts_at_zero() {
        let store = DashMapMetricsStore::new();
        assert_eq!(store.get_connection_count("backend-1"), 0);
    }

    #[test]
    fn test_connection_count_increment() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 1);

        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 2);

        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 3);
    }

    #[test]
    fn test_connection_count_decrement() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 2);

        store.decrement_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 1);

        store.decrement_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 0);
    }

    #[test]
    fn test_decrement_nonexistent_backend() {
        let store = DashMapMetricsStore::new();

        // Should not panic
        store.decrement_connections("nonexistent");
        assert_eq!(store.get_connection_count("nonexistent"), 0);
    }

    #[test]
    fn test_decrement_at_zero_saturates() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        store.decrement_connections("backend-1");
        // Decrement again at 0 should saturate at 0 (no underflow)
        store.decrement_connections("backend-1");

        assert_eq!(store.get_connection_count("backend-1"), 0);
    }

    // ===== RTT Recording Tests =====

    #[test]
    fn test_rtt_starts_as_none() {
        let store = DashMapMetricsStore::new();
        assert!(store.get_last_rtt("backend-1").is_none());
    }

    #[test]
    fn test_rtt_recording() {
        let store = DashMapMetricsStore::new();

        store.record_rtt("backend-1", 50);
        assert_eq!(store.get_last_rtt("backend-1"), Some(50));
    }

    #[test]
    fn test_rtt_overwrites_previous() {
        let store = DashMapMetricsStore::new();

        store.record_rtt("backend-1", 50);
        store.record_rtt("backend-1", 75);
        store.record_rtt("backend-1", 100);

        assert_eq!(store.get_last_rtt("backend-1"), Some(100));
    }

    #[test]
    fn test_rtt_zero_value() {
        let store = DashMapMetricsStore::new();

        store.record_rtt("backend-1", 0);
        // Note: get_last_rtt returns Some(0), not None
        // After recording, the entry exists
        assert_eq!(store.get_last_rtt("backend-1"), Some(0));
    }

    // ===== Multiple Backends Tests =====

    #[test]
    fn test_multiple_backends_isolated() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        store.increment_connections("backend-1");
        store.increment_connections("backend-2");

        assert_eq!(store.get_connection_count("backend-1"), 2);
        assert_eq!(store.get_connection_count("backend-2"), 1);
        assert_eq!(store.get_connection_count("backend-3"), 0);
    }

    #[test]
    fn test_multiple_backends_rtt_isolated() {
        let store = DashMapMetricsStore::new();

        store.record_rtt("backend-1", 50);
        store.record_rtt("backend-2", 100);

        assert_eq!(store.get_last_rtt("backend-1"), Some(50));
        assert_eq!(store.get_last_rtt("backend-2"), Some(100));
        assert!(store.get_last_rtt("backend-3").is_none());
    }

    // ===== Helper Methods Tests =====

    #[test]
    fn test_backend_ids_empty() {
        let store = DashMapMetricsStore::new();
        assert!(store.backend_ids().is_empty());
    }

    #[test]
    fn test_backend_ids_after_increment() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        store.increment_connections("backend-2");
        store.increment_connections("backend-3");

        let ids = store.backend_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"backend-1".to_string()));
        assert!(ids.contains(&"backend-2".to_string()));
        assert!(ids.contains(&"backend-3".to_string()));
    }

    #[test]
    fn test_get_metrics() {
        let store = DashMapMetricsStore::new();

        store.increment_connections("backend-1");
        store.increment_connections("backend-1");
        store.record_rtt("backend-1", 42);

        let metrics = store.get_metrics("backend-1");
        assert!(metrics.is_some());
        let (conns, rtt) = metrics.unwrap();
        assert_eq!(conns, 2);
        assert_eq!(rtt, 42);
    }

    #[test]
    fn test_get_metrics_nonexistent() {
        let store = DashMapMetricsStore::new();
        assert!(store.get_metrics("nonexistent").is_none());
    }

    // ===== Default Trait Tests =====

    #[test]
    fn test_default() {
        let store = DashMapMetricsStore::default();
        assert_eq!(store.get_connection_count("test"), 0);
    }

    // ===== Concurrency Safety Tests =====

    #[test]
    fn test_concurrent_increments() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(DashMapMetricsStore::new());
        let mut handles = vec![];

        for _ in 0..10 {
            let store = store.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    store.increment_connections("backend-1");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.get_connection_count("backend-1"), 1000);
    }

    #[test]
    fn test_concurrent_decrements() {
        use std::sync::Arc;
        use std::thread;

        let store = Arc::new(DashMapMetricsStore::new());

        // First increment 1000 times
        for _ in 0..1000 {
            store.increment_connections("backend-1");
        }

        let mut handles = vec![];

        // Then decrement concurrently
        for _ in 0..10 {
            let store = store.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    store.decrement_connections("backend-1");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.get_connection_count("backend-1"), 0);
    }

    #[test]
    fn test_backend_metrics_default() {
        let metrics = BackendMetrics::default();
        assert_eq!(metrics.current_conns.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.last_rtt_ms.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_backend_metrics_debug() {
        let metrics = BackendMetrics::new();
        let debug_str = format!("{:?}", metrics);
        assert!(debug_str.contains("current_conns"));
        assert!(debug_str.contains("last_rtt_ms"));
    }
}
