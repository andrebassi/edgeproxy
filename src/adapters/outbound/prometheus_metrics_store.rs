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

/// Replication-specific metrics for distributed state sync.
#[derive(Debug, Default)]
pub struct ReplicationMetrics {
    /// Time since last successful sync (milliseconds)
    pub lag_ms: AtomicU64,
    /// Number of changes pending broadcast
    pub pending_changes: AtomicU64,
    /// Total changes applied from remote peers
    pub applied_total: AtomicU64,
    /// Total changesets broadcast to peers
    pub broadcast_total: AtomicU64,
    /// Total replication errors
    pub errors_total: AtomicU64,
    /// Number of alive peers in the cluster
    pub peers_alive: AtomicU64,
    /// Number of anti-entropy repairs performed
    pub merkle_repairs_total: AtomicU64,
    /// Total bytes sent for replication
    pub bytes_sent: AtomicU64,
    /// Total bytes received for replication
    pub bytes_received: AtomicU64,
    /// Timestamp of last successful sync (Unix millis)
    pub last_sync_timestamp: AtomicU64,
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
    /// Replication metrics
    replication: Arc<ReplicationMetrics>,
    /// Region label for metrics
    region: String,
}

impl PrometheusMetricsStore {
    /// Create a new Prometheus metrics store.
    pub fn new(region: String) -> Self {
        Self {
            backends: DashMap::new(),
            global: Arc::new(AggregatedMetrics::default()),
            replication: Arc::new(ReplicationMetrics::default()),
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

    /// Get replication metrics.
    pub fn replication_metrics(&self) -> &ReplicationMetrics {
        &self.replication
    }

    // ==================== Replication Metrics ====================

    /// Record replication lag (time since last sync).
    pub fn record_replication_lag(&self, lag_ms: u64) {
        self.replication.lag_ms.store(lag_ms, Ordering::Relaxed);
    }

    /// Set the number of pending changes.
    pub fn set_pending_changes(&self, count: u64) {
        self.replication.pending_changes.store(count, Ordering::Relaxed);
    }

    /// Increment the count of applied changes.
    pub fn increment_applied_changes(&self, count: u64) {
        self.replication.applied_total.fetch_add(count, Ordering::Relaxed);
    }

    /// Increment the count of broadcast changesets.
    pub fn increment_broadcast(&self) {
        self.replication.broadcast_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Increment replication errors.
    pub fn increment_replication_error(&self) {
        self.replication.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Set the number of alive peers.
    pub fn set_alive_peers(&self, count: u64) {
        self.replication.peers_alive.store(count, Ordering::Relaxed);
    }

    /// Increment Merkle tree repair count.
    pub fn increment_merkle_repairs(&self) {
        self.replication.merkle_repairs_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record replication bytes transferred.
    pub fn record_replication_bytes(&self, sent: u64, received: u64) {
        self.replication.bytes_sent.fetch_add(sent, Ordering::Relaxed);
        self.replication.bytes_received.fetch_add(received, Ordering::Relaxed);
    }

    /// Update last sync timestamp.
    pub fn update_last_sync(&self) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        self.replication.last_sync_timestamp.store(now, Ordering::Relaxed);

        // Calculate and update lag
        self.replication.lag_ms.store(0, Ordering::Relaxed);
    }

    /// Calculate current lag based on last sync timestamp.
    pub fn calculate_lag(&self) -> u64 {
        let last_sync = self.replication.last_sync_timestamp.load(Ordering::Relaxed);
        if last_sync == 0 {
            return 0;
        }
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        now.saturating_sub(last_sync)
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

        // Replication metrics
        output.push_str("\n# HELP edgeproxy_replication_lag_ms Time since last successful sync in milliseconds\n");
        output.push_str("# TYPE edgeproxy_replication_lag_ms gauge\n");
        output.push_str(&format!(
            "edgeproxy_replication_lag_ms{{region=\"{}\"}} {}\n",
            self.region,
            self.calculate_lag()
        ));

        output.push_str("# HELP edgeproxy_replication_pending_changes Number of changes pending broadcast\n");
        output.push_str("# TYPE edgeproxy_replication_pending_changes gauge\n");
        output.push_str(&format!(
            "edgeproxy_replication_pending_changes{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.pending_changes.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_applied_total Total changes applied from remote peers\n");
        output.push_str("# TYPE edgeproxy_replication_applied_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_applied_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.applied_total.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_broadcast_total Total changesets broadcast to peers\n");
        output.push_str("# TYPE edgeproxy_replication_broadcast_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_broadcast_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.broadcast_total.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_errors_total Total replication errors\n");
        output.push_str("# TYPE edgeproxy_replication_errors_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_errors_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.errors_total.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_peers_alive Number of alive peers in the cluster\n");
        output.push_str("# TYPE edgeproxy_replication_peers_alive gauge\n");
        output.push_str(&format!(
            "edgeproxy_replication_peers_alive{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.peers_alive.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_merkle_repairs_total Anti-entropy repairs performed\n");
        output.push_str("# TYPE edgeproxy_replication_merkle_repairs_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_merkle_repairs_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.merkle_repairs_total.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_bytes_sent_total Bytes sent for replication\n");
        output.push_str("# TYPE edgeproxy_replication_bytes_sent_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_bytes_sent_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.bytes_sent.load(Ordering::Relaxed)
        ));

        output.push_str("# HELP edgeproxy_replication_bytes_received_total Bytes received for replication\n");
        output.push_str("# TYPE edgeproxy_replication_bytes_received_total counter\n");
        output.push_str(&format!(
            "edgeproxy_replication_bytes_received_total{{region=\"{}\"}} {}\n",
            self.region,
            self.replication.bytes_received.load(Ordering::Relaxed)
        ));

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

    #[test]
    fn test_global_metrics() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_connections("b1");
        store.record_error("b1");
        store.record_bytes(100, 50);

        let global = store.global_metrics();
        assert_eq!(global.total_connections.load(Ordering::Relaxed), 1);
        assert_eq!(global.connection_errors.load(Ordering::Relaxed), 1);
        assert_eq!(global.bytes_sent.load(Ordering::Relaxed), 100);
        assert_eq!(global.bytes_received.load(Ordering::Relaxed), 50);
    }

    #[test]
    fn test_concurrent_decrement() {
        use std::thread;

        let store = Arc::new(PrometheusMetricsStore::new("eu".to_string()));

        // Pre-increment many connections
        for _ in 0..1000 {
            store.increment_connections("b1");
        }
        assert_eq!(store.get_connection_count("b1"), 1000);

        // Concurrently decrement
        let mut handles = vec![];
        for _ in 0..10 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    s.decrement_connections("b1");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.get_connection_count("b1"), 0);
    }

    // ==================== Replication Metrics Tests ====================

    #[test]
    fn test_replication_metrics_default() {
        let metrics = ReplicationMetrics::default();
        assert_eq!(metrics.lag_ms.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.pending_changes.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.applied_total.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.broadcast_total.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.errors_total.load(Ordering::Relaxed), 0);
        assert_eq!(metrics.peers_alive.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn test_replication_lag() {
        let store = PrometheusMetricsStore::new("sa".to_string());

        store.record_replication_lag(150);
        assert_eq!(store.replication.lag_ms.load(Ordering::Relaxed), 150);

        store.record_replication_lag(200);
        assert_eq!(store.replication.lag_ms.load(Ordering::Relaxed), 200);
    }

    #[test]
    fn test_pending_changes() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.set_pending_changes(10);
        assert_eq!(store.replication.pending_changes.load(Ordering::Relaxed), 10);

        store.set_pending_changes(5);
        assert_eq!(store.replication.pending_changes.load(Ordering::Relaxed), 5);
    }

    #[test]
    fn test_applied_changes() {
        let store = PrometheusMetricsStore::new("us".to_string());

        store.increment_applied_changes(5);
        assert_eq!(store.replication.applied_total.load(Ordering::Relaxed), 5);

        store.increment_applied_changes(3);
        assert_eq!(store.replication.applied_total.load(Ordering::Relaxed), 8);
    }

    #[test]
    fn test_broadcast_count() {
        let store = PrometheusMetricsStore::new("ap".to_string());

        store.increment_broadcast();
        store.increment_broadcast();
        store.increment_broadcast();

        assert_eq!(store.replication.broadcast_total.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_replication_errors() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_replication_error();
        store.increment_replication_error();

        assert_eq!(store.replication.errors_total.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_alive_peers() {
        let store = PrometheusMetricsStore::new("sa".to_string());

        store.set_alive_peers(5);
        assert_eq!(store.replication.peers_alive.load(Ordering::Relaxed), 5);

        store.set_alive_peers(3);
        assert_eq!(store.replication.peers_alive.load(Ordering::Relaxed), 3);
    }

    #[test]
    fn test_merkle_repairs() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.increment_merkle_repairs();
        store.increment_merkle_repairs();

        assert_eq!(store.replication.merkle_repairs_total.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn test_replication_bytes() {
        let store = PrometheusMetricsStore::new("us".to_string());

        store.record_replication_bytes(1000, 500);
        assert_eq!(store.replication.bytes_sent.load(Ordering::Relaxed), 1000);
        assert_eq!(store.replication.bytes_received.load(Ordering::Relaxed), 500);

        store.record_replication_bytes(500, 250);
        assert_eq!(store.replication.bytes_sent.load(Ordering::Relaxed), 1500);
        assert_eq!(store.replication.bytes_received.load(Ordering::Relaxed), 750);
    }

    #[test]
    fn test_last_sync_and_lag() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        // Initially lag should be 0
        assert_eq!(store.calculate_lag(), 0);

        // Update last sync
        store.update_last_sync();
        assert_eq!(store.replication.lag_ms.load(Ordering::Relaxed), 0);

        // Wait a bit and check lag increases
        std::thread::sleep(std::time::Duration::from_millis(50));
        let lag = store.calculate_lag();
        assert!(lag >= 50);
    }

    #[test]
    fn test_replication_metrics_getter() {
        let store = PrometheusMetricsStore::new("eu".to_string());

        store.set_alive_peers(4);
        store.increment_broadcast();

        let metrics = store.replication_metrics();
        assert_eq!(metrics.peers_alive.load(Ordering::Relaxed), 4);
        assert_eq!(metrics.broadcast_total.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_export_prometheus_includes_replication() {
        let store = PrometheusMetricsStore::new("sa".to_string());

        store.set_pending_changes(5);
        store.set_alive_peers(3);
        store.increment_applied_changes(100);
        store.increment_broadcast();
        store.record_replication_bytes(5000, 3000);

        let output = store.export_prometheus();

        assert!(output.contains("edgeproxy_replication_lag_ms"));
        assert!(output.contains("edgeproxy_replication_pending_changes"));
        assert!(output.contains("edgeproxy_replication_applied_total"));
        assert!(output.contains("edgeproxy_replication_broadcast_total"));
        assert!(output.contains("edgeproxy_replication_errors_total"));
        assert!(output.contains("edgeproxy_replication_peers_alive"));
        assert!(output.contains("edgeproxy_replication_merkle_repairs_total"));
        assert!(output.contains("edgeproxy_replication_bytes_sent_total"));
        assert!(output.contains("edgeproxy_replication_bytes_received_total"));
        assert!(output.contains("region=\"sa\""));
    }

    #[test]
    fn test_concurrent_replication_metrics() {
        use std::thread;

        let store = Arc::new(PrometheusMetricsStore::new("eu".to_string()));
        let mut handles = vec![];

        // Concurrent increments
        for _ in 0..10 {
            let s = store.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..100 {
                    s.increment_applied_changes(1);
                    s.increment_broadcast();
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        assert_eq!(store.replication.applied_total.load(Ordering::Relaxed), 1000);
        assert_eq!(store.replication.broadcast_total.load(Ordering::Relaxed), 1000);
    }
}
