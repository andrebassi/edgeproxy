//! Metrics Store Port
//!
//! Defines the interface for storing and retrieving runtime metrics.

/// Store for runtime metrics per backend.
///
/// This is an outbound port for tracking connection counts and latency.
/// The load balancer uses this information to make routing decisions.
pub trait MetricsStore: Send + Sync {
    /// Get the current connection count for a backend.
    fn get_connection_count(&self, backend_id: &str) -> usize;

    /// Increment the connection count when a new connection is established.
    fn increment_connections(&self, backend_id: &str);

    /// Decrement the connection count when a connection is closed.
    fn decrement_connections(&self, backend_id: &str);

    /// Record the round-trip time to establish a connection to a backend.
    fn record_rtt(&self, backend_id: &str, rtt_ms: u64);

    /// Get the last recorded RTT for a backend.
    #[allow(dead_code)]
    fn get_last_rtt(&self, backend_id: &str) -> Option<u64>;
}
