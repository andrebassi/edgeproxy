//! Circuit Breaker Pattern
//!
//! Prevents cascading failures by temporarily blocking requests to failing backends.

use dashmap::DashMap;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Circuit breaker configuration.
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening the circuit
    pub failure_threshold: u32,
    /// Duration to keep circuit open before testing
    pub reset_timeout: Duration,
    /// Number of successes in half-open to close circuit
    pub success_threshold: u32,
    /// Window for counting failures (failures older than this are forgotten)
    pub failure_window: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(30),
            success_threshold: 3,
            failure_window: Duration::from_secs(60),
        }
    }
}

/// Circuit breaker state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    /// Normal operation - requests allowed
    Closed,
    /// Circuit tripped - requests blocked
    Open,
    /// Testing recovery - limited requests allowed
    HalfOpen,
}

impl std::fmt::Display for CircuitState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CircuitState::Closed => write!(f, "closed"),
            CircuitState::Open => write!(f, "open"),
            CircuitState::HalfOpen => write!(f, "half-open"),
        }
    }
}

/// Per-backend circuit breaker state.
#[derive(Debug)]
struct BackendCircuit {
    /// Current state (encoded as u32: 0=Closed, 1=Open, 2=HalfOpen)
    state: AtomicU32,
    /// Consecutive failures in current window
    failures: AtomicU32,
    /// Consecutive successes (for half-open recovery)
    successes: AtomicU32,
    /// Timestamp when circuit opened (ms since start)
    opened_at_ms: AtomicU64,
    /// Last failure timestamp (ms since start)
    last_failure_ms: AtomicU64,
}

impl BackendCircuit {
    fn new() -> Self {
        Self {
            state: AtomicU32::new(0), // Closed
            failures: AtomicU32::new(0),
            successes: AtomicU32::new(0),
            opened_at_ms: AtomicU64::new(0),
            last_failure_ms: AtomicU64::new(0),
        }
    }

    fn get_state(&self) -> CircuitState {
        match self.state.load(Ordering::SeqCst) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            _ => CircuitState::HalfOpen,
        }
    }

    fn set_state(&self, state: CircuitState) {
        let val = match state {
            CircuitState::Closed => 0,
            CircuitState::Open => 1,
            CircuitState::HalfOpen => 2,
        };
        self.state.store(val, Ordering::SeqCst);
    }
}

/// Circuit breaker for backends.
///
/// Tracks failure rates and prevents requests to failing backends.
pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    /// Per-backend circuit state
    circuits: DashMap<String, BackendCircuit>,
}

impl CircuitBreaker {
    /// Create a new circuit breaker.
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            circuits: DashMap::new(),
        }
    }

    /// Get the current timestamp in milliseconds.
    fn now_ms() -> u64 {
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_millis() as u64
    }

    /// Get or create circuit for a backend.
    fn get_or_create(&self, backend_id: &str) -> dashmap::mapref::one::Ref<'_, String, BackendCircuit> {
        if !self.circuits.contains_key(backend_id) {
            self.circuits.insert(backend_id.to_string(), BackendCircuit::new());
        }
        self.circuits.get(backend_id).unwrap()
    }

    /// Check if a request to this backend is allowed.
    ///
    /// Returns true if allowed, false if circuit is open.
    pub fn allow_request(&self, backend_id: &str) -> bool {
        let circuit = self.get_or_create(backend_id);
        let state = circuit.get_state();

        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if reset timeout has passed
                let opened_at = circuit.opened_at_ms.load(Ordering::Relaxed);
                let now = Self::now_ms();
                let reset_timeout_ms = self.config.reset_timeout.as_millis() as u64;

                if now.saturating_sub(opened_at) >= reset_timeout_ms {
                    // Transition to half-open
                    circuit.set_state(CircuitState::HalfOpen);
                    circuit.successes.store(0, Ordering::Relaxed);
                    tracing::info!("circuit breaker for {} transitioning to half-open", backend_id);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true, // Allow test requests
        }
    }

    /// Record a successful request.
    pub fn record_success(&self, backend_id: &str) {
        let circuit = self.get_or_create(backend_id);
        let state = circuit.get_state();

        match state {
            CircuitState::HalfOpen => {
                let successes = circuit.successes.fetch_add(1, Ordering::Relaxed) + 1;
                if successes >= self.config.success_threshold {
                    circuit.set_state(CircuitState::Closed);
                    circuit.failures.store(0, Ordering::Relaxed);
                    circuit.successes.store(0, Ordering::Relaxed);
                    tracing::info!("circuit breaker for {} closed (recovered)", backend_id);
                }
            }
            CircuitState::Closed => {
                // Reset failure count on success
                circuit.failures.store(0, Ordering::Relaxed);
            }
            CircuitState::Open => {
                // Should not happen - requests blocked when open
            }
        }
    }

    /// Record a failed request.
    pub fn record_failure(&self, backend_id: &str) {
        let circuit = self.get_or_create(backend_id);
        let state = circuit.get_state();
        let now = Self::now_ms();

        match state {
            CircuitState::Closed => {
                // Check if we need to reset the failure window
                let last_failure = circuit.last_failure_ms.load(Ordering::Relaxed);
                let window_ms = self.config.failure_window.as_millis() as u64;

                if now.saturating_sub(last_failure) > window_ms {
                    // Reset failure count - outside window
                    circuit.failures.store(1, Ordering::Relaxed);
                } else {
                    let failures = circuit.failures.fetch_add(1, Ordering::Relaxed) + 1;
                    if failures >= self.config.failure_threshold {
                        circuit.set_state(CircuitState::Open);
                        circuit.opened_at_ms.store(now, Ordering::Relaxed);
                        tracing::warn!(
                            "circuit breaker for {} opened after {} failures",
                            backend_id,
                            failures
                        );
                    }
                }
                circuit.last_failure_ms.store(now, Ordering::Relaxed);
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open immediately re-opens
                circuit.set_state(CircuitState::Open);
                circuit.opened_at_ms.store(now, Ordering::Relaxed);
                circuit.successes.store(0, Ordering::Relaxed);
                tracing::warn!(
                    "circuit breaker for {} re-opened (failed in half-open)",
                    backend_id
                );
            }
            CircuitState::Open => {
                // Already open, update opened_at to extend timeout
                circuit.opened_at_ms.store(now, Ordering::Relaxed);
            }
        }
    }

    /// Get the current state of a circuit.
    pub fn get_state(&self, backend_id: &str) -> CircuitState {
        self.circuits
            .get(backend_id)
            .map(|c| c.get_state())
            .unwrap_or(CircuitState::Closed)
    }

    /// Get metrics for a circuit.
    pub fn get_metrics(&self, backend_id: &str) -> CircuitMetrics {
        self.circuits
            .get(backend_id)
            .map(|c| CircuitMetrics {
                state: c.get_state(),
                failures: c.failures.load(Ordering::Relaxed),
                successes: c.successes.load(Ordering::Relaxed),
            })
            .unwrap_or_default()
    }

    /// Get all circuit states.
    pub fn all_states(&self) -> Vec<(String, CircuitState)> {
        self.circuits
            .iter()
            .map(|e| (e.key().clone(), e.get_state()))
            .collect()
    }

    /// Manually reset a circuit to closed.
    pub fn reset(&self, backend_id: &str) {
        if let Some(circuit) = self.circuits.get(backend_id) {
            circuit.set_state(CircuitState::Closed);
            circuit.failures.store(0, Ordering::Relaxed);
            circuit.successes.store(0, Ordering::Relaxed);
            tracing::info!("circuit breaker for {} manually reset", backend_id);
        }
    }

    /// Clear all circuit states.
    pub fn clear_all(&self) {
        self.circuits.clear();
    }
}

impl Default for CircuitBreaker {
    fn default() -> Self {
        Self::new(CircuitBreakerConfig::default())
    }
}

/// Metrics for a circuit.
#[derive(Debug, Clone, Default)]
pub struct CircuitMetrics {
    pub state: CircuitState,
    pub failures: u32,
    pub successes: u32,
}

impl Default for CircuitState {
    fn default() -> Self {
        CircuitState::Closed
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_circuit_state_display() {
        assert_eq!(CircuitState::Closed.to_string(), "closed");
        assert_eq!(CircuitState::Open.to_string(), "open");
        assert_eq!(CircuitState::HalfOpen.to_string(), "half-open");
    }

    #[test]
    fn test_circuit_state_default() {
        assert_eq!(CircuitState::default(), CircuitState::Closed);
    }

    #[test]
    fn test_config_default() {
        let config = CircuitBreakerConfig::default();
        assert_eq!(config.failure_threshold, 5);
        assert_eq!(config.reset_timeout, Duration::from_secs(30));
        assert_eq!(config.success_threshold, 3);
    }

    #[test]
    fn test_circuit_breaker_new() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig::default());
        assert!(cb.circuits.is_empty());
    }

    #[test]
    fn test_circuit_breaker_default() {
        let cb = CircuitBreaker::default();
        assert!(cb.circuits.is_empty());
    }

    #[test]
    fn test_allow_request_initial() {
        let cb = CircuitBreaker::default();
        assert!(cb.allow_request("backend-1"));
        assert_eq!(cb.get_state("backend-1"), CircuitState::Closed);
    }

    #[test]
    fn test_allow_request_unknown_backend() {
        let cb = CircuitBreaker::default();
        assert_eq!(cb.get_state("unknown"), CircuitState::Closed);
    }

    #[test]
    fn test_record_success_resets_failures() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 5,
            ..Default::default()
        });

        cb.record_failure("b1");
        cb.record_failure("b1");
        assert_eq!(cb.get_metrics("b1").failures, 2);

        cb.record_success("b1");
        assert_eq!(cb.get_metrics("b1").failures, 0);
    }

    #[test]
    fn test_circuit_opens_after_threshold() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            failure_window: Duration::from_secs(60),
            ..Default::default()
        });

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Closed);

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Closed);

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);
    }

    #[test]
    fn test_circuit_open_blocks_requests() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_secs(60),
            ..Default::default()
        });

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);
        assert!(!cb.allow_request("b1"));
    }

    #[test]
    fn test_circuit_transitions_to_half_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(10),
            ..Default::default()
        });

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);

        // Wait for reset timeout
        std::thread::sleep(Duration::from_millis(20));

        assert!(cb.allow_request("b1")); // Should transition to half-open
        assert_eq!(cb.get_state("b1"), CircuitState::HalfOpen);
    }

    #[test]
    fn test_half_open_closes_on_successes() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(1),
            success_threshold: 2,
            ..Default::default()
        });

        cb.record_failure("b1");
        std::thread::sleep(Duration::from_millis(5));
        cb.allow_request("b1"); // Transition to half-open

        cb.record_success("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::HalfOpen);

        cb.record_success("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Closed);
    }

    #[test]
    fn test_half_open_reopens_on_failure() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_millis(1),
            ..Default::default()
        });

        cb.record_failure("b1");
        std::thread::sleep(Duration::from_millis(5));
        cb.allow_request("b1"); // Transition to half-open

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);
    }

    #[test]
    fn test_get_metrics() {
        let cb = CircuitBreaker::default();

        cb.record_failure("b1");
        cb.record_failure("b1");

        let metrics = cb.get_metrics("b1");
        assert_eq!(metrics.failures, 2);
        assert_eq!(metrics.state, CircuitState::Closed);
    }

    #[test]
    fn test_get_metrics_unknown() {
        let cb = CircuitBreaker::default();
        let metrics = cb.get_metrics("unknown");
        assert_eq!(metrics.failures, 0);
        assert_eq!(metrics.state, CircuitState::Closed);
    }

    #[test]
    fn test_all_states() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        });

        cb.allow_request("b1"); // Creates closed circuit
        cb.record_failure("b2"); // Opens b2

        let states = cb.all_states();
        assert_eq!(states.len(), 2);
    }

    #[test]
    fn test_reset() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            ..Default::default()
        });

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);

        cb.reset("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Closed);
        assert_eq!(cb.get_metrics("b1").failures, 0);
    }

    #[test]
    fn test_reset_unknown() {
        let cb = CircuitBreaker::default();
        cb.reset("unknown"); // Should not panic
    }

    #[test]
    fn test_clear_all() {
        let cb = CircuitBreaker::default();

        cb.allow_request("b1");
        cb.allow_request("b2");
        cb.allow_request("b3");

        assert_eq!(cb.circuits.len(), 3);

        cb.clear_all();
        assert_eq!(cb.circuits.len(), 0);
    }

    #[test]
    fn test_failure_window_resets() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            failure_window: Duration::from_millis(10),
            ..Default::default()
        });

        cb.record_failure("b1");
        cb.record_failure("b1");
        assert_eq!(cb.get_metrics("b1").failures, 2);

        // Wait for window to expire
        std::thread::sleep(Duration::from_millis(20));

        cb.record_failure("b1");
        assert_eq!(cb.get_metrics("b1").failures, 1); // Reset to 1
    }

    #[test]
    fn test_record_failure_when_open() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 1,
            reset_timeout: Duration::from_secs(60),
            ..Default::default()
        });

        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);

        // Recording more failures should just extend timeout
        cb.record_failure("b1");
        assert_eq!(cb.get_state("b1"), CircuitState::Open);
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let cb = Arc::new(CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 100,
            ..Default::default()
        }));

        let mut handles = vec![];

        for _ in 0..10 {
            let cb = cb.clone();
            handles.push(thread::spawn(move || {
                for _ in 0..50 {
                    cb.allow_request("b1");
                    cb.record_failure("b1");
                }
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        // Should still be functional
        assert!(cb.get_metrics("b1").failures > 0);
    }
}
