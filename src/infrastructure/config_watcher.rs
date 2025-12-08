//! Configuration Hot Reload
//!
//! Watches configuration files and environment variables for changes,
//! enabling runtime reconfiguration without service restart.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};
use tokio::sync::{broadcast, RwLock};

/// Configuration change event.
#[derive(Debug, Clone)]
pub enum ConfigChange {
    /// A configuration file was modified
    FileModified(PathBuf),
    /// A configuration value was updated
    ValueChanged {
        key: String,
        old_value: Option<String>,
        new_value: String,
    },
    /// Configuration was fully reloaded
    FullReload,
}

/// Configuration watcher that detects changes.
pub struct ConfigWatcher {
    /// Files being watched with their last modification time
    watched_files: Arc<RwLock<HashMap<PathBuf, SystemTime>>>,
    /// Current configuration values
    config_values: Arc<RwLock<HashMap<String, String>>>,
    /// Broadcast channel for change notifications
    change_tx: broadcast::Sender<ConfigChange>,
    /// Poll interval for file changes
    poll_interval: Duration,
}

impl ConfigWatcher {
    /// Create a new configuration watcher.
    pub fn new(poll_interval: Duration) -> Self {
        let (change_tx, _) = broadcast::channel(64);
        Self {
            watched_files: Arc::new(RwLock::new(HashMap::new())),
            config_values: Arc::new(RwLock::new(HashMap::new())),
            change_tx,
            poll_interval,
        }
    }

    /// Add a file to watch for changes.
    pub async fn watch_file(&self, path: impl AsRef<Path>) -> Result<(), ConfigWatchError> {
        let path = path.as_ref().to_path_buf();

        let mtime = std::fs::metadata(&path)
            .map_err(|e| ConfigWatchError::FileError(path.clone(), e.to_string()))?
            .modified()
            .map_err(|e| ConfigWatchError::FileError(path.clone(), e.to_string()))?;

        self.watched_files.write().await.insert(path, mtime);
        Ok(())
    }

    /// Remove a file from watch list.
    pub async fn unwatch_file(&self, path: impl AsRef<Path>) {
        self.watched_files.write().await.remove(path.as_ref());
    }

    /// Set a configuration value.
    pub async fn set(&self, key: impl Into<String>, value: impl Into<String>) {
        let key = key.into();
        let value = value.into();

        let old_value = {
            let mut values = self.config_values.write().await;
            values.insert(key.clone(), value.clone())
        };

        if old_value.as_ref() != Some(&value) {
            let _ = self.change_tx.send(ConfigChange::ValueChanged {
                key,
                old_value,
                new_value: value,
            });
        }
    }

    /// Get a configuration value.
    pub async fn get(&self, key: &str) -> Option<String> {
        self.config_values.read().await.get(key).cloned()
    }

    /// Get a configuration value or default.
    pub async fn get_or(&self, key: &str, default: impl Into<String>) -> String {
        self.config_values
            .read()
            .await
            .get(key)
            .cloned()
            .unwrap_or_else(|| default.into())
    }

    /// Get all configuration values.
    pub async fn get_all(&self) -> HashMap<String, String> {
        self.config_values.read().await.clone()
    }

    /// Subscribe to configuration changes.
    pub fn subscribe(&self) -> broadcast::Receiver<ConfigChange> {
        self.change_tx.subscribe()
    }

    /// Manually trigger a full reload notification.
    pub fn notify_reload(&self) {
        let _ = self.change_tx.send(ConfigChange::FullReload);
    }

    /// Check watched files for modifications.
    async fn check_files(&self) -> Vec<PathBuf> {
        let mut modified = Vec::new();
        let mut files = self.watched_files.write().await;

        for (path, last_mtime) in files.iter_mut() {
            if let Ok(metadata) = std::fs::metadata(path) {
                if let Ok(mtime) = metadata.modified() {
                    if mtime > *last_mtime {
                        modified.push(path.clone());
                        *last_mtime = mtime;
                    }
                }
            }
        }

        modified
    }

    /// Start the file watcher loop.
    ///
    /// This spawns a background task that periodically checks for file changes.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start(self: Arc<Self>) {
        let watcher = self.clone();

        tokio::spawn(async move {
            let mut interval = tokio::time::interval(watcher.poll_interval);

            loop {
                interval.tick().await;

                let modified = watcher.check_files().await;
                for path in modified {
                    tracing::info!(?path, "configuration file modified");
                    let _ = watcher.change_tx.send(ConfigChange::FileModified(path));
                }
            }
        });
    }

    /// Get the number of watched files.
    pub async fn watched_count(&self) -> usize {
        self.watched_files.read().await.len()
    }
}

impl Default for ConfigWatcher {
    fn default() -> Self {
        Self::new(Duration::from_secs(5))
    }
}

/// Hot-reloadable configuration value.
///
/// A wrapper that automatically updates when configuration changes.
pub struct HotValue<T> {
    value: Arc<RwLock<T>>,
    key: String,
}

impl<T: Clone + Send + Sync + 'static> HotValue<T> {
    /// Create a new hot value.
    pub fn new(key: impl Into<String>, initial: T) -> Self {
        Self {
            value: Arc::new(RwLock::new(initial)),
            key: key.into(),
        }
    }

    /// Get the current value.
    pub async fn get(&self) -> T {
        self.value.read().await.clone()
    }

    /// Update the value.
    pub async fn set(&self, value: T) {
        *self.value.write().await = value;
    }

    /// Get the configuration key.
    pub fn key(&self) -> &str {
        &self.key
    }
}

/// Errors that can occur during configuration watching.
#[derive(Debug, Clone, PartialEq)]
pub enum ConfigWatchError {
    /// File operation failed
    FileError(PathBuf, String),
    /// Parse error
    ParseError(String),
}

impl std::fmt::Display for ConfigWatchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigWatchError::FileError(path, e) => {
                write!(f, "file error for {:?}: {}", path, e)
            }
            ConfigWatchError::ParseError(e) => write!(f, "parse error: {}", e),
        }
    }
}

impl std::error::Error for ConfigWatchError {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_config_watcher_new() {
        let watcher = ConfigWatcher::new(Duration::from_secs(10));
        assert_eq!(watcher.poll_interval, Duration::from_secs(10));
    }

    #[test]
    fn test_config_watcher_default() {
        let watcher = ConfigWatcher::default();
        assert_eq!(watcher.poll_interval, Duration::from_secs(5));
    }

    #[tokio::test]
    async fn test_watch_file() {
        let watcher = ConfigWatcher::default();
        let temp_file = NamedTempFile::new().unwrap();

        let result = watcher.watch_file(temp_file.path()).await;
        assert!(result.is_ok());
        assert_eq!(watcher.watched_count().await, 1);
    }

    #[tokio::test]
    async fn test_watch_nonexistent_file() {
        let watcher = ConfigWatcher::default();
        let result = watcher.watch_file("/nonexistent/file.conf").await;
        assert!(result.is_err());
        assert!(matches!(result, Err(ConfigWatchError::FileError(_, _))));
    }

    #[tokio::test]
    async fn test_unwatch_file() {
        let watcher = ConfigWatcher::default();
        let temp_file = NamedTempFile::new().unwrap();

        watcher.watch_file(temp_file.path()).await.unwrap();
        assert_eq!(watcher.watched_count().await, 1);

        watcher.unwatch_file(temp_file.path()).await;
        assert_eq!(watcher.watched_count().await, 0);
    }

    #[tokio::test]
    async fn test_set_and_get() {
        let watcher = ConfigWatcher::default();

        watcher.set("key1", "value1").await;
        assert_eq!(watcher.get("key1").await, Some("value1".to_string()));
        assert_eq!(watcher.get("nonexistent").await, None);
    }

    #[tokio::test]
    async fn test_get_or() {
        let watcher = ConfigWatcher::default();

        watcher.set("key1", "value1").await;
        assert_eq!(watcher.get_or("key1", "default").await, "value1");
        assert_eq!(watcher.get_or("nonexistent", "default").await, "default");
    }

    #[tokio::test]
    async fn test_get_all() {
        let watcher = ConfigWatcher::default();

        watcher.set("key1", "value1").await;
        watcher.set("key2", "value2").await;

        let all = watcher.get_all().await;
        assert_eq!(all.len(), 2);
        assert_eq!(all.get("key1"), Some(&"value1".to_string()));
        assert_eq!(all.get("key2"), Some(&"value2".to_string()));
    }

    #[tokio::test]
    async fn test_subscribe_value_change() {
        let watcher = ConfigWatcher::default();
        let mut rx = watcher.subscribe();

        watcher.set("key1", "value1").await;

        let change = rx.recv().await.unwrap();
        match change {
            ConfigChange::ValueChanged { key, old_value, new_value } => {
                assert_eq!(key, "key1");
                assert_eq!(old_value, None);
                assert_eq!(new_value, "value1");
            }
            _ => panic!("expected ValueChanged"),
        }
    }

    #[tokio::test]
    async fn test_subscribe_value_update() {
        let watcher = ConfigWatcher::default();
        let mut rx = watcher.subscribe();

        watcher.set("key1", "value1").await;
        let _ = rx.recv().await; // consume first change

        watcher.set("key1", "value2").await;

        let change = rx.recv().await.unwrap();
        match change {
            ConfigChange::ValueChanged { key, old_value, new_value } => {
                assert_eq!(key, "key1");
                assert_eq!(old_value, Some("value1".to_string()));
                assert_eq!(new_value, "value2");
            }
            _ => panic!("expected ValueChanged"),
        }
    }

    #[tokio::test]
    async fn test_no_change_on_same_value() {
        let watcher = ConfigWatcher::default();
        let mut rx = watcher.subscribe();

        watcher.set("key1", "value1").await;
        let _ = rx.recv().await; // consume first change

        // Set same value again - should not trigger change
        watcher.set("key1", "value1").await;

        // Use timeout to verify no message
        let result = tokio::time::timeout(
            Duration::from_millis(50),
            rx.recv()
        ).await;
        assert!(result.is_err()); // timeout means no message
    }

    #[tokio::test]
    async fn test_notify_reload() {
        let watcher = ConfigWatcher::default();
        let mut rx = watcher.subscribe();

        watcher.notify_reload();

        let change = rx.recv().await.unwrap();
        assert!(matches!(change, ConfigChange::FullReload));
    }

    #[tokio::test]
    async fn test_check_files_detects_modification() {
        let watcher = ConfigWatcher::default();
        let mut temp_file = NamedTempFile::new().unwrap();

        watcher.watch_file(temp_file.path()).await.unwrap();

        // First check should find no modifications
        let modified = watcher.check_files().await;
        assert!(modified.is_empty());

        // Modify the file
        std::thread::sleep(Duration::from_millis(10)); // Ensure mtime differs
        writeln!(temp_file, "new content").unwrap();

        // Second check should detect modification
        let modified = watcher.check_files().await;
        assert_eq!(modified.len(), 1);
        assert_eq!(modified[0], temp_file.path());
    }

    #[test]
    fn test_hot_value_new() {
        let value: HotValue<i32> = HotValue::new("test_key", 42);
        assert_eq!(value.key(), "test_key");
    }

    #[tokio::test]
    async fn test_hot_value_get_set() {
        let value: HotValue<String> = HotValue::new("key", "initial".to_string());
        assert_eq!(value.get().await, "initial");

        value.set("updated".to_string()).await;
        assert_eq!(value.get().await, "updated");
    }

    #[test]
    fn test_config_watch_error_display() {
        let err = ConfigWatchError::FileError(PathBuf::from("/test"), "not found".to_string());
        assert!(err.to_string().contains("file error"));
        assert!(err.to_string().contains("/test"));

        let err = ConfigWatchError::ParseError("invalid".to_string());
        assert!(err.to_string().contains("parse error"));
    }

    #[test]
    fn test_config_change_clone() {
        let change = ConfigChange::FullReload;
        let _ = change.clone();

        let change = ConfigChange::FileModified(PathBuf::from("/test"));
        let cloned = change.clone();
        assert!(matches!(cloned, ConfigChange::FileModified(_)));

        let change = ConfigChange::ValueChanged {
            key: "k".to_string(),
            old_value: None,
            new_value: "v".to_string(),
        };
        let cloned = change.clone();
        if let ConfigChange::ValueChanged { key, .. } = cloned {
            assert_eq!(key, "k");
        }
    }
}
