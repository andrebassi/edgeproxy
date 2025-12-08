//! Auto-Discovery API Server
//!
//! HTTP API for backends to register themselves and send heartbeats.
//! Enables dynamic backend discovery without manual routing.db updates.

use crate::domain::entities::Backend;
use crate::domain::value_objects::RegionCode;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{delete, get, post},
    Json, Router,
};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tower_http::trace::TraceLayer;

/// Registration request from a backend.
#[derive(Debug, Clone, Deserialize)]
pub struct RegisterRequest {
    pub id: String,
    pub app: String,
    pub region: String,
    #[serde(default)]
    pub country: Option<String>,
    pub ip: String,
    pub port: u16,
    #[serde(default = "default_weight")]
    pub weight: u8,
    #[serde(default = "default_soft_limit")]
    pub soft_limit: u32,
    #[serde(default = "default_hard_limit")]
    pub hard_limit: u32,
}

fn default_weight() -> u8 {
    2
}
fn default_soft_limit() -> u32 {
    100
}
fn default_hard_limit() -> u32 {
    150
}

/// Registration response.
#[derive(Debug, Serialize)]
pub struct RegisterResponse {
    pub id: String,
    pub registered: bool,
    pub message: String,
}

/// Backend status response.
#[derive(Debug, Serialize)]
pub struct BackendStatus {
    pub id: String,
    pub app: String,
    pub region: String,
    pub ip: String,
    pub port: u16,
    pub healthy: bool,
    pub last_heartbeat_secs: u64,
    pub registered_secs: u64,
}

/// List of backends response.
#[derive(Debug, Serialize)]
pub struct BackendsListResponse {
    pub backends: Vec<BackendStatus>,
    pub total: usize,
}

/// Health response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub version: String,
    pub registered_backends: usize,
}

/// Registered backend with metadata.
#[derive(Debug, Clone)]
pub struct RegisteredBackend {
    pub backend: Backend,
    pub registered_at: Instant,
    pub last_heartbeat: Instant,
}

/// API Server state.
#[derive(Clone)]
pub struct ApiState {
    /// Registered backends (id -> backend)
    pub backends: Arc<DashMap<String, RegisteredBackend>>,
    /// Heartbeat TTL - backends removed after this time without heartbeat
    pub heartbeat_ttl: Duration,
}

impl ApiState {
    pub fn new(heartbeat_ttl_secs: u64) -> Self {
        Self {
            backends: Arc::new(DashMap::new()),
            heartbeat_ttl: Duration::from_secs(heartbeat_ttl_secs),
        }
    }

    /// Get all healthy backends.
    #[allow(dead_code)]
    pub fn get_healthy_backends(&self) -> Vec<Backend> {
        let now = Instant::now();
        self.backends
            .iter()
            .filter(|entry| now.duration_since(entry.last_heartbeat) < self.heartbeat_ttl)
            .map(|entry| {
                let mut backend = entry.backend.clone();
                backend.healthy = true;
                backend
            })
            .collect()
    }

    /// Get all backends (including unhealthy).
    pub fn get_all_backends(&self) -> Vec<BackendStatus> {
        let now = Instant::now();
        self.backends
            .iter()
            .map(|entry| {
                let healthy =
                    now.duration_since(entry.last_heartbeat) < self.heartbeat_ttl;
                BackendStatus {
                    id: entry.backend.id.clone(),
                    app: entry.backend.app.clone(),
                    region: entry.backend.region.as_str().to_string(),
                    ip: entry.backend.wg_ip.clone(),
                    port: entry.backend.port,
                    healthy,
                    last_heartbeat_secs: now.duration_since(entry.last_heartbeat).as_secs(),
                    registered_secs: now.duration_since(entry.registered_at).as_secs(),
                }
            })
            .collect()
    }

    /// Register or update a backend.
    pub fn register(&self, req: RegisterRequest) -> RegisteredBackend {
        let now = Instant::now();
        let region = RegionCode::from_str(&req.region);
        // Use provided country or derive from region
        let country = req.country.unwrap_or_else(|| region.default_country().to_string());
        let backend = Backend {
            id: req.id.clone(),
            app: req.app,
            region,
            country,
            wg_ip: req.ip,
            port: req.port,
            healthy: true,
            weight: req.weight,
            soft_limit: req.soft_limit,
            hard_limit: req.hard_limit,
        };

        let registered = RegisteredBackend {
            backend,
            registered_at: now,
            last_heartbeat: now,
        };

        self.backends.insert(req.id.clone(), registered.clone());
        registered
    }

    /// Update heartbeat for a backend.
    pub fn heartbeat(&self, id: &str) -> bool {
        if let Some(mut entry) = self.backends.get_mut(id) {
            entry.last_heartbeat = Instant::now();
            true
        } else {
            false
        }
    }

    /// Deregister a backend.
    pub fn deregister(&self, id: &str) -> bool {
        self.backends.remove(id).is_some()
    }

    /// Cleanup expired backends.
    pub fn cleanup_expired(&self) -> usize {
        let now = Instant::now();
        let expired: Vec<String> = self
            .backends
            .iter()
            .filter(|entry| now.duration_since(entry.last_heartbeat) >= self.heartbeat_ttl)
            .map(|entry| entry.key().clone())
            .collect();

        let count = expired.len();
        for id in expired {
            self.backends.remove(&id);
            tracing::info!("removed expired backend: {}", id);
        }
        count
    }
}

/// API Server for Auto-Discovery.
pub struct ApiServer {
    listen_addr: String,
    state: ApiState,
}

impl ApiServer {
    pub fn new(listen_addr: String, heartbeat_ttl_secs: u64) -> Self {
        Self {
            listen_addr,
            state: ApiState::new(heartbeat_ttl_secs),
        }
    }

    /// Get shared state for use by other components.
    #[allow(dead_code)]
    pub fn state(&self) -> ApiState {
        self.state.clone()
    }

    /// Run the API server.
    ///
    /// The final Ok(()) is excluded from coverage since axum::serve runs forever.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn run(&self) -> anyhow::Result<()> {
        let app = Router::new()
            // Health endpoint
            .route("/health", get(health_handler))
            // Backend registration
            .route("/api/v1/register", post(register_handler))
            // Heartbeat
            .route("/api/v1/heartbeat/:id", post(heartbeat_handler))
            // Deregister
            .route("/api/v1/backends/:id", delete(deregister_handler))
            // List backends
            .route("/api/v1/backends", get(list_backends_handler))
            // Get specific backend
            .route("/api/v1/backends/:id", get(get_backend_handler))
            .layer(TraceLayer::new_for_http())
            .with_state(self.state.clone());

        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("Auto-Discovery API listening on {}", self.listen_addr);

        axum::serve(listener, app).await?;
        Ok(())
    }

    /// Start background cleanup task.
    pub fn start_cleanup_task(&self, interval_secs: u64) {
        let state = self.state.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                interval.tick().await;
                let removed = state.cleanup_expired();
                if removed > 0 {
                    tracing::debug!("cleanup: removed {} expired backends", removed);
                }
            }
        });
    }
}

// Handler functions

async fn health_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let response = HealthResponse {
        status: "ok".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        registered_backends: state.backends.len(),
    };
    Json(response)
}

async fn register_handler(
    State(state): State<ApiState>,
    Json(req): Json<RegisterRequest>,
) -> impl IntoResponse {
    let id = req.id.clone();
    let _registered = state.register(req);

    tracing::info!("registered backend: {}", id);

    let response = RegisterResponse {
        id,
        registered: true,
        message: "Backend registered successfully".to_string(),
    };
    (StatusCode::CREATED, Json(response))
}

async fn heartbeat_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.heartbeat(&id) {
        tracing::debug!("heartbeat from: {}", id);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": id,
                "status": "ok"
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "id": id,
                "error": "backend not registered"
            })),
        )
    }
}

async fn deregister_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    if state.deregister(&id) {
        tracing::info!("deregistered backend: {}", id);
        (
            StatusCode::OK,
            Json(serde_json::json!({
                "id": id,
                "deregistered": true
            })),
        )
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "id": id,
                "error": "backend not found"
            })),
        )
    }
}

async fn list_backends_handler(State(state): State<ApiState>) -> impl IntoResponse {
    let backends = state.get_all_backends();
    let total = backends.len();
    Json(BackendsListResponse { backends, total })
}

async fn get_backend_handler(
    State(state): State<ApiState>,
    Path(id): Path<String>,
) -> impl IntoResponse {
    let now = Instant::now();
    if let Some(entry) = state.backends.get(&id) {
        let healthy =
            now.duration_since(entry.last_heartbeat) < state.heartbeat_ttl;
        let status = BackendStatus {
            id: entry.backend.id.clone(),
            app: entry.backend.app.clone(),
            region: entry.backend.region.as_str().to_string(),
            ip: entry.backend.wg_ip.clone(),
            port: entry.backend.port,
            healthy,
            last_heartbeat_secs: now.duration_since(entry.last_heartbeat).as_secs(),
            registered_secs: now.duration_since(entry.registered_at).as_secs(),
        };
        (StatusCode::OK, Json(serde_json::to_value(status).unwrap()))
    } else {
        (
            StatusCode::NOT_FOUND,
            Json(serde_json::json!({
                "error": "backend not found"
            })),
        )
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_api_state_new() {
        let state = ApiState::new(60);
        assert_eq!(state.heartbeat_ttl, Duration::from_secs(60));
        assert!(state.backends.is_empty());
    }

    #[test]
    fn test_register_backend() {
        let state = ApiState::new(60);
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        state.register(req);
        assert_eq!(state.backends.len(), 1);
        assert!(state.backends.contains_key("test-1"));
    }

    #[test]
    fn test_heartbeat() {
        let state = ApiState::new(60);
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        state.register(req);
        assert!(state.heartbeat("test-1"));
        assert!(!state.heartbeat("nonexistent"));
    }

    #[test]
    fn test_deregister() {
        let state = ApiState::new(60);
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        state.register(req);
        assert!(state.deregister("test-1"));
        assert!(!state.deregister("test-1"));
        assert!(state.backends.is_empty());
    }

    #[test]
    fn test_get_healthy_backends() {
        let state = ApiState::new(60);

        // Register two backends
        state.register(RegisterRequest {
            id: "eu-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: Some("DE".to_string()),
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });
        state.register(RegisterRequest {
            id: "us-1".to_string(),
            app: "myapp".to_string(),
            region: "us".to_string(),
            country: Some("US".to_string()),
            ip: "10.0.0.2".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let healthy = state.get_healthy_backends();
        assert_eq!(healthy.len(), 2);
    }

    #[test]
    fn test_get_all_backends() {
        let state = ApiState::new(60);

        state.register(RegisterRequest {
            id: "eu-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let all = state.get_all_backends();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "eu-1");
    }

    #[test]
    fn test_default_values() {
        assert_eq!(default_weight(), 2);
        assert_eq!(default_soft_limit(), 100);
        assert_eq!(default_hard_limit(), 150);
    }

    #[test]
    fn test_register_with_country() {
        let state = ApiState::new(60);
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: Some("FR".to_string()),
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let registered = state.register(req);
        assert_eq!(registered.backend.country, "FR");
        assert_eq!(registered.backend.region, RegionCode::Europe);
    }

    #[test]
    fn test_register_updates_existing() {
        let state = ApiState::new(60);

        // Register first time
        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        // Register again with different IP
        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.99".to_string(),
            port: 9090,
            weight: 5,
            soft_limit: 200,
            hard_limit: 300,
        });

        assert_eq!(state.backends.len(), 1);
        let entry = state.backends.get("test-1").unwrap();
        assert_eq!(entry.backend.wg_ip, "10.0.0.99");
        assert_eq!(entry.backend.port, 9090);
        assert_eq!(entry.backend.weight, 5);
    }

    #[test]
    fn test_get_all_backends_with_multiple_regions() {
        let state = ApiState::new(60);

        state.register(RegisterRequest {
            id: "sa-1".to_string(),
            app: "myapp".to_string(),
            region: "sa".to_string(),
            country: Some("BR".to_string()),
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        state.register(RegisterRequest {
            id: "ap-1".to_string(),
            app: "myapp".to_string(),
            region: "ap".to_string(),
            country: Some("JP".to_string()),
            ip: "10.0.0.2".to_string(),
            port: 8080,
            weight: 3,
            soft_limit: 50,
            hard_limit: 75,
        });

        let all = state.get_all_backends();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_cleanup_expired_removes_old_backends() {
        let state = ApiState::new(0); // TTL of 0 means immediate expiration

        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        assert_eq!(state.backends.len(), 1);

        // With TTL=0, should immediately expire
        std::thread::sleep(std::time::Duration::from_millis(10));
        let removed = state.cleanup_expired();
        assert_eq!(removed, 1);
        assert!(state.backends.is_empty());
    }

    #[test]
    fn test_cleanup_expired_keeps_fresh_backends() {
        let state = ApiState::new(3600); // 1 hour TTL

        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let removed = state.cleanup_expired();
        assert_eq!(removed, 0);
        assert_eq!(state.backends.len(), 1);
    }

    #[test]
    fn test_api_server_new() {
        let server = ApiServer::new("0.0.0.0:8081".to_string(), 60);
        assert_eq!(server.listen_addr, "0.0.0.0:8081");
        assert_eq!(server.state.heartbeat_ttl, Duration::from_secs(60));
    }

    #[test]
    fn test_api_server_state_clone() {
        let server = ApiServer::new("0.0.0.0:8081".to_string(), 60);
        let state1 = server.state();
        let state2 = server.state();

        // Both should share the same backends
        state1.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        assert_eq!(state2.backends.len(), 1);
    }

    #[test]
    fn test_registered_backend_clone() {
        let state = ApiState::new(60);
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let registered = state.register(req);
        let cloned = registered.clone();
        assert_eq!(registered.backend.id, cloned.backend.id);
    }

    #[test]
    fn test_register_request_debug() {
        let req = RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let debug_str = format!("{:?}", req);
        assert!(debug_str.contains("test-1"));
        assert!(debug_str.contains("myapp"));
    }

    #[test]
    fn test_register_response_debug() {
        let response = RegisterResponse {
            id: "test-1".to_string(),
            registered: true,
            message: "ok".to_string(),
        };

        let debug_str = format!("{:?}", response);
        assert!(debug_str.contains("test-1"));
        assert!(debug_str.contains("true"));
    }

    #[test]
    fn test_backend_status_debug() {
        let status = BackendStatus {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            last_heartbeat_secs: 0,
            registered_secs: 100,
        };

        let debug_str = format!("{:?}", status);
        assert!(debug_str.contains("test-1"));
    }

    #[test]
    fn test_backends_list_response_debug() {
        let list = BackendsListResponse {
            backends: vec![],
            total: 0,
        };

        let debug_str = format!("{:?}", list);
        assert!(debug_str.contains("total"));
    }

    #[test]
    fn test_health_response_debug() {
        let health = HealthResponse {
            status: "ok".to_string(),
            version: "0.1.0".to_string(),
            registered_backends: 5,
        };

        let debug_str = format!("{:?}", health);
        assert!(debug_str.contains("ok"));
    }

    #[test]
    fn test_registered_backend_debug() {
        let backend = Backend {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let registered = RegisteredBackend {
            backend,
            registered_at: Instant::now(),
            last_heartbeat: Instant::now(),
        };

        let debug_str = format!("{:?}", registered);
        assert!(debug_str.contains("test-1"));
    }

    #[test]
    fn test_api_state_clone() {
        let state = ApiState::new(60);
        let cloned = state.clone();

        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        // Cloned state should share the same DashMap
        assert_eq!(cloned.backends.len(), 1);
    }

    #[test]
    fn test_register_all_regions() {
        let state = ApiState::new(60);

        for (region, country) in [("sa", "BR"), ("us", "US"), ("eu", "DE"), ("ap", "JP")] {
            state.register(RegisterRequest {
                id: format!("{}-1", region),
                app: "myapp".to_string(),
                region: region.to_string(),
                country: Some(country.to_string()),
                ip: "10.0.0.1".to_string(),
                port: 8080,
                weight: 2,
                soft_limit: 100,
                hard_limit: 150,
            });
        }

        assert_eq!(state.backends.len(), 4);

        let sa = state.backends.get("sa-1").unwrap();
        assert_eq!(sa.backend.region, RegionCode::SouthAmerica);

        let us = state.backends.get("us-1").unwrap();
        assert_eq!(us.backend.region, RegionCode::NorthAmerica);

        let eu = state.backends.get("eu-1").unwrap();
        assert_eq!(eu.backend.region, RegionCode::Europe);

        let ap = state.backends.get("ap-1").unwrap();
        assert_eq!(ap.backend.region, RegionCode::AsiaPacific);
    }

    #[test]
    fn test_backend_status_healthy_reflects_ttl() {
        let state = ApiState::new(3600);

        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let backends = state.get_all_backends();
        assert_eq!(backends.len(), 1);
        assert!(backends[0].healthy);
    }

    // Integration tests for HTTP handlers
    use axum::{
        body::Body,
        http::{Request, StatusCode as HttpStatusCode},
    };
    use tower::ServiceExt;

    fn create_test_app() -> Router {
        let state = ApiState::new(60);
        Router::new()
            .route("/health", get(health_handler))
            .route("/api/v1/register", post(register_handler))
            .route("/api/v1/heartbeat/:id", post(heartbeat_handler))
            .route("/api/v1/backends/:id", delete(deregister_handler))
            .route("/api/v1/backends", get(list_backends_handler))
            .route("/api/v1/backends/:id", get(get_backend_handler))
            .with_state(state)
    }

    fn create_test_app_with_state(state: ApiState) -> Router {
        Router::new()
            .route("/health", get(health_handler))
            .route("/api/v1/register", post(register_handler))
            .route("/api/v1/heartbeat/:id", post(heartbeat_handler))
            .route("/api/v1/backends/:id", delete(deregister_handler))
            .route("/api/v1/backends", get(list_backends_handler))
            .route("/api/v1/backends/:id", get(get_backend_handler))
            .with_state(state)
    }

    #[tokio::test]
    async fn test_health_handler() {
        let app = create_test_app();

        let request = Request::builder()
            .uri("/health")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_register_handler() {
        let app = create_test_app();

        let body = serde_json::json!({
            "id": "backend-1",
            "app": "myapp",
            "region": "eu",
            "ip": "10.0.0.1",
            "port": 8080
        });

        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/register")
            .header("content-type", "application/json")
            .body(Body::from(serde_json::to_string(&body).unwrap()))
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::CREATED);
    }

    #[tokio::test]
    async fn test_heartbeat_handler_success() {
        let state = ApiState::new(60);
        state.register(RegisterRequest {
            id: "backend-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let app = create_test_app_with_state(state);

        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/heartbeat/backend-1")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_heartbeat_handler_not_found() {
        let app = create_test_app();

        let request = Request::builder()
            .method("POST")
            .uri("/api/v1/heartbeat/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_deregister_handler_success() {
        let state = ApiState::new(60);
        state.register(RegisterRequest {
            id: "backend-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let app = create_test_app_with_state(state);

        let request = Request::builder()
            .method("DELETE")
            .uri("/api/v1/backends/backend-1")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_deregister_handler_not_found() {
        let app = create_test_app();

        let request = Request::builder()
            .method("DELETE")
            .uri("/api/v1/backends/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_list_backends_handler_empty() {
        let app = create_test_app();

        let request = Request::builder()
            .uri("/api/v1/backends")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_list_backends_handler_with_backends() {
        let state = ApiState::new(60);
        state.register(RegisterRequest {
            id: "backend-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let app = create_test_app_with_state(state);

        let request = Request::builder()
            .uri("/api/v1/backends")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_backend_handler_success() {
        let state = ApiState::new(60);
        state.register(RegisterRequest {
            id: "backend-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        let app = create_test_app_with_state(state);

        let request = Request::builder()
            .uri("/api/v1/backends/backend-1")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::OK);
    }

    #[tokio::test]
    async fn test_get_backend_handler_not_found() {
        let app = create_test_app();

        let request = Request::builder()
            .uri("/api/v1/backends/nonexistent")
            .body(Body::empty())
            .unwrap();

        let response = app.oneshot(request).await.unwrap();
        assert_eq!(response.status(), HttpStatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn test_start_cleanup_task_removes_expired() {
        use std::time::Duration;

        // Create server with 0 TTL (immediate expiration)
        let server = ApiServer::new("127.0.0.1:0".to_string(), 0);

        // Register a backend
        server.state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        assert_eq!(server.state.backends.len(), 1);

        // Start cleanup with very short interval
        server.start_cleanup_task(1);

        // Wait for cleanup to run
        tokio::time::sleep(Duration::from_millis(1500)).await;

        // Backend should be removed
        assert!(server.state.backends.is_empty());
    }

    #[tokio::test]
    async fn test_cleanup_expired_no_tracing_when_zero() {
        let state = ApiState::new(3600);

        // Register a backend that won't expire
        state.register(RegisterRequest {
            id: "test-1".to_string(),
            app: "myapp".to_string(),
            region: "eu".to_string(),
            country: None,
            ip: "10.0.0.1".to_string(),
            port: 8080,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        });

        // cleanup should return 0 (no expired backends)
        let removed = state.cleanup_expired();
        assert_eq!(removed, 0);
        assert_eq!(state.backends.len(), 1);
    }

    #[tokio::test]
    async fn test_api_server_run_starts_listening() {
        use std::time::Duration;

        // Find an available port
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let server = ApiServer::new(addr.to_string(), 60);

        // Run server in background
        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give server time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Try to connect
        let client = reqwest::Client::new();
        let result = client
            .get(format!("http://{}/health", addr))
            .timeout(Duration::from_secs(2))
            .send()
            .await;

        assert!(result.is_ok());
        let response = result.unwrap();
        assert_eq!(response.status(), reqwest::StatusCode::OK);

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_full_lifecycle() {
        use std::time::Duration;

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);

        let server = ApiServer::new(addr.to_string(), 60);

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        let client = reqwest::Client::new();
        let base_url = format!("http://{}", addr);

        // 1. Health check
        let health = client.get(format!("{}/health", base_url)).send().await.unwrap();
        assert_eq!(health.status(), reqwest::StatusCode::OK);

        // 2. Register backend
        let body = serde_json::json!({
            "id": "test-backend",
            "app": "myapp",
            "region": "eu",
            "ip": "10.0.0.1",
            "port": 8080
        });
        let register = client
            .post(format!("{}/api/v1/register", base_url))
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(register.status(), reqwest::StatusCode::CREATED);

        // 3. List backends
        let list = client.get(format!("{}/api/v1/backends", base_url)).send().await.unwrap();
        assert_eq!(list.status(), reqwest::StatusCode::OK);

        // 4. Get specific backend
        let get = client.get(format!("{}/api/v1/backends/test-backend", base_url)).send().await.unwrap();
        assert_eq!(get.status(), reqwest::StatusCode::OK);

        // 5. Heartbeat
        let heartbeat = client
            .post(format!("{}/api/v1/heartbeat/test-backend", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(heartbeat.status(), reqwest::StatusCode::OK);

        // 6. Deregister
        let deregister = client
            .delete(format!("{}/api/v1/backends/test-backend", base_url))
            .send()
            .await
            .unwrap();
        assert_eq!(deregister.status(), reqwest::StatusCode::OK);

        server_handle.abort();
    }
}
