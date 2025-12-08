//! Prometheus Metrics Store
//!
//! Implements MetricsStore with Prometheus metrics exposition.

use crate::domain::ports::MetricsStore;
use dashmap::DashMap;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::Arc;

/// Aggregated metrics for Prometheus export.
#[derive(Debug, Default)]
pub struct AggregatedMetrics {
    /// Total connections established
    pub total_connections: AtomicU64,
    /// Total bytes proxied (client to backend)
    pub bytes_sent: AtomicU64,
    /// Total bytes proxied (backend to client)
    pub bytes_received: AtomicU64,
    /// Total connection errors
    pub connection_errors: AtomicU64,
}

/// Per-backend metrics.
#[derive(Debug)]
pub struct BackendMetrics {
    /// Current active connections
    pub active_connections: AtomicUsize,
    /// Total connections to this backend
    pub total_connections: AtomicU64,
    /// Last RTT in milliseconds
    pub last_rtt_ms: AtomicU64,
    /// Sum of all RTT measurements (for average calculation)
    pub rtt_sum_ms: AtomicU64,
    /// Number of RTT measurements
    pub rtt_count: AtomicU64,
    /// Connection errors to this backend
    pub connection_errors: AtomicU64,
}

impl BackendMetrics {
    fn new() -> Self {
        Self {
            active_connections: AtomicUsize::new(0),
            total_connections: AtomicU64::new(0),
            last_rtt_ms: AtomicU64::new(0),
            rtt_sum_ms: AtomicU64::new(0),
            rtt_count: AtomicU64::new(0),
            connection_errors: AtomicU64::new(0),
        }
    }

    /// Get average RTT in milliseconds.
    pub fn avg_rtt_ms(&self) -> f64 {
        let count = self.rtt_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let sum = self.rtt_sum_ms.load(Ordering::Relaxed);
        sum as f64 / count as f64
    }
}

impl Default for BackendMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Prometheus-compatible metrics store.
///
/// Stores metrics in a format suitable for Prometheus scraping.
pub struct PrometheusMetricsStore {
    /// Per-backend metrics
    backends: DashMap<String, Arc<BackendMetrics>>,
    /// Global aggregated metrics
    global: Arc<AggregatedMetrics>,
    /// Region label for metrics
    region: String,
}

impl PrometheusMetricsStore {
    /// Create a new Prometheus metrics store.
    pub fn new(region: String) -> Self {
        Self {
            backends: DashMap::new(),
            global: Arc::new(AggregatedMetrics::default()),
            region,
        }
    }

    /// Get or create backend metrics.
    fn get_or_create(&self, backend_id: &str) -> Arc<BackendMetrics> {
        self.backends
            .entry(backend_id.to_string())
            .or_insert_with(|| Arc::new(BackendMetrics::new()))
            .clone()
    }

    /// Record a connection error.
    pub fn record_error(&self, backend_id: &str) {
        let metrics = self.get_or_create(backend_id);
        metrics.connection_errors.fetch_add(1, Ordering::Relaxed);
        self.global.connection_errors.fetch_add(1, Ordering::Relaxed);
    }

    /// Record bytes transferred.
    pub fn record_bytes(&self, sent: u64, received: u64) {
        self.global.bytes_sent.fetch_add(sent, Ordering::Relaxed);
        self.global
            .bytes_received
            .fetch_add(received, Ordering::Relaxed);
    }

    /// Get all backend IDs.
    pub fn backend_ids(&self) -> Vec<String> {
        self.backends.iter().map(|e| e.key().clone()).collect()
    }

    /// Get metrics for a specific backend.
    pub fn get_backend_metrics(&self, backend_id: &str) -> Option<Arc<BackendMetrics>> {
        self.backends.get(backend_id).map(|e| e.clone())
    }

    /// Get global metrics.
    pub fn global_metrics(&self) -> &AggregatedMetrics {
        &self.global
    }

    /// Export metrics in Prometheus text format.
    pub fn export_prometheus(&self) -> String {
        let mut output = String::new();

        // Global metrics
        output.push_str("# HELP edgeproxy_connections_total Total connections established\n");
        output.push_str("# TYPE edgeproxy_connections_total counter\n");
        output.push_str(&format!(
            "edgeproxy_connections_total{{region=\"{}\"}} {}\n",
            self.region,
            self.global.total_connections.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_bytes_sent_total Total bytes sent to backends\n");
        output.push_str("# TYPE edgeproxy_bytes_sent_total counter\n");
        output.push_str(&format!(
            "edgeproxy_bytes_sent_total{{region=\"{}\"}} {}\n",
            self.region,
            self.global.bytes_sent.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_bytes_received_total Total bytes received from backends\n");
        output.push_str("# TYPE edgeproxy_bytes_received_total counter\n");
        output.push_str(&format!(
            "edgeproxy_bytes_received_total{{region=\"{}\"}} {}\n",
            self.region,
            self.global.bytes_received.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_connection_errors_total Total connection errors\n");
        output.push_str("# TYPE edgeproxy_connection_errors_total counter\n");
        output.push_str(&format!(
            "edgeproxy_connection_errors_total{{region=\"{}\"}} {}\n",
            self.region,
            self.global.connection_errors.load(Ordering::Relaxed)
        ));

        // Per-backend metrics
        output.push_str("# HELP edgeproxy_backend_connections_active Current active connections per backend\n");
        output.push_str("# TYPE edgeproxy_backend_connections_active gauge\n");

        output.push_str("# HELP edgeproxy_backend_connections_total Total connections per backend\n");
        output.push_str("# TYPE edgeproxy_backend_connections_total counter\n");

        output.push_str("# HELP edgeproxy_backend_rtt_ms Last RTT to backend in milliseconds\n");
        output.push_str("# TYPE edgeproxy_backend_rtt_ms gauge\n");

        output.push_str("# HELP edgeproxy_backend_rtt_avg_ms Average RTT to backend in milliseconds\n");
        output.push_str("# TYPE edgeproxy_backend_rtt_avg_ms gauge\n");

        output.push_str("# HELP edgeproxy_backend_errors_total Total errors per backend\n");
        output.push_str("# TYPE edgeproxy_backend_errors_total counter\n");

        for entry in self.backends.iter() {
            let backend_id = entry.key();
            let metrics = entry.value();

            output.push_str(&format!(
                "edgeproxy_backend_connections_active{{region=\"{}\",backend=\"{}\"}} {}\n",
                self.region,
                backend_id,
                metrics.active_connections.load(Ordering::Relaxed)
            ));

            output.push_str(&format!(
                "edgeproxy_backend_connections_total{{region=\"{}\",backend=\"{}\"}} {}\n",
                self.region,
                backend_id,
                metrics.total_connections.load(Ordering::Relaxed)
            ));

            output.push_str(&format!(
                "edgeproxy_backend_rtt_ms{{region=\"{}\",backend=\"{}\"}} {}\n",
                self.region,
                backend_id,
                metrics.last_rtt_ms.load(Ordering::Relaxed)
            ));

            output.push_str(&format!(
                "edgeproxy_backend_rtt_avg_ms{{region=\"{}\",backend=\"{}\"}} {:.2}\n",
                self.region,
                backend_id,
                metrics.avg_rtt_ms()
            ));

            output.push_str(&format!(
                "edgeproxy_backend_errors_total{{region=\"{}\",backend=\"{}\"}} {}\n",
                self.region,
                backend_id,
                metrics.connection_errors.load(Ordering::Relaxed)
            ));
        }

        output
    }
}

impl Default for PrometheusMetricsStore {
    fn default() -> Self {
        Self::new("unknown".to_string())
    }
}

impl MetricsStore for PrometheusMetricsStore {
    fn get_connection_count(&self, backend_id: &str) -> usize {
        self.backends
            .get(backend_id)
            .map(|m| m.active_connections.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    fn increment_connections(&self, backend_id: &str) {
        let metrics = self.get_or_create(backend_id);
        metrics.active_connections.fetch_add(1, Ordering::Relaxed);
        metrics.total_connections.fetch_add(1, Ordering::Relaxed);
        self.global.total_connections.fetch_add(1, Ordering::Relaxed);
    }

    fn decrement_connections(&self, backend_id: &str) {
        if let Some(m) = self.backends.get(backend_id) {
            let mut current = m.active_connections.load(Ordering::Relaxed);
            while current > 0 {
                match m.active_connections.compare_exchange_weak(
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
        let metrics = self.get_or_create(backend_id);
        metrics.last_rtt_ms.store(rtt_ms, Ordering::Relaxed);
        metrics.rtt_sum_ms.fetch_add(rtt_ms, Ordering::Relaxed);
        metrics.rtt_count.fetch_add(1, Ordering::Relaxed);
    }

    fn get_last_rtt(&self, backend_id: &str) -> Option<u64> {
        self.backends
            .get(backend_id)
            .map(|m| m.last_rtt_ms.load(Ordering::Relaxed))
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_new() {
        let store = PrometheusMetricsStore::new("eu".to_string());
        assert_eq!(store.region, "eu");
        assert!(store.backend_ids().is_empty());
    }

    #[test]
    fn test_default() {
        let store = PrometheusMetricsStore::default();
        assert_eq!(store.region, "unknown");
    }

    #[test]
    fn test_connection_tracking() {
        let store = PrometheusMetricsStore::new("us".to_string());

        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 1);

        store.increment_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 2);

        store.decrement_connections("backend-1");
        assert_eq!(store.get_connection_count("backend-1"), 1);
    }

    #[test]
    fn test_total_connections() {
        let store = PrometheusMetricsStore::new("sa".to_string());

        store.increment_connections("b1");
        store.increment_connections("b2");
        store.increment_connections("b1");

        assert_eq!(store.global.total_connections.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_rtt_recording() {
        let store = PrometheusMetricsStore::new("ap".to_string());

        store.record_rtt("backend-1", 50);
        assert_eq!(store.get_last_rtt("backend-1"), Some(50));

        store.record_rtt("backend-1", 100);
        assert_eq!(store.get_last_rtt("backend-1"), Some(100));
    }

    #[test]
    fn test_avg_rtt() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.record_rtt("backend-1", 50);
        store.record_rtt("backend-1", 100);
        store.record_rtt("backend-1", 150);

        let metrics = store.get_backend_metrics("backend-1").unwrap();
        assert!((metrics.avg_rtt_ms() - 100.0).abs() < 0.01);
    }

    #[test]
    fn test_avg_rtt_zero_count() {
        let metrics = BackendMetrics::new();
        assert_eq!(metrics.avg_rtt_ms(), 0.0);
    }

    #[test]
    fn test_error_recording() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.record_error("backend-1");
        store.record_error("backend-1");
        store.record_error("backend-2");

        assert_eq!(store.global.connection_errors.load(Ordering::Relaxed), 3);
        assert_eq!(
            store
                .get_backend_metrics("backend-1")
                .unwrap()
                .connection_errors
                .load(Ordering::Relaxed),
            2
        );
    }

    #[test]
    fn test_bytes_recording() {
        let store = PrometheusMetricsStore::new("us".to_string());

        store.record_bytes(1000, 500);
        store.record_bytes(500, 250);

        assert_eq!(store.global.bytes_sent.load(Ordering::Relaxed), 1500);
        assert_eq!(store.global.bytes_received.load(Ordering::Relaxed), 750);
    }

    #[test]
    fn test_decrement_at_zero() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_connections("b1");
        store.decrement_connections("b1");
        store.decrement_connections("b1"); // Should not underflow

        assert_eq!(store.get_connection_count("b1"), 0);
    }

    #[test]
    fn test_decrement_nonexistent() {
        let store = PrometheusMetricsStore::new("eu".to_string());
        store.decrement_connections("nonexistent"); // Should not panic
        assert_eq!(store.get_connection_count("nonexistent"), 0);
    }

    #[test]
    fn test_get_last_rtt_nonexistent() {
        let store = PrometheusMetricsStore::new("eu".to_string());
        assert!(store.get_last_rtt("nonexistent").is_none());
    }

    #[test]
    fn test_backend_ids() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_connections("b1");
        store.increment_connections("b2");
        store.increment_connections("b3");

        let ids = store.backend_ids();
        assert_eq!(ids.len(), 3);
        assert!(ids.contains(&"b1".to_string()));
        assert!(ids.contains(&"b2".to_string()));
        assert!(ids.contains(&"b3".to_string()));
    }

    #[test]
    fn test_export_prometheus() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_connections("backend-1");
        store.record_rtt("backend-1", 42);
        store.record_error("backend-1");
        store.record_bytes(1000, 500);

        let output = store.export_prometheus();

        assert!(output.contains("edgeproxy_connections_total"));
        assert!(output.contains("edgeproxy_bytes_sent_total"));
        assert!(output.contains("edgeproxy_bytes_received_total"));
        assert!(output.contains("edgeproxy_backend_connections_active"));
        assert!(output.contains("backend=\"backend-1\""));
        assert!(output.contains("region=\"eu\""));
    }

    #[test]
    fn test_backend_metrics_default() {
        let metrics = BackendMetrics::default();
        assert_eq!(metrics.active_connections.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.total_connections.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_concurrent_access() {
        use std::thread;

        let store = Arc::new(PrometheusMetricsStore::new("eu".to_string()));
        let mut handles = vec![];

        for _ in 0..10 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    s.increment_connections("b1");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.get_connection_count("b1"), 1000);
        assert_eq!(store.global.total_connections.load(Ordering::Relaxed), 1000);
    }
}
