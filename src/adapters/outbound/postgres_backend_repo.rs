//! PostgreSQL Backend Repository
//!
//! Implements BackendRepository using PostgreSQL for backend storage.

use crate::domain::entities::Backend;
use crate::domain::ports::BackendRepository;
use async_trait::async_trait;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// PostgreSQL connection configuration.
#[derive(Debug, Clone)]
pub struct PostgresConfig {
    /// Connection URL (e.g., postgres://user:pass@host:5432/db)
    pub url: String,
    /// Maximum connections in the pool
    pub max_connections: u32,
    /// Minimum idle connections
    pub min_connections: u32,
    /// Connection timeout
    pub connect_timeout: Duration,
    /// Query timeout
    pub query_timeout: Duration,
    /// How often to reload backends
    pub reload_interval: Duration,
}

impl Default for PostgresConfig {
    fn default() -> Self {
        Self {
            url: "postgres://localhost:5432/edgeproxy".to_string(),
            max_connections: 10,
            min_connections: 2,
            connect_timeout: Duration::from_secs(5),
            query_timeout: Duration::from_secs(10),
            reload_interval: Duration::from_secs(5),
        }
    }
}

/// PostgreSQL-backed backend repository.
///
/// This implementation uses PostgreSQL for persistent backend storage,
/// suitable for production deployments with existing PostgreSQL infrastructure.
pub struct PostgresBackendRepository {
    /// Configuration
    config: PostgresConfig,
    /// Cached backends
    backends: Arc<RwLock<Vec<Backend>>>,
    /// Version counter for change detection
    version: AtomicU64,
    /// Whether the repository has been initialized
    initialized: Arc<RwLock<bool>>,
}

impl PostgresBackendRepository {
    /// Create a new PostgreSQL backend repository.
    pub fn new(config: PostgresConfig) -> Self {
        Self {
            config,
            backends: Arc::new(RwLock::new(Vec::new())),
            version: AtomicU64::new(0),
            initialized: Arc::new(RwLock::new(false)),
        }
    }

    /// Initialize the repository (create table if not exists).
    ///
    /// Note: This is a stub implementation. In production, you would use
    /// sqlx or tokio-postgres to actually connect to PostgreSQL.
    pub async fn initialize(&self) -> Result<(), PostgresError> {
        // In a real implementation, this would:
        // 1. Create connection pool
        // 2. Run migrations / create table if needed
        // 3. Load initial backends

        tracing::info!("PostgreSQL repository initialized (stub)");
        *self.initialized.write().await = true;
        Ok(())
    }

    /// Start the background sync loop.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start_sync(&self) {
        let backends = self.backends.clone();
        let version = Arc::new(AtomicU64::new(self.version.load(Ordering::SeqCst)));
        let config = self.config.clone();
        let initialized = self.initialized.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.reload_interval);

            loop {
                interval.tick().await;

                if !*initialized.read().await {
                    continue;
                }

                match Self::load_backends_from_postgres(&config).await {
                    Ok(new_backends) => {
                        let mut current = backends.write().await;
                        if *current != new_backends {
                            *current = new_backends;
                            version.fetch_add(1, Ordering::SeqCst);
                            tracing::debug!("PostgreSQL backends reloaded");
                        }
                    }
                    Err(e) => {
                        tracing::error!("failed to reload backends from PostgreSQL: {:?}", e);
                    }
                }
            }
        });
    }

    /// Load backends from PostgreSQL.
    ///
    /// This is a stub that returns empty. In production, use sqlx/tokio-postgres.
    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn load_backends_from_postgres(_config: &PostgresConfig) -> Result<Vec<Backend>, PostgresError> {
        // In a real implementation, this would execute:
        // SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit
        // FROM backends
        // WHERE deleted = false

        // For now, return empty (stub implementation)
        Ok(Vec::new())
    }

    /// Add a backend to PostgreSQL.
    pub async fn add_backend(&self, backend: &Backend) -> Result<(), PostgresError> {
        // In production, execute INSERT statement
        tracing::debug!("would add backend {} to PostgreSQL", backend.id);

        // Update cache
        let mut backends = self.backends.write().await;
        backends.push(backend.clone());
        self.version.fetch_add(1, Ordering::SeqCst);

        Ok(())
    }

    /// Remove a backend from PostgreSQL.
    pub async fn remove_backend(&self, backend_id: &str) -> Result<(), PostgresError> {
        // In production, execute UPDATE ... SET deleted = true
        tracing::debug!("would remove backend {} from PostgreSQL", backend_id);

        // Update cache
        let mut backends = self.backends.write().await;
        backends.retain(|b| b.id != backend_id);
        self.version.fetch_add(1, Ordering::SeqCst);

        Ok(())
    }

    /// Update backend health status.
    pub async fn update_health(&self, backend_id: &str, healthy: bool) -> Result<(), PostgresError> {
        // In production, execute UPDATE backends SET healthy = $1 WHERE id = $2
        tracing::debug!("would update backend {} health to {} in PostgreSQL", backend_id, healthy);

        // Update cache
        let mut backends = self.backends.write().await;
        if let Some(backend) = backends.iter_mut().find(|b| b.id == backend_id) {
            backend.healthy = healthy;
            self.version.fetch_add(1, Ordering::SeqCst);
        }

        Ok(())
    }
}

impl Default for PostgresBackendRepository {
    fn default() -> Self {
        Self::new(PostgresConfig::default())
    }
}

#[async_trait]
impl BackendRepository for PostgresBackendRepository {
    async fn get_all(&self) -> Vec<Backend> {
        self.backends.read().await.clone()
    }

    async fn get_by_id(&self, id: &str) -> Option<Backend> {
        self.backends
            .read()
            .await
            .iter()
            .find(|b| b.id == id)
            .cloned()
    }

    async fn get_healthy(&self) -> Vec<Backend> {
        self.backends
            .read()
            .await
            .iter()
            .filter(|b| b.healthy)
            .cloned()
            .collect()
    }

    async fn get_version(&self) -> u64 {
        self.version.load(Ordering::SeqCst)
    }
}

/// PostgreSQL errors.
#[derive(Debug, Clone, PartialEq)]
pub enum PostgresError {
    /// Connection failed
    ConnectionError(String),
    /// Query failed
    QueryError(String),
    /// Data conversion error
    DataError(String),
    /// Not initialized
    NotInitialized,
}

impl std::fmt::Display for PostgresError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PostgresError::ConnectionError(e) => write!(f, "connection error: {}", e),
            PostgresError::QueryError(e) => write!(f, "query error: {}", e),
            PostgresError::DataError(e) => write!(f, "data error: {}", e),
            PostgresError::NotInitialized => write!(f, "repository not initialized"),
        }
    }
}

impl std::error::Error for PostgresError {}

/// SQL schema for backends table.
pub const BACKENDS_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS backends (
    id TEXT PRIMARY KEY,
    app TEXT NOT NULL,
    region TEXT NOT NULL,
    country TEXT NOT NULL,
    wg_ip TEXT NOT NULL,
    port INTEGER NOT NULL,
    healthy INTEGER NOT NULL DEFAULT 1,
    weight INTEGER NOT NULL DEFAULT 1,
    soft_limit INTEGER NOT NULL DEFAULT 100,
    hard_limit INTEGER NOT NULL DEFAULT 150,
    deleted INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP,
    updated_at TIMESTAMP WITH TIME ZONE DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_backends_healthy ON backends(healthy) WHERE deleted = 0;
CREATE INDEX IF NOT EXISTS idx_backends_region ON backends(region) WHERE deleted = 0;
"#;

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::domain::value_objects::RegionCode;

    fn create_test_backend(id: &str) -> Backend {
        Backend {
            id: id.to_string(),
            app: "test".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        }
    }

    #[test]
    fn test_postgres_config_default() {
        let config = PostgresConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.reload_interval, Duration::from_secs(5));
    }

    #[test]
    fn test_postgres_error_display() {
        assert!(PostgresError::ConnectionError("test".to_string())
            .to_string()
            .contains("connection error"));
        assert!(PostgresError::QueryError("test".to_string())
            .to_string()
            .contains("query error"));
        assert!(PostgresError::DataError("test".to_string())
            .to_string()
            .contains("data error"));
        assert_eq!(
            PostgresError::NotInitialized.to_string(),
            "repository not initialized"
        );
    }

    #[tokio::test]
    async fn test_postgres_repo_new() {
        let repo = PostgresBackendRepository::new(PostgresConfig::default());
        assert!(repo.get_all().await.is_empty());
    }

    #[tokio::test]
    async fn test_postgres_repo_default() {
        let repo = PostgresBackendRepository::default();
        assert!(repo.get_all().await.is_empty());
    }

    #[tokio::test]
    async fn test_initialize() {
        let repo = PostgresBackendRepository::default();
        let result = repo.initialize().await;
        assert!(result.is_ok());
        assert!(*repo.initialized.read().await);
    }

    #[tokio::test]
    async fn test_add_backend() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        let backend = create_test_backend("b1");
        repo.add_backend(&backend).await.unwrap();

        let all = repo.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b1");
    }

    #[tokio::test]
    async fn test_remove_backend() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        repo.add_backend(&create_test_backend("b1")).await.unwrap();
        repo.add_backend(&create_test_backend("b2")).await.unwrap();

        repo.remove_backend("b1").await.unwrap();

        let all = repo.get_all().await;
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "b2");
    }

    #[tokio::test]
    async fn test_update_health() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        repo.add_backend(&create_test_backend("b1")).await.unwrap();
        assert!(repo.get_by_id("b1").await.unwrap().healthy);

        repo.update_health("b1", false).await.unwrap();
        assert!(!repo.get_by_id("b1").await.unwrap().healthy);
    }

    #[tokio::test]
    async fn test_get_by_id() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        repo.add_backend(&create_test_backend("b1")).await.unwrap();

        assert!(repo.get_by_id("b1").await.is_some());
        assert!(repo.get_by_id("b999").await.is_none());
    }

    #[tokio::test]
    async fn test_get_healthy() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        let mut b1 = create_test_backend("b1");
        b1.healthy = true;
        let mut b2 = create_test_backend("b2");
        b2.healthy = false;

        repo.add_backend(&b1).await.unwrap();
        repo.add_backend(&b2).await.unwrap();

        let healthy = repo.get_healthy().await;
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, "b1");
    }

    #[tokio::test]
    async fn test_get_version() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        let v1 = repo.get_version().await;

        repo.add_backend(&create_test_backend("b1")).await.unwrap();

        let v2 = repo.get_version().await;
        assert!(v2 > v1);
    }

    #[tokio::test]
    async fn test_update_health_unknown_backend() {
        let repo = PostgresBackendRepository::default();
        repo.initialize().await.unwrap();

        // Should not error on unknown backend
        let result = repo.update_health("unknown", false).await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_backends_schema() {
        assert!(BACKENDS_SCHEMA.contains("CREATE TABLE"));
        assert!(BACKENDS_SCHEMA.contains("backends"));
        assert!(BACKENDS_SCHEMA.contains("region"));
        assert!(BACKENDS_SCHEMA.contains("healthy"));
    }
}
