//! Rate Limiter
//!
//! Token bucket rate limiting per client IP.

use dashmap::DashMap;
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Rate limiter configuration.
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: u64,
    /// Time window for rate limiting
    pub window: Duration,
    /// Maximum burst size (token bucket capacity)
    pub burst_size: u64,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window: Duration::from_secs(1),
            burst_size: 10,
        }
    }
}

/// Per-client rate limit state.
struct ClientState {
    /// Available tokens
    tokens: AtomicU64,
    /// Last refill timestamp (as milliseconds since epoch-ish)
    last_refill_ms: AtomicU64,
}

impl ClientState {
    fn new(burst_size: u64) -> Self {
        Self {
            tokens: AtomicU64::new(burst_size),
            last_refill_ms: AtomicU64::new(Self::now_ms()),
        }
    }

    fn now_ms() -> u64 {
        // Use a simple monotonic counter based on Instant
        static START: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        let start = START.get_or_init(Instant::now);
        start.elapsed().as_millis() as u64
    }
}

/// Token bucket rate limiter.
///
/// Tracks request rates per client IP using the token bucket algorithm.
pub struct RateLimiter {
    config: RateLimitConfig,
    /// Per-client state
    clients: DashMap<IpAddr, ClientState>,
    /// Tokens added per millisecond
    refill_rate_per_ms: f64,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        let refill_rate_per_ms = config.max_requests as f64 / config.window.as_millis() as f64;
        Self {
            config,
            clients: DashMap::new(),
            refill_rate_per_ms,
        }
    }

    /// Check if a request from this IP is allowed.
    ///
    /// Returns true if allowed, false if rate limited.
    pub fn check(&self, ip: IpAddr) -> bool {
        self.check_with_cost(ip, 1)
    }

    /// Check if a request is allowed with a specific cost.
    pub fn check_with_cost(&self, ip: IpAddr, cost: u64) -> bool {
        let state = self.clients
            .entry(ip)
            .or_insert_with(|| ClientState::new(self.config.burst_size));

        let now_ms = ClientState::now_ms();
        let last_refill = state.last_refill_ms.load(Ordering::Relaxed);
        let elapsed_ms = now_ms.saturating_sub(last_refill);

        // Calculate tokens to add
        let tokens_to_add = (elapsed_ms as f64 * self.refill_rate_per_ms) as u64;

        if tokens_to_add > 0 {
            // Refill tokens (capped at burst_size)
            let current = state.tokens.load(Ordering::Relaxed);
            let new_tokens = (current + tokens_to_add).min(self.config.burst_size);
            state.tokens.store(new_tokens, Ordering::Relaxed);
            state.last_refill_ms.store(now_ms, Ordering::Relaxed);
        }

        // Try to consume tokens
        let mut current = state.tokens.load(Ordering::Relaxed);
        loop {
            if current < cost {
                return false; // Rate limited
            }

            match state.tokens.compare_exchange_weak(
                current,
                current - cost,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true, // Allowed
                Err(c) => current = c,
            }
        }
    }

    /// Get remaining tokens for a client.
    pub fn remaining(&self, ip: IpAddr) -> u64 {
        self.clients
            .get(&ip)
            .map(|s| s.tokens.load(Ordering::Relaxed))
            .unwrap_or(self.config.burst_size)
    }

    /// Clear rate limit state for a client.
    pub fn clear(&self, ip: IpAddr) {
        self.clients.remove(&ip);
    }

    /// Clear all rate limit state.
    pub fn clear_all(&self) {
        self.clients.clear();
    }

    /// Get the number of tracked clients.
    pub fn client_count(&self) -> usize {
        self.clients.len()
    }

    /// Remove stale entries (clients that haven't been seen for a while).
    pub fn cleanup(&self, max_age: Duration) {
        let now_ms = ClientState::now_ms();
        let max_age_ms = max_age.as_millis() as u64;

        self.clients.retain(|_, state| {
            let last_refill = state.last_refill_ms.load(Ordering::Relaxed);
            now_ms.saturating_sub(last_refill) < max_age_ms
        });
    }

    /// Start periodic cleanup task.
    ///
    /// Note: This method requires the RateLimiter to be wrapped in an Arc
    /// and passed to a spawned task. Use `start_cleanup_with_arc` instead.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start_cleanup_with_arc(limiter: std::sync::Arc<Self>, interval: Duration, max_age: Duration) {
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                let now_ms = ClientState::now_ms();
                let max_age_ms = max_age.as_millis() as u64;

                let before = limiter.clients.len();
                limiter.clients.retain(|_, state| {
                    let last_refill = state.last_refill_ms.load(Ordering::Relaxed);
                    now_ms.saturating_sub(last_refill) < max_age_ms
                });
                let after = limiter.clients.len();

                if before != after {
                    tracing::debug!(
                        "rate limiter cleanup: removed {} stale entries",
                        before - after
                    );
                }
            }
        });
    }
}

impl Default for RateLimiter {
    fn default() -> Self {
        Self::new(RateLimitConfig::default())
    }
}

/// Result of a rate limit check.
#[derive(Debug, Clone, PartialEq)]
pub enum RateLimitResult {
    /// Request is allowed
    Allowed { remaining: u64 },
    /// Request is rate limited
    Limited { retry_after_ms: u64 },
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn test_ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(192, 168, 1, n))
    }

    #[test]
    fn test_rate_limit_config_default() {
        let config = RateLimitConfig::default();
        assert_eq!(config.max_requests, 100);
        assert_eq!(config.window, Duration::from_secs(1));
        assert_eq!(config.burst_size, 10);
    }

    #[test]
    fn test_rate_limiter_new() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        assert_eq!(limiter.client_count(), 0);
    }

    #[test]
    fn test_rate_limiter_default() {
        let limiter = RateLimiter::default();
        assert_eq!(limiter.client_count(), 0);
    }

    #[test]
    fn test_check_allows_initial_burst() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 5,
            max_requests: 10,
            window: Duration::from_secs(1),
        });

        let ip = test_ip(1);

        // Should allow burst_size requests
        for _ in 0..5 {
            assert!(limiter.check(ip));
        }

        // Should be rate limited after burst
        assert!(!limiter.check(ip));
    }

    #[test]
    fn test_check_different_clients_isolated() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 2,
            max_requests: 10,
            window: Duration::from_secs(1),
        });

        let ip1 = test_ip(1);
        let ip2 = test_ip(2);

        // Exhaust ip1's tokens
        assert!(limiter.check(ip1));
        assert!(limiter.check(ip1));
        assert!(!limiter.check(ip1));

        // ip2 should still have tokens
        assert!(limiter.check(ip2));
        assert!(limiter.check(ip2));
    }

    #[test]
    fn test_remaining() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 5,
            ..Default::default()
        });

        let ip = test_ip(1);

        // Unknown client has full burst
        assert_eq!(limiter.remaining(ip), 5);

        limiter.check(ip);
        assert_eq!(limiter.remaining(ip), 4);

        limiter.check(ip);
        limiter.check(ip);
        assert_eq!(limiter.remaining(ip), 2);
    }

    #[test]
    fn test_clear_client() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 2,
            ..Default::default()
        });

        let ip = test_ip(1);

        limiter.check(ip);
        limiter.check(ip);
        assert!(!limiter.check(ip));

        limiter.clear(ip);

        // Should have full burst again
        assert!(limiter.check(ip));
        assert!(limiter.check(ip));
    }

    #[test]
    fn test_clear_all() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 1,
            ..Default::default()
        });

        for i in 0..5 {
            limiter.check(test_ip(i));
        }

        assert_eq!(limiter.client_count(), 5);

        limiter.clear_all();

        assert_eq!(limiter.client_count(), 0);
    }

    #[test]
    fn test_check_with_cost() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 10,
            ..Default::default()
        });

        let ip = test_ip(1);

        assert!(limiter.check_with_cost(ip, 5));
        assert_eq!(limiter.remaining(ip), 5);

        assert!(limiter.check_with_cost(ip, 5));
        assert_eq!(limiter.remaining(ip), 0);

        assert!(!limiter.check_with_cost(ip, 1));
    }

    #[test]
    fn test_check_with_cost_too_high() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 5,
            ..Default::default()
        });

        let ip = test_ip(1);

        // Cost higher than available tokens
        assert!(!limiter.check_with_cost(ip, 10));
        // Tokens should be unchanged
        assert_eq!(limiter.remaining(ip), 5);
    }

    #[test]
    fn test_cleanup_removes_stale() {
        let limiter = RateLimiter::new(RateLimitConfig::default());

        for i in 0..5 {
            limiter.check(test_ip(i));
        }

        assert_eq!(limiter.client_count(), 5);

        // Wait a bit and cleanup with very short max_age
        std::thread::sleep(Duration::from_millis(10));
        limiter.cleanup(Duration::from_millis(1));

        assert_eq!(limiter.client_count(), 0);
    }

    #[test]
    fn test_cleanup_keeps_recent() {
        let limiter = RateLimiter::new(RateLimitConfig::default());

        let ip = test_ip(1);
        limiter.check(ip);

        // Cleanup with long max_age should keep entry
        limiter.cleanup(Duration::from_secs(60));

        assert_eq!(limiter.client_count(), 1);
    }

    #[tokio::test]
    async fn test_refill_over_time() {
        let limiter = RateLimiter::new(RateLimitConfig {
            burst_size: 2,
            max_requests: 1000, // High rate = fast refill
            window: Duration::from_millis(100),
        });

        let ip = test_ip(1);

        // Exhaust tokens
        limiter.check(ip);
        limiter.check(ip);
        assert!(!limiter.check(ip));

        // Wait for refill
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Should have tokens again
        assert!(limiter.check(ip));
    }

    #[test]
    fn test_concurrent_access() {
        use std::sync::Arc;
        use std::thread;

        let limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            burst_size: 100,
            max_requests: 1000,
            window: Duration::from_secs(1),
        }));

        let mut handles = vec![];

        for i in 0..10 {
            let l = limiter.clone();
            handles.push(thread::spawn(move || {
                let ip = test_ip(i as u8);
                let mut allowed = 0;
                for _ in 0..20 {
                    if l.check(ip) {
                        allowed += 1;
                    }
                }
                allowed
            }));
        }

        let total: usize = handles.into_iter().map(|h| h.join().unwrap()).sum();

        // Each client should have gotten at most 100 (burst_size)
        // but likely 100 since we have high refill rate
        assert!(total > 0);
    }
}
