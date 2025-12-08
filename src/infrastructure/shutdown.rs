//! Graceful Shutdown Handler
//!
//! Provides coordinated shutdown for all server components.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::sync::Notify;

/// Shutdown coordinator for graceful termination.
///
/// Tracks active connections and signals shutdown to all components.
#[derive(Clone)]
pub struct ShutdownController {
    /// Whether shutdown has been initiated
    shutdown_initiated: Arc<AtomicBool>,
    /// Number of active connections
    active_connections: Arc<AtomicUsize>,
    /// Broadcast channel for shutdown signal
    shutdown_tx: broadcast::Sender<()>,
    /// Notify when all connections are drained
    drain_complete: Arc<Notify>,
}

impl ShutdownController {
    /// Create a new shutdown controller.
    pub fn new() -> Self {
        let (shutdown_tx, _) = broadcast::channel(1);
        Self {
            shutdown_initiated: Arc::new(AtomicBool::new(false)),
            active_connections: Arc::new(AtomicUsize::new(0)),
            shutdown_tx,
            drain_complete: Arc::new(Notify::new()),
        }
    }

    /// Subscribe to shutdown notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<()> {
        self.shutdown_tx.subscribe()
    }

    /// Initiate graceful shutdown.
    pub fn shutdown(&self) {
        if !self.shutdown_initiated.swap(true, Ordering::SeqCst) {
            tracing::info!("initiating graceful shutdown");
            let _ = self.shutdown_tx.send(());
        }
    }

    /// Check if shutdown has been initiated.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown_initiated.load(Ordering::SeqCst)
    }

    /// Get the number of active connections.
    pub fn active_connections(&self) -> usize {
        self.active_connections.load(Ordering::SeqCst)
    }

    /// Increment active connection count.
    pub fn connection_started(&self) {
        self.active_connections.fetch_add(1, Ordering::SeqCst);
    }

    /// Decrement active connection count and notify if drained.
    pub fn connection_ended(&self) {
        let prev = self.active_connections.fetch_sub(1, Ordering::SeqCst);
        if prev == 1 && self.is_shutdown() {
            self.drain_complete.notify_waiters();
        }
    }

    /// Wait for all connections to drain (with timeout).
    pub async fn wait_for_drain(&self, timeout: Duration) -> bool {
        if self.active_connections() == 0 {
            return true;
        }

        tokio::select! {
            _ = self.drain_complete.notified() => true,
            _ = tokio::time::sleep(timeout) => {
                tracing::warn!(
                    "drain timeout: {} connections still active",
                    self.active_connections()
                );
                false
            }
        }
    }

    /// Create a connection guard that auto-decrements on drop.
    pub fn connection_guard(&self) -> ConnectionGuard {
        self.connection_started();
        ConnectionGuard {
            controller: self.clone(),
        }
    }
}

impl Default for ShutdownController {
    fn default() -> Self {
        Self::new()
    }
}

/// RAII guard for tracking active connections.
///
/// Automatically decrements the connection count when dropped.
pub struct ConnectionGuard {
    controller: ShutdownController,
}

impl Drop for ConnectionGuard {
    fn drop(&mut self) {
        self.controller.connection_ended();
    }
}

/// Install signal handlers for graceful shutdown.
///
/// Returns a future that completes when a shutdown signal is received.
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn shutdown_signal(controller: ShutdownController) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            tracing::info!("received Ctrl+C, initiating shutdown");
        }
        _ = terminate => {
            tracing::info!("received SIGTERM, initiating shutdown");
        }
    }

    controller.shutdown();
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_shutdown_controller_new() {
        let controller = ShutdownController::new();
        assert!(!controller.is_shutdown());
        assert_eq!(controller.active_connections(), 0);
    }

    #[test]
    fn test_shutdown_controller_default() {
        let controller = ShutdownController::default();
        assert!(!controller.is_shutdown());
    }

    #[test]
    fn test_shutdown_initiates_once() {
        let controller = ShutdownController::new();

        controller.shutdown();
        assert!(controller.is_shutdown());

        // Calling again should be idempotent
        controller.shutdown();
        assert!(controller.is_shutdown());
    }

    #[test]
    fn test_connection_tracking() {
        let controller = ShutdownController::new();

        controller.connection_started();
        assert_eq!(controller.active_connections(), 1);

        controller.connection_started();
        assert_eq!(controller.active_connections(), 2);

        controller.connection_ended();
        assert_eq!(controller.active_connections(), 1);

        controller.connection_ended();
        assert_eq!(controller.active_connections(), 0);
    }

    #[test]
    fn test_connection_guard() {
        let controller = ShutdownController::new();
        assert_eq!(controller.active_connections(), 0);

        {
            let _guard = controller.connection_guard();
            assert_eq!(controller.active_connections(), 1);
        }

        assert_eq!(controller.active_connections(), 0);
    }

    #[test]
    fn test_multiple_connection_guards() {
        let controller = ShutdownController::new();

        let guard1 = controller.connection_guard();
        let guard2 = controller.connection_guard();
        let guard3 = controller.connection_guard();

        assert_eq!(controller.active_connections(), 3);

        drop(guard1);
        assert_eq!(controller.active_connections(), 2);

        drop(guard2);
        drop(guard3);
        assert_eq!(controller.active_connections(), 0);
    }

    #[tokio::test]
    async fn test_subscribe_receives_shutdown() {
        let controller = ShutdownController::new();
        let mut rx = controller.subscribe();

        controller.shutdown();

        let result = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_wait_for_drain_immediate() {
        let controller = ShutdownController::new();
        controller.shutdown();

        let drained = controller.wait_for_drain(Duration::from_millis(100)).await;
        assert!(drained);
    }

    #[tokio::test]
    async fn test_wait_for_drain_with_connections() {
        let controller = ShutdownController::new();
        let guard = controller.connection_guard();
        controller.shutdown();

        // Spawn task to drop guard after a delay
        let ctrl = controller.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            drop(guard);
        });

        let drained = ctrl.wait_for_drain(Duration::from_millis(200)).await;
        assert!(drained);
    }

    #[tokio::test]
    async fn test_wait_for_drain_timeout() {
        let controller = ShutdownController::new();
        let _guard = controller.connection_guard();
        controller.shutdown();

        let drained = controller.wait_for_drain(Duration::from_millis(50)).await;
        assert!(!drained);
    }

    #[test]
    fn test_clone() {
        let controller = ShutdownController::new();
        let cloned = controller.clone();

        controller.connection_started();
        assert_eq!(cloned.active_connections(), 1);

        cloned.shutdown();
        assert!(controller.is_shutdown());
    }
}
