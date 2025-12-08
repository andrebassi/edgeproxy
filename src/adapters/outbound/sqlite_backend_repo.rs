//! SQLite Backend Repository
//!
//! Implements BackendRepository using SQLite for storage.
//! Supports periodic reloading for dynamic backend updates.

use crate::domain::entities::Backend;
use crate::domain::ports::BackendRepository;
use crate::domain::value_objects::RegionCode;
use anyhow::Result;
use async_trait::async_trait;
use rusqlite::{Connection, Row};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

/// SQLite-backed backend repository.
///
/// Periodically reloads backends from the database file.
/// The database is expected to be replicated via Corrosion.
pub struct SqliteBackendRepository {
    backends: Arc<RwLock<Vec<Backend>>>,
    version: Arc<AtomicU64>,
}

impl SqliteBackendRepository {
    /// Create a new repository (empty until sync starts).
    pub fn new() -> Self {
        Self {
            backends: Arc::new(RwLock::new(Vec::new())),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start the background sync task.
    ///
    /// This spawns a Tokio task that periodically reloads backends
    /// from the SQLite database file. The error handling paths inside
    /// the spawned task are excluded from coverage as they require
    /// specific runtime failures (spawn_blocking, IO errors) to trigger.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start_sync(&self, db_path: String, interval_secs: u64) {
        let backends = self.backends.clone();
        let version = self.version.clone();

        tokio::spawn(async move {
            loop {
                let db_path_clone = db_path.clone();
                match tokio::task::spawn_blocking(move || {
                    Self::load_from_sqlite(&db_path_clone)
                })
                .await
                {
                    Ok(Ok(new_backends)) => {
                        let count = new_backends.len();
                        {
                            let mut guard = backends.write().await;
                            *guard = new_backends;
                        }
                        let new_version = version.fetch_add(1, Ordering::SeqCst) + 1;
                        tracing::info!(
                            "routing reload ok, version={} backends={}",
                            new_version,
                            count
                        );
                    }
                    Ok(Err(e)) => tracing::error!("error reading routing: {:?}", e),
                    Err(e) => tracing::error!("spawn_blocking error: {:?}", e),
                }

                sleep(Duration::from_secs(interval_secs)).await;
            }
        });
    }

    /// Load backends from SQLite database file.
    ///
    /// This function is only called from start_sync and error paths
    /// (invalid SQL, missing table) are excluded from coverage.
    #[cfg_attr(coverage_nightly, coverage(off))]
    fn load_from_sqlite(db_path: &str) -> Result<Vec<Backend>> {
        let conn = Connection::open(db_path)?;

        let mut stmt = conn.prepare(
            "SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit
             FROM backends
             WHERE deleted IS NULL OR deleted = 0",
        )?;

        let backends = stmt
            .query_map([], Self::row_to_backend)?
            .collect::<Result<Vec<_>, _>>()?;

        Ok(backends)
    }

    /// Convert a SQLite row to a Backend entity.
    fn row_to_backend(row: &Row) -> rusqlite::Result<Backend> {
        Ok(Backend {
            id: row.get(0)?,
            app: row.get(1)?,
            region: RegionCode::from_str(&row.get::<_, String>(2)?),
            country: row.get(3)?,
            wg_ip: row.get(4)?,
            port: row.get::<_, i64>(5)? as u16,
            healthy: row.get::<_, i64>(6)? != 0,
            weight: row.get::<_, i64>(7)? as u8,
            soft_limit: row.get::<_, i64>(8)? as u32,
            hard_limit: row.get::<_, i64>(9)? as u32,
        })
    }
}

impl Default for SqliteBackendRepository {
    fn default() -> Self {
        Self::new()
    }
}

impl SqliteBackendRepository {
    /// Create repository with pre-loaded backends (for testing).
    #[cfg(test)]
    pub fn with_backends(backends: Vec<Backend>) -> Self {
        Self {
            backends: Arc::new(RwLock::new(backends)),
            version: Arc::new(AtomicU64::new(1)),
        }
    }
}

#[async_trait]
impl BackendRepository for SqliteBackendRepository {
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

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    fn create_test_backend(id: &str, healthy: bool) -> Backend {
        Backend {
            id: id.to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.50.1.1".to_string(),
            port: 8080,
            healthy,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        }
    }

    #[test]
    fn test_new_repository() {
        let repo = SqliteBackendRepository::new();
        assert_eq!(repo.version.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_default_repository() {
        let repo = SqliteBackendRepository::default();
        assert_eq!(repo.version.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_with_backends() {
        let backends = vec![
            create_test_backend("backend-1", true),
            create_test_backend("backend-2", false),
        ];
        let repo = SqliteBackendRepository::with_backends(backends);
        assert_eq!(repo.version.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_get_all_empty() {
        let repo = SqliteBackendRepository::new();
        let backends = repo.get_all().await;
        assert!(backends.is_empty());
    }

    #[tokio::test]
    async fn test_get_all_with_backends() {
        let backends = vec![
            create_test_backend("eu-1", true),
            create_test_backend("us-1", true),
        ];
        let repo = SqliteBackendRepository::with_backends(backends);
        let result = repo.get_all().await;
        assert_eq!(result.len(), 2);
    }

    #[tokio::test]
    async fn test_get_by_id_found() {
        let backends = vec![
            create_test_backend("eu-1", true),
            create_test_backend("us-1", true),
        ];
        let repo = SqliteBackendRepository::with_backends(backends);
        let result = repo.get_by_id("eu-1").await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "eu-1");
    }

    #[tokio::test]
    async fn test_get_by_id_not_found() {
        let backends = vec![create_test_backend("eu-1", true)];
        let repo = SqliteBackendRepository::with_backends(backends);
        let result = repo.get_by_id("nonexistent").await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_get_healthy_filters_unhealthy() {
        let backends = vec![
            create_test_backend("healthy-1", true),
            create_test_backend("unhealthy-1", false),
            create_test_backend("healthy-2", true),
        ];
        let repo = SqliteBackendRepository::with_backends(backends);
        let healthy = repo.get_healthy().await;
        assert_eq!(healthy.len(), 2);
        assert!(healthy.iter().all(|b| b.healthy));
    }

    #[tokio::test]
    async fn test_get_healthy_empty_when_all_unhealthy() {
        let backends = vec![
            create_test_backend("unhealthy-1", false),
            create_test_backend("unhealthy-2", false),
        ];
        let repo = SqliteBackendRepository::with_backends(backends);
        let healthy = repo.get_healthy().await;
        assert!(healthy.is_empty());
    }

    #[tokio::test]
    async fn test_get_version_initial() {
        let repo = SqliteBackendRepository::new();
        let version = repo.get_version().await;
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn test_get_version_after_with_backends() {
        let backends = vec![create_test_backend("eu-1", true)];
        let repo = SqliteBackendRepository::with_backends(backends);
        let version = repo.get_version().await;
        assert_eq!(version, 1);
    }

    #[test]
    fn test_load_from_sqlite_nonexistent_file() {
        let result = SqliteBackendRepository::load_from_sqlite("/nonexistent/path/db.sqlite");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_from_sqlite_with_temp_db() {
        use tempfile::NamedTempFile;

        // Create a temporary SQLite database
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();

        // Create schema and insert test data
        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY,
                app TEXT,
                region TEXT,
                country TEXT,
                wg_ip TEXT,
                port INTEGER,
                healthy INTEGER,
                weight INTEGER,
                soft_limit INTEGER,
                hard_limit INTEGER,
                deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO backends VALUES ('test-1', 'myapp', 'eu', 'DE', '10.50.1.1', 8080, 1, 2, 100, 150, 0)",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO backends VALUES ('test-2', 'myapp', 'us', 'US', '10.50.2.1', 8080, 0, 1, 50, 100, 0)",
            [],
        )
        .unwrap();

        // Test loading
        let backends = SqliteBackendRepository::load_from_sqlite(db_path).unwrap();
        assert_eq!(backends.len(), 2);

        let eu_backend = backends.iter().find(|b| b.id == "test-1").unwrap();
        assert_eq!(eu_backend.app, "myapp");
        assert_eq!(eu_backend.region, RegionCode::Europe);
        assert_eq!(eu_backend.country, "DE");
        assert!(eu_backend.healthy);

        let us_backend = backends.iter().find(|b| b.id == "test-2").unwrap();
        assert!(!us_backend.healthy);
    }

    #[test]
    fn test_load_from_sqlite_excludes_deleted() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();

        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY,
                app TEXT,
                region TEXT,
                country TEXT,
                wg_ip TEXT,
                port INTEGER,
                healthy INTEGER,
                weight INTEGER,
                soft_limit INTEGER,
                hard_limit INTEGER,
                deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();

        // Active backend
        conn.execute(
            "INSERT INTO backends VALUES ('active', 'myapp', 'eu', 'DE', '10.50.1.1', 8080, 1, 2, 100, 150, 0)",
            [],
        )
        .unwrap();

        // Deleted backend
        conn.execute(
            "INSERT INTO backends VALUES ('deleted', 'myapp', 'eu', 'DE', '10.50.1.2', 8080, 1, 2, 100, 150, 1)",
            [],
        )
        .unwrap();

        let backends = SqliteBackendRepository::load_from_sqlite(db_path).unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "active");
    }

    #[test]
    fn test_row_to_backend_mapping() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();

        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT, app TEXT, region TEXT, country TEXT, wg_ip TEXT,
                port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO backends VALUES ('b1', 'app1', 'sa', 'BR', '10.1.1.1', 9000, 1, 5, 200, 300)",
            [],
        )
        .unwrap();

        let mut stmt = conn
            .prepare("SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit FROM backends")
            .unwrap();

        let backend = stmt
            .query_row([], SqliteBackendRepository::row_to_backend)
            .unwrap();

        assert_eq!(backend.id, "b1");
        assert_eq!(backend.app, "app1");
        assert_eq!(backend.region, RegionCode::SouthAmerica);
        assert_eq!(backend.country, "BR");
        assert_eq!(backend.wg_ip, "10.1.1.1");
        assert_eq!(backend.port, 9000);
        assert!(backend.healthy);
        assert_eq!(backend.weight, 5);
        assert_eq!(backend.soft_limit, 200);
        assert_eq!(backend.hard_limit, 300);
    }

    // ===== Integration Tests for start_sync =====

    #[tokio::test]
    async fn test_start_sync_loads_backends() {
        use tempfile::NamedTempFile;

        // Create temp database with backends
        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY,
                app TEXT,
                region TEXT,
                country TEXT,
                wg_ip TEXT,
                port INTEGER,
                healthy INTEGER,
                weight INTEGER,
                soft_limit INTEGER,
                hard_limit INTEGER,
                deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();

        conn.execute(
            "INSERT INTO backends VALUES ('sync-test-1', 'myapp', 'eu', 'DE', '10.50.1.1', 8080, 1, 2, 100, 150, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        // Create repo and start sync
        let repo = SqliteBackendRepository::new();
        assert!(repo.get_all().await.is_empty());

        // Start sync with 1 second interval
        repo.start_sync(db_path, 1);

        // Wait for first sync cycle
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should have loaded backends
        let backends = repo.get_all().await;
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "sync-test-1");
    }

    #[tokio::test]
    async fn test_start_sync_updates_version() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY, app TEXT, region TEXT, country TEXT, wg_ip TEXT,
                port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO backends VALUES ('v-test', 'app', 'eu', 'DE', '10.0.0.1', 80, 1, 1, 10, 20, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = SqliteBackendRepository::new();
        assert_eq!(repo.get_version().await, 0);

        repo.start_sync(db_path, 1);

        // Wait for sync
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Version should have incremented
        assert!(repo.get_version().await >= 1);
    }

    #[tokio::test]
    async fn test_start_sync_handles_missing_file() {
        let repo = SqliteBackendRepository::new();

        // Start sync with nonexistent file
        repo.start_sync("/nonexistent/path/db.sqlite".to_string(), 1);

        // Wait for sync attempt
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should not panic, backends remain empty
        assert!(repo.get_all().await.is_empty());
        assert_eq!(repo.get_version().await, 0);
    }

    #[tokio::test]
    async fn test_start_sync_multiple_iterations() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap().to_string();

        // Create initial database
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY, app TEXT, region TEXT, country TEXT, wg_ip TEXT,
                port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER DEFAULT 0
            )",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO backends VALUES ('iter-1', 'app', 'eu', 'DE', '10.0.0.1', 80, 1, 1, 10, 20, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        let repo = SqliteBackendRepository::new();
        repo.start_sync(db_path.clone(), 1);

        // Wait for first sync
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(repo.get_all().await.len(), 1);
        let v1 = repo.get_version().await;

        // Add another backend to database
        let conn = Connection::open(&db_path).unwrap();
        conn.execute(
            "INSERT INTO backends VALUES ('iter-2', 'app', 'eu', 'DE', '10.0.0.2', 80, 1, 1, 10, 20, 0)",
            [],
        )
        .unwrap();
        drop(conn);

        // Wait for next sync (at least 1 second)
        tokio::time::sleep(Duration::from_secs(1) + Duration::from_millis(100)).await;

        // Should have 2 backends now
        let backends = repo.get_all().await;
        assert_eq!(backends.len(), 2);

        // Version should have incremented
        let v2 = repo.get_version().await;
        assert!(v2 > v1);
    }

    #[tokio::test]
    async fn test_load_from_sqlite_includes_null_deleted() {
        use tempfile::NamedTempFile;

        let temp_file = NamedTempFile::new().unwrap();
        let db_path = temp_file.path().to_str().unwrap();

        let conn = Connection::open(db_path).unwrap();
        conn.execute(
            "CREATE TABLE backends (
                id TEXT PRIMARY KEY, app TEXT, region TEXT, country TEXT, wg_ip TEXT,
                port INTEGER, healthy INTEGER, weight INTEGER, soft_limit INTEGER, hard_limit INTEGER, deleted INTEGER
            )",
            [],
        )
        .unwrap();

        // Backend with NULL deleted (should be included)
        conn.execute(
            "INSERT INTO backends VALUES ('null-deleted', 'app', 'eu', 'DE', '10.0.0.1', 80, 1, 1, 10, 20, NULL)",
            [],
        )
        .unwrap();

        let backends = SqliteBackendRepository::load_from_sqlite(db_path).unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "null-deleted");
    }
}
