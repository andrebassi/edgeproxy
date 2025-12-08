//! Integration tests for Health Check with Wiremock
//!
//! Tests HTTP health checking using mock servers.

use std::time::Duration;
use wiremock::{MockServer, Mock, ResponseTemplate};
use wiremock::matchers::{method, path, header};

/// Test basic HTTP GET health check
#[tokio::test]
async fn test_http_health_check_success() {
    // Start mock server
    let mock_server = MockServer::start().await;

    // Configure mock response
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .expect(1)
        .mount(&mock_server)
        .await;

    // Make request
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);
    assert_eq!(resp.text().await.unwrap(), "OK");
}

/// Test health check with JSON response
#[tokio::test]
async fn test_http_health_check_json() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(serde_json::json!({
                    "status": "healthy",
                    "version": "1.0.0",
                    "uptime_secs": 3600
                }))
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 200);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["status"], "healthy");
    assert_eq!(body["version"], "1.0.0");
}

/// Test health check failure (unhealthy backend)
#[tokio::test]
async fn test_http_health_check_unhealthy() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503).set_body_string("Service Unavailable"))
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .get(format!("{}/health", mock_server.uri()))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 503);
}

/// Test health check timeout
#[tokio::test]
async fn test_http_health_check_timeout() {
    let mock_server = MockServer::start().await;

    // Mock with delay longer than client timeout
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_delay(Duration::from_secs(5))
        )
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::builder()
        .timeout(Duration::from_millis(100))
        .build()
        .unwrap();

    let result = client
        .get(format!("{}/health", mock_server.uri()))
        .send()
        .await;

    // Should timeout
    assert!(result.is_err());
}

/// Test multiple health check endpoints
#[tokio::test]
async fn test_multiple_health_endpoints() {
    let mock_server = MockServer::start().await;

    // /health endpoint
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    // /ready endpoint
    Mock::given(method("GET"))
        .and(path("/ready"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Ready"))
        .mount(&mock_server)
        .await;

    // /live endpoint
    Mock::given(method("GET"))
        .and(path("/live"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Alive"))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Check all endpoints
    let health = client.get(format!("{}/health", mock_server.uri())).send().await.unwrap();
    assert_eq!(health.status(), 200);

    let ready = client.get(format!("{}/ready", mock_server.uri())).send().await.unwrap();
    assert_eq!(ready.status(), 200);

    let live = client.get(format!("{}/live", mock_server.uri())).send().await.unwrap();
    assert_eq!(live.status(), 200);
}

/// Test health check with custom headers
#[tokio::test]
async fn test_health_check_with_headers() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .and(header("X-Health-Token", "secret123"))
        .respond_with(ResponseTemplate::new(200).set_body_string("Authenticated"))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Without header - should fail (404 since no matching mock)
    let resp_no_header = client
        .get(format!("{}/health", mock_server.uri()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp_no_header.status(), 404);

    // With header - should succeed
    let resp_with_header = client
        .get(format!("{}/health", mock_server.uri()))
        .header("X-Health-Token", "secret123")
        .send()
        .await
        .unwrap();
    assert_eq!(resp_with_header.status(), 200);
}

/// Test health check retries
#[tokio::test]
async fn test_health_check_retry_success() {
    let mock_server = MockServer::start().await;

    // First two calls fail, third succeeds
    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(503))
        .up_to_n_times(2)
        .mount(&mock_server)
        .await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("OK"))
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // First attempt - fails
    let resp1 = client.get(format!("{}/health", mock_server.uri())).send().await.unwrap();
    assert_eq!(resp1.status(), 503);

    // Second attempt - fails
    let resp2 = client.get(format!("{}/health", mock_server.uri())).send().await.unwrap();
    assert_eq!(resp2.status(), 503);

    // Third attempt - succeeds
    let resp3 = client.get(format!("{}/health", mock_server.uri())).send().await.unwrap();
    assert_eq!(resp3.status(), 200);
}

/// Test concurrent health checks to multiple backends
#[tokio::test]
async fn test_concurrent_health_checks() {
    // Create 3 mock servers (simulating 3 backends)
    let backend1 = MockServer::start().await;
    let backend2 = MockServer::start().await;
    let backend3 = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"backend": 1})))
        .mount(&backend1)
        .await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"backend": 2})))
        .mount(&backend2)
        .await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"backend": 3})))
        .mount(&backend3)
        .await;

    let client = reqwest::Client::new();
    let urls = vec![
        format!("{}/health", backend1.uri()),
        format!("{}/health", backend2.uri()),
        format!("{}/health", backend3.uri()),
    ];

    // Check all concurrently
    let futures: Vec<_> = urls.iter().map(|url| client.get(url).send()).collect();
    let results = futures::future::join_all(futures).await;

    // All should succeed
    for result in results {
        let resp = result.unwrap();
        assert_eq!(resp.status(), 200);
    }
}

/// Test health check with varying response times
#[tokio::test]
async fn test_health_check_response_times() {
    let fast_backend = MockServer::start().await;
    let slow_backend = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200).set_body_string("fast"))
        .mount(&fast_backend)
        .await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_string("slow")
                .set_delay(Duration::from_millis(200))
        )
        .mount(&slow_backend)
        .await;

    let client = reqwest::Client::new();

    let start = std::time::Instant::now();
    let _ = client.get(format!("{}/health", fast_backend.uri())).send().await.unwrap();
    let fast_duration = start.elapsed();

    let start = std::time::Instant::now();
    let _ = client.get(format!("{}/health", slow_backend.uri())).send().await.unwrap();
    let slow_duration = start.elapsed();

    // Slow backend should take noticeably longer
    assert!(slow_duration > fast_duration);
    assert!(slow_duration >= Duration::from_millis(200));
}

/// Test health check expectation verification
#[tokio::test]
async fn test_health_check_call_count() {
    let mock_server = MockServer::start().await;

    Mock::given(method("GET"))
        .and(path("/health"))
        .respond_with(ResponseTemplate::new(200))
        .expect(3)  // Expect exactly 3 calls
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Make exactly 3 calls
    for _ in 0..3 {
        client.get(format!("{}/health", mock_server.uri())).send().await.unwrap();
    }

    // Mock server will verify expectations on drop
}

/// Test backend registration API mock
#[tokio::test]
async fn test_backend_registration_api() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/register"))
        .respond_with(
            ResponseTemplate::new(201)
                .set_body_json(serde_json::json!({
                    "id": "backend-1",
                    "status": "registered"
                }))
        )
        .expect(1)
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{}/api/v1/register", mock_server.uri()))
        .json(&serde_json::json!({
            "id": "backend-1",
            "app": "myapp",
            "region": "sa",
            "ip": "10.0.0.1",
            "port": 8080
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(resp.status(), 201);

    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["id"], "backend-1");
    assert_eq!(body["status"], "registered");
}

/// Test heartbeat API mock
#[tokio::test]
async fn test_heartbeat_api() {
    let mock_server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/api/v1/heartbeat/backend-1"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({"status": "ok"})))
        .expect(5)  // Expect 5 heartbeats
        .mount(&mock_server)
        .await;

    let client = reqwest::Client::new();

    // Simulate 5 heartbeats
    for _ in 0..5 {
        let resp = client
            .post(format!("{}/api/v1/heartbeat/backend-1", mock_server.uri()))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
    }
}
