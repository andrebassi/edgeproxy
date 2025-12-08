//! Binding Repository Port
//!
//! Defines the interface for managing client-to-backend bindings.
//! Bindings provide session affinity (sticky sessions).

use crate::domain::entities::{Binding, ClientKey};
use async_trait::async_trait;
use std::time::Duration;

/// Repository for managing client-to-backend bindings.
///
/// Bindings ensure that once a client is assigned to a backend,
/// subsequent connections from the same client go to the same backend.
/// This is important for stateful applications and gaming servers.
#[async_trait]
pub trait BindingRepository: Send + Sync {
    /// Get the binding for a client, if one exists.
    async fn get(&self, key: &ClientKey) -> Option<Binding>;

    /// Create or update a binding for a client.
    async fn set(&self, key: ClientKey, binding: Binding);

    /// Remove a binding for a client.
    async fn remove(&self, key: &ClientKey);

    /// Update the last_seen timestamp for a binding.
    /// Called when a client makes a new connection.
    async fn touch(&self, key: &ClientKey);

    /// Remove all bindings that have not been seen within the TTL.
    #[allow(dead_code)]
    async fn cleanup_expired(&self, ttl: Duration) -> usize;

    /// Get the total number of active bindings.
    #[allow(dead_code)]
    async fn count(&self) -> usize;
}
