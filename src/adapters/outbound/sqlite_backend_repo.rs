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
    /// from the SQLite database file.
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
    fn load_from_sqlite(db_path: &str) -> Result<Vec<Backend>> {
        let conn = Connection::open(db_path)?;

        let mut stmt = conn.prepare(
            "SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit
             FROM backends
             WHERE deleted IS NULL OR deleted = 0",
        )?;

        let backends = stmt
            .query_map([], |row| Self::row_to_backend(row))?
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
