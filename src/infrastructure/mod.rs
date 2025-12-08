//! Infrastructure Layer
//!
//! Cross-cutting concerns and infrastructure components.

pub mod circuit_breaker;
pub mod config_watcher;
pub mod connection_pool;
pub mod health_checker;
pub mod rate_limiter;
pub mod shutdown;

pub use circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitMetrics, CircuitState};
pub use config_watcher::{ConfigChange, ConfigWatchError, ConfigWatcher, HotValue};
pub use connection_pool::{ConnectionPool, PoolConfig, PoolError, PoolStats, PooledConnection};
pub use health_checker::{HealthCheckConfig, HealthCheckResult, HealthCheckType, HealthChecker, HealthStatus};
pub use rate_limiter::{RateLimitConfig, RateLimitResult, RateLimiter};
pub use shutdown::{shutdown_signal, ConnectionGuard, ShutdownController};
