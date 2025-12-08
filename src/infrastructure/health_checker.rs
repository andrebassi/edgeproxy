//! Active Health Checker
//!
//! Performs periodic health checks on backends via TCP or HTTP probes.

use crate::domain::entities::Backend;
use crate::domain::ports::BackendRepository;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::RwLock;

/// Health check configuration.
#[derive(Debug, Clone)]
pub struct HealthCheckConfig {
    /// Interval between health checks
    pub interval: Duration,
    /// Timeout for each probe
    pub timeout: Duration,
    /// Number of consecutive failures before marking unhealthy
    pub unhealthy_threshold: u32,
    /// Number of consecutive successes before marking healthy
    pub healthy_threshold: u32,
    /// Type of health check
    pub check_type: HealthCheckType,
}

impl Default for HealthCheckConfig {
    fn default() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            unhealthy_threshold: 3,
            healthy_threshold: 2,
            check_type: HealthCheckType::Tcp,
        }
    }
}

/// Type of health check probe.
#[derive(Debug, Clone)]
pub enum HealthCheckType {
    /// Simple TCP connection check
    Tcp,
    /// HTTP GET request (expects 2xx response)
    Http { path: String },
}

/// Health status for a backend.
#[derive(Debug, Clone)]
pub struct HealthStatus {
    /// Whether the backend is healthy
    pub healthy: bool,
    /// Last check timestamp
    pub last_check: Instant,
    /// Last check latency
    pub latency_ms: Option<u64>,
    /// Consecutive failures
    pub consecutive_failures: u32,
    /// Consecutive successes
    pub consecutive_successes: u32,
    /// Last error message
    pub last_error: Option<String>,
}

impl Default for HealthStatus {
    fn default() -> Self {
        Self {
            healthy: true,
            last_check: Instant::now(),
            latency_ms: None,
            consecutive_failures: 0,
            consecutive_successes: 0,
            last_error: None,
        }
    }
}

/// Active health checker for backends.
pub struct HealthChecker {
    config: HealthCheckConfig,
    /// Health status per backend ID
    status: Arc<RwLock<HashMap<String, HealthStatus>>>,
    /// Callback when health changes
    on_health_change: Option<Arc<dyn Fn(&str, bool) + Send + Sync>>,
}

impl HealthChecker {
    /// Create a new health checker.
    pub fn new(config: HealthCheckConfig) -> Self {
        Self {
            config,
            status: Arc::new(RwLock::new(HashMap::new())),
            on_health_change: None,
        }
    }

    /// Set callback for health status changes.
    pub fn on_health_change<F>(mut self, callback: F) -> Self
    where
        F: Fn(&str, bool) + Send + Sync + 'static,
    {
        self.on_health_change = Some(Arc::new(callback));
        self
    }

    /// Get health status for a backend.
    pub async fn get_status(&self, backend_id: &str) -> Option<HealthStatus> {
        self.status.read().await.get(backend_id).cloned()
    }

    /// Check if a backend is healthy.
    pub async fn is_healthy(&self, backend_id: &str) -> bool {
        self.status
            .read()
            .await
            .get(backend_id)
            .map(|s| s.healthy)
            .unwrap_or(true) // Default to healthy if not checked yet
    }

    /// Get all health statuses.
    pub async fn all_statuses(&self) -> HashMap<String, HealthStatus> {
        self.status.read().await.clone()
    }

    /// Start the health check loop.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start<R: BackendRepository + 'static>(&self, backend_repo: Arc<R>) {
        let config = self.config.clone();
        let status = self.status.clone();
        let on_change = self.on_health_change.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.interval);

            loop {
                interval.tick().await;

                let backends = backend_repo.get_all().await;

                for backend in backends {
                    let result = Self::check_backend(&backend, &config).await;
                    Self::update_status(&status, &backend.id, result, &config, &on_change).await;
                }
            }
        });
    }

    /// Perform a single health check on a backend.
    async fn check_backend(backend: &Backend, config: &HealthCheckConfig) -> HealthCheckResult {
        let addr = format!("{}:{}", backend.wg_ip, backend.port);
        let start = Instant::now();

        let result = match &config.check_type {
            HealthCheckType::Tcp => Self::tcp_check(&addr, config.timeout).await,
            HealthCheckType::Http { path } => {
                Self::http_check(&addr, path, config.timeout).await
            }
        };

        let latency = start.elapsed().as_millis() as u64;

        match result {
            Ok(_) => HealthCheckResult::Success { latency_ms: latency },
            Err(e) => HealthCheckResult::Failure {
                error: e,
                latency_ms: latency,
            },
        }
    }

    /// TCP connection check.
    async fn tcp_check(addr: &str, timeout: Duration) -> Result<(), String> {
        match tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
            Ok(Ok(mut stream)) => {
                let _ = stream.shutdown().await;
                Ok(())
            }
            Ok(Err(e)) => Err(format!("connection failed: {}", e)),
            Err(_) => Err("connection timeout".to_string()),
        }
    }

    /// HTTP health check.
    async fn http_check(addr: &str, path: &str, timeout: Duration) -> Result<(), String> {
        let url = format!("http://{}{}", addr, path);

        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .map_err(|e| format!("client error: {}", e))?;

        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => Ok(()),
            Ok(resp) => Err(format!("unhealthy status: {}", resp.status())),
            Err(e) => Err(format!("request failed: {}", e)),
        }
    }

    /// Update health status based on check result.
    async fn update_status(
        status: &Arc<RwLock<HashMap<String, HealthStatus>>>,
        backend_id: &str,
        result: HealthCheckResult,
        config: &HealthCheckConfig,
        on_change: &Option<Arc<dyn Fn(&str, bool) + Send + Sync>>,
    ) {
        let mut statuses = status.write().await;
        let entry = statuses
            .entry(backend_id.to_string())
            .or_insert_with(HealthStatus::default);

        let was_healthy = entry.healthy;

        match result {
            HealthCheckResult::Success { latency_ms } => {
                entry.consecutive_successes += 1;
                entry.consecutive_failures = 0;
                entry.latency_ms = Some(latency_ms);
                entry.last_error = None;

                if !entry.healthy && entry.consecutive_successes >= config.healthy_threshold {
                    entry.healthy = true;
                    tracing::info!("backend {} is now healthy", backend_id);
                }
            }
            HealthCheckResult::Failure { error, latency_ms } => {
                entry.consecutive_failures += 1;
                entry.consecutive_successes = 0;
                entry.latency_ms = Some(latency_ms);
                entry.last_error = Some(error.clone());

                if entry.healthy && entry.consecutive_failures >= config.unhealthy_threshold {
                    entry.healthy = false;
                    tracing::warn!("backend {} is now unhealthy: {}", backend_id, error);
                }
            }
        }

        entry.last_check = Instant::now();

        // Notify callback if health changed
        if was_healthy != entry.healthy {
            if let Some(callback) = on_change {
                callback(backend_id, entry.healthy);
            }
        }
    }

    /// Perform a single check (for testing).
    pub async fn check_once(&self, backend: &Backend) -> HealthCheckResult {
        Self::check_backend(backend, &self.config).await
    }
}

/// Result of a health check.
#[derive(Debug, Clone)]
pub enum HealthCheckResult {
    Success { latency_ms: u64 },
    Failure { error: String, latency_ms: u64 },
}

impl HealthCheckResult {
    pub fn is_success(&self) -> bool {
        matches!(self, HealthCheckResult::Success { .. })
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::domain::value_objects::RegionCode;
    use tokio::net::TcpListener;

    fn create_test_backend(port: u16) -> Backend {
        Backend {
            id: format!("test-{}", port),
            app: "test".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        }
    }

    #[test]
    fn test_health_check_config_default() {
        let config = HealthCheckConfig::default();
        assert_eq!(config.interval, Duration::from_secs(10));
        assert_eq!(config.timeout, Duration::from_secs(5));
        assert_eq!(config.unhealthy_threshold, 3);
        assert_eq!(config.healthy_threshold, 2);
    }

    #[test]
    fn test_health_status_default() {
        let status = HealthStatus::default();
        assert!(status.healthy);
        assert_eq!(status.consecutive_failures, 0);
        assert_eq!(status.consecutive_successes, 0);
    }

    #[test]
    fn test_health_check_result_is_success() {
        let success = HealthCheckResult::Success { latency_ms: 10 };
        assert!(success.is_success());

        let failure = HealthCheckResult::Failure {
            error: "test".to_string(),
            latency_ms: 10,
        };
        assert!(!failure.is_success());
    }

    #[tokio::test]
    async fn test_tcp_check_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Accept connection in background
        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let result = HealthChecker::tcp_check(
            &addr.to_string(),
            Duration::from_secs(1),
        )
        .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_tcp_check_failure() {
        let result = HealthChecker::tcp_check(
            "127.0.0.1:59999",
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_tcp_check_timeout() {
        // Use a non-routable IP to trigger timeout
        let result = HealthChecker::tcp_check(
            "10.255.255.1:80",
            Duration::from_millis(100),
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("timeout"));
    }

    #[tokio::test]
    async fn test_check_backend_success() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let backend = create_test_backend(port);
        let config = HealthCheckConfig::default();

        let result = HealthChecker::check_backend(&backend, &config).await;
        assert!(result.is_success());
    }

    #[tokio::test]
    async fn test_check_backend_failure() {
        let backend = create_test_backend(59999);
        let config = HealthCheckConfig {
            timeout: Duration::from_millis(100),
            ..Default::default()
        };

        let result = HealthChecker::check_backend(&backend, &config).await;
        assert!(!result.is_success());
    }

    #[tokio::test]
    async fn test_health_checker_new() {
        let checker = HealthChecker::new(HealthCheckConfig::default());
        assert!(checker.all_statuses().await.is_empty());
    }

    #[tokio::test]
    async fn test_is_healthy_default() {
        let checker = HealthChecker::new(HealthCheckConfig::default());
        // Unknown backends default to healthy
        assert!(checker.is_healthy("unknown").await);
    }

    #[tokio::test]
    async fn test_get_status_none() {
        let checker = HealthChecker::new(HealthCheckConfig::default());
        assert!(checker.get_status("unknown").await.is_none());
    }

    #[tokio::test]
    async fn test_update_status_success() {
        let status = Arc::new(RwLock::new(HashMap::new()));
        let config = HealthCheckConfig::default();

        let result = HealthCheckResult::Success { latency_ms: 10 };
        HealthChecker::update_status(&status, "b1", result, &config, &None).await;

        let statuses = status.read().await;
        let s = statuses.get("b1").unwrap();
        assert!(s.healthy);
        assert_eq!(s.consecutive_successes, 1);
        assert_eq!(s.consecutive_failures, 0);
        assert_eq!(s.latency_ms, Some(10));
    }

    #[tokio::test]
    async fn test_update_status_failure() {
        let status = Arc::new(RwLock::new(HashMap::new()));
        let config = HealthCheckConfig::default();

        let result = HealthCheckResult::Failure {
            error: "conn refused".to_string(),
            latency_ms: 5,
        };
        HealthChecker::update_status(&status, "b1", result, &config, &None).await;

        let statuses = status.read().await;
        let s = statuses.get("b1").unwrap();
        assert!(s.healthy); // Still healthy (threshold not reached)
        assert_eq!(s.consecutive_failures, 1);
        assert_eq!(s.last_error, Some("conn refused".to_string()));
    }

    #[tokio::test]
    async fn test_update_status_becomes_unhealthy() {
        let status = Arc::new(RwLock::new(HashMap::new()));
        let config = HealthCheckConfig {
            unhealthy_threshold: 2,
            ..Default::default()
        };

        let failure = HealthCheckResult::Failure {
            error: "error".to_string(),
            latency_ms: 5,
        };

        // First failure
        HealthChecker::update_status(&status, "b1", failure.clone(), &config, &None).await;
        assert!(status.read().await.get("b1").unwrap().healthy);

        // Second failure - should become unhealthy
        HealthChecker::update_status(&status, "b1", failure, &config, &None).await;
        assert!(!status.read().await.get("b1").unwrap().healthy);
    }

    #[tokio::test]
    async fn test_update_status_becomes_healthy() {
        let status = Arc::new(RwLock::new(HashMap::new()));
        let config = HealthCheckConfig {
            unhealthy_threshold: 1,
            healthy_threshold: 2,
            ..Default::default()
        };

        // Make it unhealthy first
        let failure = HealthCheckResult::Failure {
            error: "error".to_string(),
            latency_ms: 5,
        };
        HealthChecker::update_status(&status, "b1", failure, &config, &None).await;
        assert!(!status.read().await.get("b1").unwrap().healthy);

        let success = HealthCheckResult::Success { latency_ms: 10 };

        // First success
        HealthChecker::update_status(&status, "b1", success.clone(), &config, &None).await;
        assert!(!status.read().await.get("b1").unwrap().healthy);

        // Second success - should become healthy
        HealthChecker::update_status(&status, "b1", success, &config, &None).await;
        assert!(status.read().await.get("b1").unwrap().healthy);
    }

    #[tokio::test]
    async fn test_on_health_change_callback() {
        use std::sync::atomic::{AtomicBool, Ordering};

        let called = Arc::new(AtomicBool::new(false));
        let called_clone = called.clone();

        let checker = HealthChecker::new(HealthCheckConfig {
            unhealthy_threshold: 1,
            ..Default::default()
        })
        .on_health_change(move |_id, _healthy| {
            called_clone.store(true, Ordering::SeqCst);
        });

        let failure = HealthCheckResult::Failure {
            error: "error".to_string(),
            latency_ms: 5,
        };

        HealthChecker::update_status(
            &checker.status,
            "b1",
            failure,
            &checker.config,
            &checker.on_health_change,
        )
        .await;

        assert!(called.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn test_check_once() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let checker = HealthChecker::new(HealthCheckConfig::default());
        let backend = create_test_backend(port);

        let result = checker.check_once(&backend).await;
        assert!(result.is_success());
    }
}
