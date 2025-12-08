//! Backend Repository Port
//!
//! Defines the interface for accessing backend configuration.
//! Implementations may use SQLite, PostgreSQL, or in-memory storage.

use crate::domain::entities::Backend;
use async_trait::async_trait;

/// Repository for accessing backend configuration.
///
/// This is an outbound port that abstracts the storage mechanism
/// for backend definitions. The domain layer calls this interface
/// to retrieve backend information without knowing the storage details.
#[async_trait]
pub trait BackendRepository: Send + Sync {
    /// Get all configured backends (including unhealthy ones).
    #[allow(dead_code)]
    async fn get_all(&self) -> Vec<Backend>;

    /// Get a specific backend by ID.
    async fn get_by_id(&self, id: &str) -> Option<Backend>;

    /// Get all healthy backends available for routing.
    async fn get_healthy(&self) -> Vec<Backend>;

    /// Get the current version/revision of the backend list.
    /// Used to detect when backends have been updated.
    #[allow(dead_code)]
    async fn get_version(&self) -> u64;
}
