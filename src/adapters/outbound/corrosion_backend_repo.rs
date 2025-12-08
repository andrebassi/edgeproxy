//! Corrosion Backend Repository
//!
//! Implements BackendRepository using Corrosion's HTTP API for distributed SQLite.
//! Corrosion provides gossip-based replication across multiple nodes.
//!
//! See: https://github.com/superfly/corrosion

use crate::domain::entities::Backend;
use crate::domain::ports::BackendRepository;
use crate::domain::value_objects::RegionCode;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};

/// Response from Corrosion's query API.
#[derive(Debug, Deserialize)]
struct CorrosionQueryResponse {
    #[allow(dead_code)]
    columns: Vec<String>,
    rows: Vec<Vec<serde_json::Value>>,
}

/// Response from Corrosion's transaction API.
#[derive(Debug, Deserialize)]
struct CorrosionTransactionResponse {
    #[allow(dead_code)]
    results: Vec<TransactionResult>,
    #[allow(dead_code)]
    time: f64,
}

#[derive(Debug, Deserialize)]
struct TransactionResult {
    #[allow(dead_code)]
    rows_affected: i64,
    #[allow(dead_code)]
    time: f64,
}

/// Request body for Corrosion subscriptions.
/// Note: Subscriptions are planned for future implementation.
#[derive(Debug, Serialize)]
#[allow(dead_code)]
struct SubscriptionRequest {
    /// SQL query to subscribe to
    sql: String,
}

/// Configuration for Corrosion connection.
#[derive(Debug, Clone)]
pub struct CorrosionConfig {
    /// Base URL for Corrosion HTTP API (e.g., "http://localhost:8080")
    pub api_url: String,
    /// Polling interval in seconds (used as fallback if subscriptions fail)
    pub poll_interval_secs: u64,
}

impl Default for CorrosionConfig {
    fn default() -> Self {
        Self {
            api_url: "http://localhost:8080".to_string(),
            poll_interval_secs: 5,
        }
    }
}

/// Corrosion-backed backend repository.
///
/// Uses Corrosion's HTTP API to query the distributed SQLite database.
/// Changes made on any node propagate to all nodes via gossip protocol.
pub struct CorrosionBackendRepository {
    config: CorrosionConfig,
    client: reqwest::Client,
    backends: Arc<RwLock<Vec<Backend>>>,
    version: Arc<AtomicU64>,
}

impl CorrosionBackendRepository {
    /// Create a new Corrosion repository with the given configuration.
    pub fn new(config: CorrosionConfig) -> Self {
        Self {
            config,
            client: reqwest::Client::new(),
            backends: Arc::new(RwLock::new(Vec::new())),
            version: Arc::new(AtomicU64::new(0)),
        }
    }

    /// Start the background sync task.
    ///
    /// This spawns a Tokio task that periodically polls Corrosion for backend updates.
    /// In production, this could be replaced with HTTP streaming subscriptions.
    pub fn start_sync(&self) {
        let api_url = self.config.api_url.clone();
        let poll_interval = self.config.poll_interval_secs;
        let backends = self.backends.clone();
        let version = self.version.clone();
        let client = self.client.clone();

        tokio::spawn(async move {
            loop {
                match Self::fetch_backends(&client, &api_url).await {
                    Ok(new_backends) => {
                        let count = new_backends.len();
                        {
                            let mut guard = backends.write().await;
                            *guard = new_backends;
                        }
                        let new_version = version.fetch_add(1, Ordering::SeqCst) + 1;
                        tracing::info!(
                            "corrosion sync ok, version={} backends={}",
                            new_version,
                            count
                        );
                    }
                    Err(e) => {
                        tracing::error!("corrosion sync error: {:?}", e);
                    }
                }

                sleep(Duration::from_secs(poll_interval)).await;
            }
        });
    }

    /// Fetch backends from Corrosion's query API.
    async fn fetch_backends(
        client: &reqwest::Client,
        api_url: &str,
    ) -> anyhow::Result<Vec<Backend>> {
        let query = "SELECT id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit \
                     FROM backends \
                     WHERE deleted IS NULL OR deleted = 0";

        let url = format!("{}/v1/queries", api_url);
        let response = client
            .post(&url)
            .header("content-type", "application/json")
            .body(format!("[\"{}\"]", query.replace('"', "\\\"")))
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Corrosion query failed: {} - {}", status, body);
        }

        let data: CorrosionQueryResponse = response.json().await?;
        let backends = Self::parse_backends(data)?;

        Ok(backends)
    }

    /// Parse Corrosion query response into Backend entities.
    fn parse_backends(response: CorrosionQueryResponse) -> anyhow::Result<Vec<Backend>> {
        let mut backends = Vec::new();

        for row in response.rows {
            if row.len() < 10 {
                continue;
            }

            let backend = Backend {
                id: row[0].as_str().unwrap_or_default().to_string(),
                app: row[1].as_str().unwrap_or_default().to_string(),
                region: RegionCode::from_str(row[2].as_str().unwrap_or("us")),
                country: row[3].as_str().unwrap_or("US").to_string(),
                wg_ip: row[4].as_str().unwrap_or_default().to_string(),
                port: row[5].as_i64().unwrap_or(8080) as u16,
                healthy: row[6].as_i64().unwrap_or(0) != 0,
                weight: row[7].as_i64().unwrap_or(1) as u8,
                soft_limit: row[8].as_i64().unwrap_or(100) as u32,
                hard_limit: row[9].as_i64().unwrap_or(150) as u32,
            };

            backends.push(backend);
        }

        Ok(backends)
    }

    /// Execute a transaction on Corrosion (write operation).
    ///
    /// Changes are propagated to all nodes in the cluster via gossip.
    #[allow(dead_code)]
    pub async fn execute_transaction(&self, statements: Vec<String>) -> anyhow::Result<()> {
        let url = format!("{}/v1/transactions", self.config.api_url);

        let body = serde_json::to_string(&statements)?;

        let response = self
            .client
            .post(&url)
            .header("content-type", "application/json")
            .body(body)
            .send()
            .await?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            anyhow::bail!("Corrosion transaction failed: {} - {}", status, body);
        }

        let _result: CorrosionTransactionResponse = response.json().await?;
        Ok(())
    }

    /// Register a backend in Corrosion (for Auto-Discovery integration).
    #[allow(dead_code)]
    pub async fn register_backend(&self, backend: &Backend) -> anyhow::Result<()> {
        let sql = format!(
            "INSERT OR REPLACE INTO backends (id, app, region, country, wg_ip, port, healthy, weight, soft_limit, hard_limit, deleted) \
             VALUES ('{}', '{}', '{}', '{}', '{}', {}, {}, {}, {}, {}, 0)",
            backend.id,
            backend.app,
            backend.region.as_str(),
            backend.country,
            backend.wg_ip,
            backend.port,
            if backend.healthy { 1 } else { 0 },
            backend.weight,
            backend.soft_limit,
            backend.hard_limit
        );

        self.execute_transaction(vec![sql]).await
    }

    /// Mark a backend as deleted in Corrosion.
    #[allow(dead_code)]
    pub async fn deregister_backend(&self, id: &str) -> anyhow::Result<()> {
        let sql = format!("UPDATE backends SET deleted = 1 WHERE id = '{}'", id);
        self.execute_transaction(vec![sql]).await
    }

    /// Update backend health status.
    #[allow(dead_code)]
    pub async fn update_health(&self, id: &str, healthy: bool) -> anyhow::Result<()> {
        let sql = format!(
            "UPDATE backends SET healthy = {} WHERE id = '{}'",
            if healthy { 1 } else { 0 },
            id
        );
        self.execute_transaction(vec![sql]).await
    }
}

#[async_trait]
impl BackendRepository for CorrosionBackendRepository {
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

    #[test]
    fn test_corrosion_config_default() {
        let config = CorrosionConfig::default();
        assert_eq!(config.api_url, "http://localhost:8080");
        assert_eq!(config.poll_interval_secs, 5);
    }

    #[test]
    fn test_parse_backends_empty() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![],
        };
        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert!(backends.is_empty());
    }

    #[test]
    fn test_parse_backends_valid() {
        let response = CorrosionQueryResponse {
            columns: vec![
                "id".to_string(),
                "app".to_string(),
                "region".to_string(),
                "country".to_string(),
                "wg_ip".to_string(),
                "port".to_string(),
                "healthy".to_string(),
                "weight".to_string(),
                "soft_limit".to_string(),
                "hard_limit".to_string(),
            ],
            rows: vec![vec![
                serde_json::json!("backend-1"),
                serde_json::json!("myapp"),
                serde_json::json!("eu"),
                serde_json::json!("DE"),
                serde_json::json!("10.50.1.1"),
                serde_json::json!(8080),
                serde_json::json!(1),
                serde_json::json!(2),
                serde_json::json!(100),
                serde_json::json!(150),
            ]],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 1);

        let b = &backends[0];
        assert_eq!(b.id, "backend-1");
        assert_eq!(b.app, "myapp");
        assert_eq!(b.region, RegionCode::Europe);
        assert_eq!(b.country, "DE");
        assert_eq!(b.wg_ip, "10.50.1.1");
        assert_eq!(b.port, 8080);
        assert!(b.healthy);
        assert_eq!(b.weight, 2);
        assert_eq!(b.soft_limit, 100);
        assert_eq!(b.hard_limit, 150);
    }

    #[test]
    fn test_parse_backends_multiple() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![
                vec![
                    serde_json::json!("eu-1"),
                    serde_json::json!("app1"),
                    serde_json::json!("eu"),
                    serde_json::json!("DE"),
                    serde_json::json!("10.50.1.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
                vec![
                    serde_json::json!("us-1"),
                    serde_json::json!("app1"),
                    serde_json::json!("us"),
                    serde_json::json!("US"),
                    serde_json::json!("10.50.2.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),
                    serde_json::json!(3),
                    serde_json::json!(200),
                    serde_json::json!(300),
                ],
            ],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 2);
        assert_eq!(backends[0].id, "eu-1");
        assert_eq!(backends[1].id, "us-1");
    }

    #[test]
    fn test_parse_backends_skip_incomplete() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![
                // Valid row
                vec![
                    serde_json::json!("valid-1"),
                    serde_json::json!("app1"),
                    serde_json::json!("eu"),
                    serde_json::json!("DE"),
                    serde_json::json!("10.50.1.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
                // Incomplete row (should be skipped)
                vec![
                    serde_json::json!("incomplete"),
                    serde_json::json!("app1"),
                ],
            ],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "valid-1");
    }

    #[tokio::test]
    async fn test_repository_get_all_empty() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);
        let backends = repo.get_all().await;
        assert!(backends.is_empty());
    }

    #[tokio::test]
    async fn test_repository_get_version_initial() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);
        let version = repo.get_version().await;
        assert_eq!(version, 0);
    }

    #[tokio::test]
    async fn test_repository_get_by_id_not_found() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);
        let backend = repo.get_by_id("nonexistent").await;
        assert!(backend.is_none());
    }

    #[tokio::test]
    async fn test_repository_get_healthy_empty() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);
        let healthy = repo.get_healthy().await;
        assert!(healthy.is_empty());
    }

    #[test]
    fn test_corrosion_config_custom() {
        let config = CorrosionConfig {
            api_url: "http://10.0.0.1:9090".to_string(),
            poll_interval_secs: 30,
        };
        assert_eq!(config.api_url, "http://10.0.0.1:9090");
        assert_eq!(config.poll_interval_secs, 30);
    }

    #[test]
    fn test_corrosion_config_clone() {
        let config = CorrosionConfig::default();
        let cloned = config.clone();
        assert_eq!(config.api_url, cloned.api_url);
        assert_eq!(config.poll_interval_secs, cloned.poll_interval_secs);
    }

    #[test]
    fn test_corrosion_config_debug() {
        let config = CorrosionConfig::default();
        let debug_str = format!("{:?}", config);
        assert!(debug_str.contains("api_url"));
        assert!(debug_str.contains("localhost"));
    }

    #[test]
    fn test_parse_backends_with_unhealthy() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![
                vec![
                    serde_json::json!("healthy-1"),
                    serde_json::json!("app1"),
                    serde_json::json!("eu"),
                    serde_json::json!("DE"),
                    serde_json::json!("10.50.1.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),  // healthy
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
                vec![
                    serde_json::json!("unhealthy-1"),
                    serde_json::json!("app1"),
                    serde_json::json!("us"),
                    serde_json::json!("US"),
                    serde_json::json!("10.50.2.1"),
                    serde_json::json!(8080),
                    serde_json::json!(0),  // unhealthy
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
            ],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 2);
        assert!(backends[0].healthy);
        assert!(!backends[1].healthy);
    }

    #[test]
    fn test_parse_backends_with_null_values() {
        // Test behavior with null values that get default values
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![vec![
                serde_json::Value::Null,  // id
                serde_json::Value::Null,  // app
                serde_json::Value::Null,  // region
                serde_json::Value::Null,  // country
                serde_json::Value::Null,  // wg_ip
                serde_json::Value::Null,  // port
                serde_json::Value::Null,  // healthy
                serde_json::Value::Null,  // weight
                serde_json::Value::Null,  // soft_limit
                serde_json::Value::Null,  // hard_limit
            ]],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 1);
        // Should use defaults
        assert_eq!(backends[0].id, "");
        assert_eq!(backends[0].region, RegionCode::NorthAmerica);
        assert_eq!(backends[0].port, 8080);
        assert!(!backends[0].healthy);
    }

    #[test]
    fn test_parse_backends_all_regions() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![
                vec![
                    serde_json::json!("sa-1"),
                    serde_json::json!("app"),
                    serde_json::json!("sa"),
                    serde_json::json!("BR"),
                    serde_json::json!("10.1.1.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
                vec![
                    serde_json::json!("ap-1"),
                    serde_json::json!("app"),
                    serde_json::json!("ap"),
                    serde_json::json!("JP"),
                    serde_json::json!("10.2.1.1"),
                    serde_json::json!(8080),
                    serde_json::json!(1),
                    serde_json::json!(2),
                    serde_json::json!(100),
                    serde_json::json!(150),
                ],
            ],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 2);
        assert_eq!(backends[0].region, RegionCode::SouthAmerica);
        assert_eq!(backends[1].region, RegionCode::AsiaPacific);
    }

    #[test]
    fn test_parse_backends_with_various_weights() {
        let response = CorrosionQueryResponse {
            columns: vec![],
            rows: vec![vec![
                serde_json::json!("test-1"),
                serde_json::json!("app"),
                serde_json::json!("eu"),
                serde_json::json!("DE"),
                serde_json::json!("10.1.1.1"),
                serde_json::json!(9000),
                serde_json::json!(1),
                serde_json::json!(10),    // high weight
                serde_json::json!(500),   // high soft_limit
                serde_json::json!(1000),  // high hard_limit
            ]],
        };

        let backends = CorrosionBackendRepository::parse_backends(response).unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].port, 9000);
        assert_eq!(backends[0].weight, 10);
        assert_eq!(backends[0].soft_limit, 500);
        assert_eq!(backends[0].hard_limit, 1000);
    }

    #[tokio::test]
    async fn test_repository_with_preloaded_backends() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);

        // Simulate preloaded backends by writing directly
        {
            let mut guard = repo.backends.write().await;
            guard.push(Backend {
                id: "test-1".to_string(),
                app: "myapp".to_string(),
                region: RegionCode::Europe,
                country: "DE".to_string(),
                wg_ip: "10.50.1.1".to_string(),
                port: 8080,
                healthy: true,
                weight: 2,
                soft_limit: 100,
                hard_limit: 150,
            });
            guard.push(Backend {
                id: "test-2".to_string(),
                app: "myapp".to_string(),
                region: RegionCode::NorthAmerica,
                country: "US".to_string(),
                wg_ip: "10.50.2.1".to_string(),
                port: 8080,
                healthy: false,
                weight: 2,
                soft_limit: 100,
                hard_limit: 150,
            });
        }

        let all = repo.get_all().await;
        assert_eq!(all.len(), 2);

        let healthy = repo.get_healthy().await;
        assert_eq!(healthy.len(), 1);
        assert_eq!(healthy[0].id, "test-1");

        let found = repo.get_by_id("test-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "test-1");

        let not_found = repo.get_by_id("test-999").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_repository_version_increments() {
        let config = CorrosionConfig::default();
        let repo = CorrosionBackendRepository::new(config);

        assert_eq!(repo.get_version().await, 0);

        // Manually increment version
        repo.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(repo.get_version().await, 1);

        repo.version.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        assert_eq!(repo.get_version().await, 2);
    }

    // ===== Integration Tests with Mock HTTP Server =====

    #[tokio::test]
    async fn test_fetch_backends_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Setup mock response
        let response_body = serde_json::json!({
            "columns": ["id", "app", "region", "country", "wg_ip", "port", "healthy", "weight", "soft_limit", "hard_limit"],
            "rows": [
                ["mock-1", "mockapp", "eu", "DE", "10.100.1.1", 8080, 1, 2, 100, 150]
            ]
        });

        Mock::given(method("POST"))
            .and(path("/v1/queries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let result = CorrosionBackendRepository::fetch_backends(&client, &mock_server.uri()).await;

        assert!(result.is_ok());
        let backends = result.unwrap();
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "mock-1");
        assert_eq!(backends[0].app, "mockapp");
        assert_eq!(backends[0].region, RegionCode::Europe);
    }

    #[tokio::test]
    async fn test_fetch_backends_empty_response() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "columns": [],
            "rows": []
        });

        Mock::given(method("POST"))
            .and(path("/v1/queries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let result = CorrosionBackendRepository::fetch_backends(&client, &mock_server.uri()).await;

        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_fetch_backends_server_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/queries"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Internal Server Error"))
            .mount(&mock_server)
            .await;

        let client = reqwest::Client::new();
        let result = CorrosionBackendRepository::fetch_backends(&client, &mock_server.uri()).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("500"));
    }

    #[tokio::test]
    async fn test_execute_transaction_success() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "results": [{"rows_affected": 1, "time": 0.001}],
            "time": 0.002
        });

        Mock::given(method("POST"))
            .and(path("/v1/transactions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 5,
        };
        let repo = CorrosionBackendRepository::new(config);

        let result = repo
            .execute_transaction(vec!["INSERT INTO test VALUES (1)".to_string()])
            .await;

        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_execute_transaction_failure() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        Mock::given(method("POST"))
            .and(path("/v1/transactions"))
            .respond_with(ResponseTemplate::new(400).set_body_string("Bad Request"))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 5,
        };
        let repo = CorrosionBackendRepository::new(config);

        let result = repo
            .execute_transaction(vec!["INVALID SQL".to_string()])
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_register_backend() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "results": [{"rows_affected": 1, "time": 0.001}],
            "time": 0.002
        });

        Mock::given(method("POST"))
            .and(path("/v1/transactions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 5,
        };
        let repo = CorrosionBackendRepository::new(config);

        let backend = Backend {
            id: "new-backend".to_string(),
            app: "myapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.50.1.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let result = repo.register_backend(&backend).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_deregister_backend() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "results": [{"rows_affected": 1, "time": 0.001}],
            "time": 0.002
        });

        Mock::given(method("POST"))
            .and(path("/v1/transactions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 5,
        };
        let repo = CorrosionBackendRepository::new(config);

        let result = repo.deregister_backend("backend-to-delete").await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_update_health() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "results": [{"rows_affected": 1, "time": 0.001}],
            "time": 0.002
        });

        Mock::given(method("POST"))
            .and(path("/v1/transactions"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 5,
        };
        let repo = CorrosionBackendRepository::new(config);

        // Test marking healthy
        let result = repo.update_health("backend-1", true).await;
        assert!(result.is_ok());

        // Test marking unhealthy
        let result = repo.update_health("backend-1", false).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_start_sync_with_mock_server() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        let response_body = serde_json::json!({
            "columns": ["id", "app", "region", "country", "wg_ip", "port", "healthy", "weight", "soft_limit", "hard_limit"],
            "rows": [
                ["sync-mock-1", "syncapp", "us", "US", "10.200.1.1", 9000, 1, 3, 200, 300]
            ]
        });

        Mock::given(method("POST"))
            .and(path("/v1/queries"))
            .respond_with(ResponseTemplate::new(200).set_body_json(&response_body))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 1,
        };
        let repo = CorrosionBackendRepository::new(config);

        // Start sync
        repo.start_sync();

        // Wait for first sync
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should have loaded backends
        let backends = repo.get_all().await;
        assert_eq!(backends.len(), 1);
        assert_eq!(backends[0].id, "sync-mock-1");
        assert_eq!(backends[0].region, RegionCode::NorthAmerica);

        // Version should have incremented
        assert!(repo.get_version().await >= 1);
    }

    #[tokio::test]
    async fn test_start_sync_handles_error() {
        use wiremock::matchers::{method, path};
        use wiremock::{Mock, MockServer, ResponseTemplate};

        let mock_server = MockServer::start().await;

        // Return error response
        Mock::given(method("POST"))
            .and(path("/v1/queries"))
            .respond_with(ResponseTemplate::new(500).set_body_string("Server Error"))
            .mount(&mock_server)
            .await;

        let config = CorrosionConfig {
            api_url: mock_server.uri(),
            poll_interval_secs: 1,
        };
        let repo = CorrosionBackendRepository::new(config);

        repo.start_sync();

        // Wait for sync attempt
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Should not panic, backends remain empty
        assert!(repo.get_all().await.is_empty());
        // Version should still be 0 (no successful sync)
        assert_eq!(repo.get_version().await, 0);
    }
}
