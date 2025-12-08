//! Connection Pool
//!
//! Maintains persistent connections to backends for reduced latency.

use dashmap::DashMap;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;

/// Connection pool configuration.
#[derive(Debug, Clone)]
pub struct PoolConfig {
    /// Maximum connections per backend
    pub max_connections: usize,
    /// Minimum idle connections to maintain
    pub min_idle: usize,
    /// Maximum time a connection can be idle
    pub idle_timeout: Duration,
    /// Maximum connection lifetime
    pub max_lifetime: Duration,
    /// Connection timeout
    pub connect_timeout: Duration,
}

impl Default for PoolConfig {
    fn default() -> Self {
        Self {
            max_connections: 10,
            min_idle: 2,
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(3600),
            connect_timeout: Duration::from_secs(5),
        }
    }
}

/// A pooled connection.
pub struct PooledConnection {
    /// The underlying TCP stream
    pub stream: TcpStream,
    /// When this connection was created
    created_at: Instant,
    /// When this connection was last used
    last_used: Instant,
    /// Backend ID this connection belongs to
    backend_id: String,
}

impl PooledConnection {
    fn new(stream: TcpStream, backend_id: String) -> Self {
        let now = Instant::now();
        Self {
            stream,
            created_at: now,
            last_used: now,
            backend_id,
        }
    }

    /// Check if this connection has exceeded its lifetime.
    pub fn is_expired(&self, max_lifetime: Duration) -> bool {
        self.created_at.elapsed() > max_lifetime
    }

    /// Check if this connection has been idle too long.
    pub fn is_idle_expired(&self, idle_timeout: Duration) -> bool {
        self.last_used.elapsed() > idle_timeout
    }

    /// Touch the connection to update last_used.
    pub fn touch(&mut self) {
        self.last_used = Instant::now();
    }
}

/// Per-backend connection pool.
struct BackendPool {
    /// Available connections
    connections: Mutex<VecDeque<PooledConnection>>,
    /// Number of connections currently in use
    in_use: AtomicUsize,
    /// Backend address
    addr: String,
}

impl BackendPool {
    fn new(addr: String) -> Self {
        Self {
            connections: Mutex::new(VecDeque::new()),
            in_use: AtomicUsize::new(0),
            addr,
        }
    }

    /// Get total connections (idle + in_use).
    fn total_connections(&self) -> usize {
        // Note: this is approximate due to async nature
        self.in_use.load(Ordering::Relaxed)
    }
}

/// Connection pool manager.
///
/// Maintains pools of persistent connections to backends.
pub struct ConnectionPool {
    config: PoolConfig,
    /// Per-backend pools
    pools: DashMap<String, Arc<BackendPool>>,
}

impl ConnectionPool {
    /// Create a new connection pool.
    pub fn new(config: PoolConfig) -> Self {
        Self {
            config,
            pools: DashMap::new(),
        }
    }

    /// Get or create a pool for a backend.
    fn get_or_create_pool(&self, backend_id: &str, addr: &str) -> Arc<BackendPool> {
        self.pools
            .entry(backend_id.to_string())
            .or_insert_with(|| Arc::new(BackendPool::new(addr.to_string())))
            .clone()
    }

    /// Acquire a connection from the pool or create a new one.
    pub async fn acquire(&self, backend_id: &str, addr: &str) -> Result<PooledConnection, PoolError> {
        let pool = self.get_or_create_pool(backend_id, addr);

        // Try to get an existing connection
        {
            let mut connections = pool.connections.lock().await;
            while let Some(mut conn) = connections.pop_front() {
                // Check if connection is still valid
                if conn.is_expired(self.config.max_lifetime) {
                    tracing::debug!("discarding expired connection to {}", backend_id);
                    let _ = conn.stream.shutdown().await;
                    continue;
                }
                if conn.is_idle_expired(self.config.idle_timeout) {
                    tracing::debug!("discarding idle connection to {}", backend_id);
                    let _ = conn.stream.shutdown().await;
                    continue;
                }

                // Connection is valid
                conn.touch();
                pool.in_use.fetch_add(1, Ordering::Relaxed);
                return Ok(conn);
            }
        }

        // Check if we can create a new connection
        let current = pool.total_connections();
        if current >= self.config.max_connections {
            return Err(PoolError::PoolExhausted);
        }

        // Create new connection
        let stream = match tokio::time::timeout(
            self.config.connect_timeout,
            TcpStream::connect(addr),
        )
        .await
        {
            Ok(Ok(stream)) => stream,
            Ok(Err(e)) => return Err(PoolError::ConnectError(e.to_string())),
            Err(_) => return Err(PoolError::ConnectTimeout),
        };

        pool.in_use.fetch_add(1, Ordering::Relaxed);
        Ok(PooledConnection::new(stream, backend_id.to_string()))
    }

    /// Release a connection back to the pool.
    pub async fn release(&self, mut conn: PooledConnection) {
        let backend_id = conn.backend_id.clone();

        if let Some(pool) = self.pools.get(&backend_id) {
            pool.in_use.fetch_sub(1, Ordering::Relaxed);

            // Check if connection is still valid
            if conn.is_expired(self.config.max_lifetime) {
                let _ = conn.stream.shutdown().await;
                return;
            }

            conn.touch();

            let mut connections = pool.connections.lock().await;
            if connections.len() < self.config.max_connections {
                connections.push_back(conn);
            } else {
                // Pool is full, close connection
                drop(conn);
            }
        }
    }

    /// Discard a connection (don't return to pool).
    pub async fn discard(&self, mut conn: PooledConnection) {
        let backend_id = conn.backend_id.clone();

        if let Some(pool) = self.pools.get(&backend_id) {
            pool.in_use.fetch_sub(1, Ordering::Relaxed);
        }

        let _ = conn.stream.shutdown().await;
    }

    /// Get pool statistics for a backend.
    pub async fn stats(&self, backend_id: &str) -> Option<PoolStats> {
        self.pools.get(backend_id).map(|pool| {
            PoolStats {
                in_use: pool.in_use.load(Ordering::Relaxed),
                addr: pool.addr.clone(),
            }
        })
    }

    /// Get all pool statistics.
    pub async fn all_stats(&self) -> Vec<(String, PoolStats)> {
        self.pools
            .iter()
            .map(|entry| {
                (
                    entry.key().clone(),
                    PoolStats {
                        in_use: entry.in_use.load(Ordering::Relaxed),
                        addr: entry.addr.clone(),
                    },
                )
            })
            .collect()
    }

    /// Start periodic cleanup of idle connections.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub fn start_cleanup(&self, interval: Duration)
    where
        Self: 'static,
    {
        let pools = self.pools.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);

            loop {
                ticker.tick().await;

                for entry in pools.iter() {
                    let pool = entry.value();
                    let mut connections = pool.connections.lock().await;
                    let before = connections.len();

                    connections.retain(|conn| {
                        !conn.is_idle_expired(config.idle_timeout)
                            && !conn.is_expired(config.max_lifetime)
                    });

                    let removed = before - connections.len();
                    if removed > 0 {
                        tracing::debug!(
                            "pool cleanup for {}: removed {} idle connections",
                            entry.key(),
                            removed
                        );
                    }
                }
            }
        });
    }

    /// Clear all pools.
    pub async fn clear(&self) {
        for entry in self.pools.iter() {
            let mut connections = entry.connections.lock().await;
            for mut conn in connections.drain(..) {
                let _ = conn.stream.shutdown().await;
            }
        }
        self.pools.clear();
    }
}

impl Default for ConnectionPool {
    fn default() -> Self {
        Self::new(PoolConfig::default())
    }
}

/// Pool statistics.
#[derive(Debug, Clone)]
pub struct PoolStats {
    /// Connections currently in use
    pub in_use: usize,
    /// Backend address
    pub addr: String,
}

/// Pool errors.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolError {
    /// Pool has no available connections
    PoolExhausted,
    /// Connection failed
    ConnectError(String),
    /// Connection timed out
    ConnectTimeout,
}

impl std::fmt::Display for PoolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PoolError::PoolExhausted => write!(f, "connection pool exhausted"),
            PoolError::ConnectError(e) => write!(f, "connection error: {}", e),
            PoolError::ConnectTimeout => write!(f, "connection timeout"),
        }
    }
}

impl std::error::Error for PoolError {}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[test]
    fn test_pool_config_default() {
        let config = PoolConfig::default();
        assert_eq!(config.max_connections, 10);
        assert_eq!(config.min_idle, 2);
        assert_eq!(config.idle_timeout, Duration::from_secs(300));
    }

    #[test]
    fn test_pool_error_display() {
        assert_eq!(PoolError::PoolExhausted.to_string(), "connection pool exhausted");
        assert_eq!(
            PoolError::ConnectError("test".to_string()).to_string(),
            "connection error: test"
        );
        assert_eq!(PoolError::ConnectTimeout.to_string(), "connection timeout");
    }

    #[tokio::test]
    async fn test_connection_pool_new() {
        let pool = ConnectionPool::new(PoolConfig::default());
        assert!(pool.pools.is_empty());
    }

    #[tokio::test]
    async fn test_connection_pool_default() {
        let pool = ConnectionPool::default();
        assert!(pool.pools.is_empty());
    }

    #[tokio::test]
    async fn test_acquire_creates_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let pool = ConnectionPool::new(PoolConfig::default());
        let conn = pool.acquire("b1", &addr.to_string()).await;

        assert!(conn.is_ok());
    }

    #[tokio::test]
    async fn test_acquire_connect_error() {
        let pool = ConnectionPool::new(PoolConfig {
            connect_timeout: Duration::from_millis(100),
            ..Default::default()
        });

        let result = pool.acquire("b1", "127.0.0.1:59999").await;
        assert!(matches!(result, Err(PoolError::ConnectError(_))));
    }

    #[tokio::test]
    async fn test_acquire_timeout() {
        let pool = ConnectionPool::new(PoolConfig {
            connect_timeout: Duration::from_millis(50),
            ..Default::default()
        });

        // Use non-routable IP to trigger timeout
        let result = pool.acquire("b1", "10.255.255.1:80").await;
        assert!(matches!(result, Err(PoolError::ConnectTimeout)));
    }

    #[tokio::test]
    async fn test_release_returns_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            loop {
                if listener.accept().await.is_err() {
                    break;
                }
            }
        });

        let pool = ConnectionPool::new(PoolConfig::default());

        let conn1 = pool.acquire("b1", &addr.to_string()).await.unwrap();
        pool.release(conn1).await;

        // Should reuse the released connection
        let conn2 = pool.acquire("b1", &addr.to_string()).await.unwrap();
        assert!(conn2.stream.peer_addr().is_ok());
    }

    #[tokio::test]
    async fn test_discard_closes_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let pool = ConnectionPool::new(PoolConfig::default());

        let conn = pool.acquire("b1", &addr.to_string()).await.unwrap();
        pool.discard(conn).await;

        // Pool should have no connections
        let stats = pool.stats("b1").await;
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().in_use, 0);
    }

    #[tokio::test]
    async fn test_stats() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let pool = ConnectionPool::new(PoolConfig::default());
        let _conn = pool.acquire("b1", &addr.to_string()).await.unwrap();

        let stats = pool.stats("b1").await;
        assert!(stats.is_some());
        assert_eq!(stats.unwrap().in_use, 1);
    }

    #[tokio::test]
    async fn test_stats_unknown() {
        let pool = ConnectionPool::default();
        assert!(pool.stats("unknown").await.is_none());
    }

    #[tokio::test]
    async fn test_all_stats() {
        let listener1 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr1 = listener1.local_addr().unwrap();

        let listener2 = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr2 = listener2.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener1.accept().await;
        });

        tokio::spawn(async move {
            let _ = listener2.accept().await;
        });

        let pool = ConnectionPool::new(PoolConfig::default());
        let _conn1 = pool.acquire("b1", &addr1.to_string()).await.unwrap();
        let _conn2 = pool.acquire("b2", &addr2.to_string()).await.unwrap();

        let stats = pool.all_stats().await;
        assert_eq!(stats.len(), 2);
    }

    #[tokio::test]
    async fn test_clear() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let pool = ConnectionPool::new(PoolConfig::default());
        let conn = pool.acquire("b1", &addr.to_string()).await.unwrap();
        pool.release(conn).await;

        pool.clear().await;
        assert!(pool.pools.is_empty());
    }

    #[tokio::test]
    async fn test_pooled_connection_is_expired() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let conn = PooledConnection::new(stream, "b1".to_string());

        assert!(!conn.is_expired(Duration::from_secs(60)));
        assert!(conn.is_expired(Duration::from_nanos(1)));
    }

    #[tokio::test]
    async fn test_pooled_connection_is_idle_expired() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let conn = PooledConnection::new(stream, "b1".to_string());

        assert!(!conn.is_idle_expired(Duration::from_secs(60)));

        std::thread::sleep(Duration::from_millis(10));
        assert!(conn.is_idle_expired(Duration::from_millis(1)));
    }

    #[tokio::test]
    async fn test_pooled_connection_touch() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        tokio::spawn(async move {
            let _ = listener.accept().await;
        });

        let stream = TcpStream::connect(addr).await.unwrap();
        let mut conn = PooledConnection::new(stream, "b1".to_string());

        std::thread::sleep(Duration::from_millis(10));
        conn.touch();

        assert!(!conn.is_idle_expired(Duration::from_millis(5)));
    }

    #[tokio::test]
    async fn test_pool_exhausted() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Accept multiple connections
        let listener = Arc::new(listener);
        let l = listener.clone();
        tokio::spawn(async move {
            loop {
                if l.accept().await.is_err() {
                    break;
                }
            }
        });

        let pool = ConnectionPool::new(PoolConfig {
            max_connections: 2,
            ..Default::default()
        });

        let _conn1 = pool.acquire("b1", &addr.to_string()).await.unwrap();
        let _conn2 = pool.acquire("b1", &addr.to_string()).await.unwrap();

        // Third acquisition should fail
        let result = pool.acquire("b1", &addr.to_string()).await;
        assert!(result.is_err());
        assert!(matches!(result, Err(PoolError::PoolExhausted)));
    }
}
