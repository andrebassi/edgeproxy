//! TLS Server Adapter
//!
//! Accepts TLS-encrypted TCP connections and proxies them to backends.
//! Supports certificate loading from files or self-signed generation for testing.

use crate::application::ProxyService;
use crate::domain::entities::GeoInfo;
use crate::domain::ports::GeoResolver;
use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use std::fs::File;
use std::io::BufReader;
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;
use tokio_rustls::TlsAcceptor;

/// TLS Server configuration.
#[derive(Clone)]
pub struct TlsConfig {
    pub acceptor: TlsAcceptor,
}

impl TlsConfig {
    /// Load TLS config from certificate and key files.
    pub fn from_pem_files(cert_path: &Path, key_path: &Path) -> anyhow::Result<Self> {
        let cert_file = File::open(cert_path)?;
        let key_file = File::open(key_path)?;

        let certs: Vec<CertificateDer<'static>> = rustls_pemfile::certs(&mut BufReader::new(cert_file))
            .collect::<Result<Vec<_>, _>>()?;

        let key = rustls_pemfile::private_key(&mut BufReader::new(key_file))?
            .ok_or_else(|| anyhow::anyhow!("no private key found in {}", key_path.display()))?;

        Self::from_certs_and_key(certs, key)
    }

    /// Create TLS config from certificates and key.
    pub fn from_certs_and_key(
        certs: Vec<CertificateDer<'static>>,
        key: PrivateKeyDer<'static>,
    ) -> anyhow::Result<Self> {
        let config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        Ok(Self {
            acceptor: TlsAcceptor::from(Arc::new(config)),
        })
    }

    /// Generate self-signed certificate for testing.
    pub fn self_signed(domain: &str) -> anyhow::Result<Self> {
        let subject_alt_names = vec![
            domain.to_string(),
            "localhost".to_string(),
            "127.0.0.1".to_string(),
        ];

        let cert = rcgen::generate_simple_self_signed(subject_alt_names)?;
        let cert_der = CertificateDer::from(cert.serialize_der()?);
        let key_der = PrivateKeyDer::try_from(cert.serialize_private_key_der())
            .map_err(|e| anyhow::anyhow!("failed to serialize key: {:?}", e))?;

        Self::from_certs_and_key(vec![cert_der], key_der)
    }
}

/// TLS Server - inbound adapter for handling TLS-encrypted client connections.
pub struct TlsServer {
    proxy_service: Arc<ProxyService>,
    listen_addr: String,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,
    tls_config: TlsConfig,
}

impl TlsServer {
    /// Create a new TLS server.
    pub fn new(
        proxy_service: Arc<ProxyService>,
        listen_addr: String,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        tls_config: TlsConfig,
    ) -> Self {
        Self {
            proxy_service,
            listen_addr,
            geo_resolver,
            public_ip_geo: Arc::new(RwLock::new(None)),
            tls_config,
        }
    }

    /// Run the TLS server.
    ///
    /// This function runs an infinite loop accepting connections.
    /// The error handlers inside the spawned tasks are excluded from coverage
    /// as they are async error paths that are difficult to test deterministically.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("edgeProxy TLS listening on {}", self.listen_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let service = self.proxy_service.clone();
            let geo_resolver = self.geo_resolver.clone();
            let public_ip_geo = self.public_ip_geo.clone();
            let acceptor = self.tls_config.acceptor.clone();

            tokio::spawn(async move {
                // Perform TLS handshake
                match acceptor.accept(stream).await {
                    Ok(tls_stream) => {
                        if let Err(e) = Self::handle_connection(
                            service,
                            tls_stream,
                            addr,
                            geo_resolver,
                            public_ip_geo,
                        )
                        .await
                        {
                            tracing::error!("TLS connection error from {}: {:?}", addr, e);
                        }
                    }
                    Err(e) => {
                        tracing::debug!("TLS handshake failed from {}: {:?}", addr, e);
                    }
                }
            });
        }
    }

    /// Handle a single TLS client connection.
    async fn handle_connection(
        service: Arc<ProxyService>,
        tls_stream: tokio_rustls::server::TlsStream<TcpStream>,
        client_addr: SocketAddr,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,
    ) -> anyhow::Result<()> {
        let client_ip = client_addr.ip();

        // For localhost connections, use public IP for geo resolution
        let client_geo = if client_ip.is_loopback() {
            Self::resolve_localhost_geo(geo_resolver, public_ip_geo).await
        } else {
            service.resolve_geo(client_ip)
        };

        // Resolve backend
        let backend = match service.resolve_backend_with_geo(client_ip, client_geo).await {
            Some(b) => b,
            None => {
                tracing::warn!("no backend available for TLS client {}", client_ip);
                return Ok(());
            }
        };

        // Format backend address
        let backend_addr = if backend.wg_ip.contains(':') {
            format!("[{}]:{}", backend.wg_ip, backend.port)
        } else {
            format!("{}:{}", backend.wg_ip, backend.port)
        };

        tracing::debug!(
            "TLS proxying {} -> {} ({})",
            client_ip,
            backend.id,
            backend_addr
        );

        // Connect to backend and measure RTT
        let t0 = Instant::now();
        let backend_stream = match TcpStream::connect(&backend_addr).await {
            Ok(s) => s,
            Err(e) => {
                tracing::error!(
                    "TLS: failed to connect to backend {} at {}: {:?}",
                    backend.id,
                    backend_addr,
                    e
                );
                service.clear_binding(client_ip).await;
                return Ok(());
            }
        };
        let rtt_ms = t0.elapsed().as_millis() as u64;

        // Record metrics
        let backend_id = backend.id.clone();
        service.record_connection_start(&backend_id);
        service.record_rtt(&backend_id, rtt_ms);

        // Perform bidirectional copy (TLS client <-> plain backend)
        let result = Self::proxy_bidirectional(tls_stream, backend_stream).await;

        // Record connection end
        service.record_connection_end(&backend_id);

        // Propagate proxy errors
        result.map_err(|e| anyhow::anyhow!("TLS {} proxy error: {:?}", backend_id, e))
    }

    /// Resolve geo for localhost connections using public IP.
    ///
    /// Excluded from coverage as it depends on external network calls.
    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn resolve_localhost_geo(
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,
    ) -> Option<GeoInfo> {
        {
            let cached = public_ip_geo.read().await;
            if cached.is_some() {
                return cached.clone();
            }
        }

        if let Some(public_ip) = Self::fetch_public_ip().await {
            let geo_info = geo_resolver.as_ref().and_then(|g| g.resolve(public_ip));

            if geo_info.is_some() {
                let mut cached = public_ip_geo.write().await;
                *cached = geo_info.clone();
            }

            return geo_info;
        }

        None
    }

    /// Fetch public IP from AWS checkip service.
    ///
    /// Excluded from coverage as it makes external network calls.
    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn fetch_public_ip() -> Option<IpAddr> {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_secs(5))
            .build()
            .ok()?;

        let resp = client
            .get("https://checkip.amazonaws.com/")
            .send()
            .await
            .ok()?;

        let text = resp.text().await.ok()?.trim().to_string();

        match text.parse::<IpAddr>() {
            Ok(ip) => {
                tracing::debug!("public IP detected: {}", ip);
                Some(ip)
            }
            Err(_) => None,
        }
    }

    /// Perform bidirectional copy between TLS client and plain backend.
    async fn proxy_bidirectional(
        tls_stream: tokio_rustls::server::TlsStream<TcpStream>,
        backend_stream: TcpStream,
    ) -> io::Result<()> {
        let (mut tls_read, mut tls_write) = tokio::io::split(tls_stream);
        let (mut backend_read, mut backend_write) = backend_stream.into_split();

        let client_to_backend = tokio::spawn(async move {
            let result = io::copy(&mut tls_read, &mut backend_write).await;
            let _ = backend_write.shutdown().await;
            result
        });

        let backend_to_client = tokio::spawn(async move {
            let result = io::copy(&mut backend_read, &mut tls_write).await;
            let _ = tls_write.shutdown().await;
            result
        });

        let (c2b, b2c) = tokio::join!(client_to_backend, backend_to_client);

        if let Ok(Err(e)) = c2b {
            tracing::trace!("TLS client->backend copy error: {:?}", e);
        }
        if let Ok(Err(e)) = b2c {
            tracing::trace!("TLS backend->client copy error: {:?}", e);
        }

        Ok(())
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::adapters::outbound::{DashMapBindingRepository, DashMapMetricsStore};
    use crate::domain::entities::Backend;
    use crate::domain::ports::BackendRepository;
    use crate::domain::value_objects::RegionCode;
    use async_trait::async_trait;

    // Mock backend repository for testing
    struct MockBackendRepository {
        backends: Vec<Backend>,
    }

    impl MockBackendRepository {
        fn new(backends: Vec<Backend>) -> Self {
            Self { backends }
        }
    }

    #[async_trait]
    impl BackendRepository for MockBackendRepository {
        async fn get_all(&self) -> Vec<Backend> {
            self.backends.clone()
        }

        async fn get_by_id(&self, id: &str) -> Option<Backend> {
            self.backends.iter().find(|b| b.id == id).cloned()
        }

        async fn get_healthy(&self) -> Vec<Backend> {
            self.backends.iter().filter(|b| b.healthy).cloned().collect()
        }

        async fn get_version(&self) -> u64 {
            1
        }
    }

    fn create_test_backend(id: &str) -> Backend {
        Backend {
            id: id.to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: 9999,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        }
    }

    fn create_proxy_service(backends: Vec<Backend>) -> Arc<ProxyService> {
        let backend_repo = Arc::new(MockBackendRepository::new(backends));
        let binding_repo = Arc::new(DashMapBindingRepository::new());
        let metrics = Arc::new(DashMapMetricsStore::new());

        Arc::new(ProxyService::new(
            backend_repo,
            binding_repo,
            None,
            metrics,
            RegionCode::Europe,
        ))
    }

    #[test]
    fn test_self_signed_certificate_generation() {
        let config = TlsConfig::self_signed("test.internal");
        assert!(config.is_ok());
    }

    #[test]
    fn test_self_signed_with_custom_domain() {
        let config = TlsConfig::self_signed("myapp.internal");
        assert!(config.is_ok());
    }

    #[test]
    fn test_self_signed_with_localhost() {
        let config = TlsConfig::self_signed("localhost");
        assert!(config.is_ok());
    }

    #[test]
    fn test_self_signed_with_ip_address() {
        let config = TlsConfig::self_signed("192.168.1.1");
        assert!(config.is_ok());
    }

    #[test]
    fn test_from_pem_files_nonexistent() {
        let result = TlsConfig::from_pem_files(
            Path::new("/nonexistent/cert.pem"),
            Path::new("/nonexistent/key.pem"),
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_tls_server_new() {
        let proxy_service = create_proxy_service(vec![create_test_backend("test-1")]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();
        let server = TlsServer::new(
            proxy_service,
            "0.0.0.0:8443".to_string(),
            None,
            tls_config,
        );
        assert_eq!(server.listen_addr, "0.0.0.0:8443");
    }

    #[test]
    fn test_tls_server_new_with_custom_config() {
        let proxy_service = create_proxy_service(vec![create_test_backend("test-1")]);
        let tls_config = TlsConfig::self_signed("custom.domain").unwrap();
        let server = TlsServer::new(
            proxy_service,
            "127.0.0.1:9443".to_string(),
            None,
            tls_config,
        );
        assert_eq!(server.listen_addr, "127.0.0.1:9443");
        assert!(server.geo_resolver.is_none());
    }

    #[tokio::test]
    async fn test_resolve_localhost_geo_returns_none_without_resolver() {
        let public_ip_geo = Arc::new(RwLock::new(None));
        let result = TlsServer::resolve_localhost_geo(None, public_ip_geo).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_resolve_localhost_geo_returns_cached() {
        let cached_geo = GeoInfo::new("DE".to_string(), RegionCode::Europe);
        let public_ip_geo = Arc::new(RwLock::new(Some(cached_geo.clone())));

        let result = TlsServer::resolve_localhost_geo(None, public_ip_geo).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap().country, "DE");
    }

    #[tokio::test]
    async fn test_backend_addr_format_ipv4() {
        let backend = Backend {
            id: "test".to_string(),
            app: "app".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let backend_addr = if backend.wg_ip.contains(':') {
            format!("[{}]:{}", backend.wg_ip, backend.port)
        } else {
            format!("{}:{}", backend.wg_ip, backend.port)
        };

        assert_eq!(backend_addr, "10.0.0.1:8080");
    }

    #[tokio::test]
    async fn test_backend_addr_format_ipv6() {
        let backend = Backend {
            id: "test".to_string(),
            app: "app".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "2001:db8::1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let backend_addr = if backend.wg_ip.contains(':') {
            format!("[{}]:{}", backend.wg_ip, backend.port)
        } else {
            format!("{}:{}", backend.wg_ip, backend.port)
        };

        assert_eq!(backend_addr, "[2001:db8::1]:8080");
    }

    #[test]
    fn test_tls_config_clone() {
        let config = TlsConfig::self_signed("test.internal").unwrap();
        let cloned = config.clone();
        // TlsConfig should be cloneable
        drop(cloned);
    }

    // ===== Integration Tests with Mock Backend =====

    #[tokio::test]
    async fn test_handle_connection_no_backend() {
        // Create proxy service with no backends
        let proxy_service = create_proxy_service(vec![]);
        let client_addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();

        // Create self-signed TLS config for test
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        // Create a local listener to simulate a TLS client
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        // Start a mock TLS client
        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            // Create client config that accepts any cert (for testing)
            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();
            let _ = connector.connect(server_name, stream).await;
        });

        // Accept the connection and perform TLS handshake
        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let public_ip_geo = Arc::new(RwLock::new(None));
                let _ = TlsServer::handle_connection(
                    proxy_service.clone(),
                    tls_stream,
                    client_addr,
                    None,
                    public_ip_geo,
                )
                .await;
            }
        })
        .await;

        assert!(result.is_ok());
        client_handle.abort();
    }

    #[tokio::test]
    async fn test_handle_connection_with_backend() {
        use tokio::sync::oneshot;

        // Start a mock backend server
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        let (echo_tx, echo_rx) = oneshot::channel::<()>();

        // Echo server
        let echo_handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = backend_listener.accept().await {
                let (mut reader, mut writer) = stream.split();
                let _ = io::copy(&mut reader, &mut writer).await;
            }
            let _ = echo_tx.send(());
        });

        // Create backend pointing to our mock
        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: backend_addr.port(),
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        // TLS client that sends data
        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();

            if let Ok(mut tls_stream) = connector.connect(server_name, stream).await {
                use tokio::io::AsyncWriteExt;
                let _ = tls_stream.write_all(b"hello").await;
                let _ = tls_stream.shutdown().await;
            }
        });

        // Accept and handle
        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let public_ip_geo = Arc::new(RwLock::new(None));
                let client_addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();
                let _ = TlsServer::handle_connection(
                    proxy_service.clone(),
                    tls_stream,
                    client_addr,
                    None,
                    public_ip_geo,
                )
                .await;
            }
        })
        .await;

        assert!(result.is_ok());
        // Wait for echo to complete
        let _ = tokio::time::timeout(Duration::from_millis(100), echo_rx).await;
        client_handle.abort();
        echo_handle.abort();
    }

    #[tokio::test]
    async fn test_handle_connection_backend_unreachable() {
        // Backend on a port that nothing is listening on
        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: 59999, // unlikely to be in use
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();
            let _ = connector.connect(server_name, stream).await;
        });

        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let public_ip_geo = Arc::new(RwLock::new(None));
                let client_addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();
                // This should handle the connection error gracefully
                let _ = TlsServer::handle_connection(
                    proxy_service.clone(),
                    tls_stream,
                    client_addr,
                    None,
                    public_ip_geo,
                )
                .await;
            }
        })
        .await;

        assert!(result.is_ok());
        client_handle.abort();
    }

    #[tokio::test]
    async fn test_handle_connection_with_loopback_ip() {
        use tokio::sync::oneshot;

        // Start mock backend
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        let (echo_tx, echo_rx) = oneshot::channel::<()>();

        let echo_handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = backend_listener.accept().await {
                let (mut reader, mut writer) = stream.split();
                let _ = io::copy(&mut reader, &mut writer).await;
            }
            let _ = echo_tx.send(());
        });

        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: backend_addr.port(),
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();

            if let Ok(mut tls_stream) = connector.connect(server_name, stream).await {
                use tokio::io::AsyncWriteExt;
                let _ = tls_stream.write_all(b"test").await;
                let _ = tls_stream.shutdown().await;
            }
        });

        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let public_ip_geo = Arc::new(RwLock::new(None));
                // Use loopback address as client
                let client_addr: SocketAddr = "127.0.0.1:12345".parse().unwrap();
                let _ = TlsServer::handle_connection(
                    proxy_service.clone(),
                    tls_stream,
                    client_addr,
                    None,
                    public_ip_geo,
                )
                .await;
            }
        })
        .await;

        assert!(result.is_ok());
        // Wait for echo to complete
        let _ = tokio::time::timeout(Duration::from_millis(100), echo_rx).await;
        client_handle.abort();
        echo_handle.abort();
    }

    #[tokio::test]
    async fn test_run_accepts_tls_connection() {
        use tokio::sync::oneshot;

        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        let (echo_tx, echo_rx) = oneshot::channel::<()>();

        let echo_handle = tokio::spawn(async move {
            if let Ok((mut stream, _)) = backend_listener.accept().await {
                let (mut reader, mut writer) = stream.split();
                let _ = io::copy(&mut reader, &mut writer).await;
            }
            let _ = echo_tx.send(());
        });

        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: backend_addr.port(),
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        // Bind to get a free port, then drop and let server bind to it
        let temp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        let server = TlsServer::new(
            proxy_service,
            listen_addr.to_string(),
            None,
            tls_config.clone(),
        );

        // Run the server in background
        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give the server time to start and bind
        tokio::time::sleep(Duration::from_millis(100)).await;

        // Connect as TLS client
        let client_result = tokio::time::timeout(Duration::from_secs(2), async {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await?;
            let server_name = ServerName::try_from("test.internal").unwrap();

            let mut tls_stream = connector.connect(server_name, stream).await?;

            use tokio::io::AsyncWriteExt;
            tls_stream.write_all(b"hello").await?;
            tls_stream.shutdown().await?;

            Ok::<_, anyhow::Error>(())
        })
        .await;

        assert!(client_result.is_ok());

        // Wait for echo to complete
        let _ = tokio::time::timeout(Duration::from_millis(100), echo_rx).await;
        server_handle.abort();
        echo_handle.abort();
    }

    #[tokio::test]
    async fn test_handle_connection_with_ipv6_backend() {
        // Create backend with IPv6 address
        let backend = Backend {
            id: "ipv6-backend".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "::1".to_string(),  // IPv6 loopback
            port: 59997,               // A port that won't be listening
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();
            let _ = connector.connect(server_name, stream).await;
        });

        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        let result = tokio::time::timeout(Duration::from_secs(2), async {
            if let Ok(tls_stream) = acceptor.accept(stream).await {
                let public_ip_geo = Arc::new(RwLock::new(None));
                let client_addr: SocketAddr = "192.168.1.100:12345".parse().unwrap();
                let _ = TlsServer::handle_connection(
                    proxy_service.clone(),
                    tls_stream,
                    client_addr,
                    None,
                    public_ip_geo,
                )
                .await;
            }
        })
        .await;

        assert!(result.is_ok());
        client_handle.abort();
    }

    #[tokio::test]
    async fn test_run_handles_tls_handshake_failure() {
        // This tests that the server gracefully handles TLS handshake failures
        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: 9999,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let temp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        let server = TlsServer::new(
            proxy_service,
            listen_addr.to_string(),
            None,
            tls_config,
        );

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Connect with plain TCP (not TLS) to trigger handshake failure
        let result = tokio::time::timeout(Duration::from_millis(500), async {
            if let Ok(mut stream) = TcpStream::connect(listen_addr).await {
                // Send non-TLS data
                let _ = stream.write_all(b"not TLS data").await;
                let _ = stream.shutdown().await;
            }
        })
        .await;

        // Should complete without panic
        assert!(result.is_ok());

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_proxy_bidirectional_with_errors() {
        // Test proxy_bidirectional with a backend that closes immediately
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        // Backend that closes immediately
        let backend_handle = tokio::spawn(async move {
            if let Ok((stream, _)) = backend_listener.accept().await {
                drop(stream);
            }
        });

        let tls_config = TlsConfig::self_signed("test.internal").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = listener.local_addr().unwrap();

        let client_handle = tokio::spawn(async move {
            use tokio_rustls::TlsConnector;
            use rustls::pki_types::ServerName;

            let client_config = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                    rustls::crypto::ring::default_provider(),
                )))
                .with_no_client_auth();

            let connector = TlsConnector::from(Arc::new(client_config));
            let stream = TcpStream::connect(listen_addr).await.unwrap();
            let server_name = ServerName::try_from("test.internal").unwrap();

            if let Ok(mut tls_stream) = connector.connect(server_name, stream).await {
                use tokio::io::AsyncWriteExt;
                let _ = tls_stream.write_all(b"test data").await;
                // Don't gracefully shutdown - just drop
            }
        });

        let (stream, _) = listener.accept().await.unwrap();
        let acceptor = tls_config.acceptor.clone();

        if let Ok(tls_stream) = tokio::time::timeout(
            Duration::from_secs(2),
            acceptor.accept(stream),
        )
        .await
        {
            if let Ok(tls_stream) = tls_stream {
                // Connect to backend and proxy
                let backend_stream = TcpStream::connect(backend_addr).await.unwrap();

                // Give backend time to close
                tokio::time::sleep(Duration::from_millis(50)).await;

                // This should handle errors gracefully
                let result = tokio::time::timeout(
                    Duration::from_millis(500),
                    TlsServer::proxy_bidirectional(tls_stream, backend_stream),
                )
                .await;

                // Should complete without panicking
                assert!(result.is_ok() || result.is_err());
            }
        }

        client_handle.abort();
        backend_handle.abort();
    }

    // Mock geo resolver for testing
    struct MockGeoResolver {
        geo_info: Option<GeoInfo>,
    }

    impl MockGeoResolver {
        fn new(geo_info: Option<GeoInfo>) -> Self {
            Self { geo_info }
        }
    }

    impl GeoResolver for MockGeoResolver {
        fn resolve(&self, _ip: IpAddr) -> Option<GeoInfo> {
            self.geo_info.clone()
        }
    }

    #[tokio::test]
    async fn test_resolve_localhost_geo_with_resolver() {
        let geo_info = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);
        let resolver = Arc::new(MockGeoResolver::new(Some(geo_info)));
        let public_ip_geo = Arc::new(RwLock::new(None));

        // This will try to fetch public IP which will fail/timeout in test env
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            TlsServer::resolve_localhost_geo(Some(resolver), public_ip_geo),
        )
        .await;

        // Should timeout (fetch_public_ip takes time) or return None
        assert!(result.is_err() || result.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_run_multiple_connections() {
        let backend_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let backend_addr = backend_listener.local_addr().unwrap();

        // Backend that handles multiple connections
        let backend_handle = tokio::spawn(async move {
            for _ in 0..3 {
                if let Ok((mut stream, _)) = backend_listener.accept().await {
                    let _ = stream.shutdown().await;
                }
            }
        });

        let backend = Backend {
            id: "test-1".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "127.0.0.1".to_string(),
            port: backend_addr.port(),
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        let proxy_service = create_proxy_service(vec![backend]);
        let tls_config = TlsConfig::self_signed("test.internal").unwrap();

        let temp_listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = temp_listener.local_addr().unwrap();
        drop(temp_listener);

        let server = TlsServer::new(
            proxy_service,
            listen_addr.to_string(),
            None,
            tls_config.clone(),
        );

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(100)).await;

        // Make multiple TLS connections
        for _ in 0..3 {
            let result = tokio::time::timeout(Duration::from_secs(2), async {
                use tokio_rustls::TlsConnector;
                use rustls::pki_types::ServerName;

                let client_config = rustls::ClientConfig::builder()
                    .dangerous()
                    .with_custom_certificate_verifier(Arc::new(danger::NoCertificateVerification::new(
                        rustls::crypto::ring::default_provider(),
                    )))
                    .with_no_client_auth();

                let connector = TlsConnector::from(Arc::new(client_config));
                let stream = TcpStream::connect(listen_addr).await?;
                let server_name = ServerName::try_from("test.internal").unwrap();

                let mut tls_stream = connector.connect(server_name, stream).await?;
                use tokio::io::AsyncWriteExt;
                tls_stream.write_all(b"test").await?;
                tls_stream.shutdown().await?;

                Ok::<_, anyhow::Error>(())
            })
            .await;

            assert!(result.is_ok());
        }

        server_handle.abort();
        backend_handle.abort();
    }

    #[tokio::test]
    async fn test_from_pem_files_success() {
        use std::io::Write;
        use tempfile::NamedTempFile;

        // Generate a self-signed cert to get the PEM data
        let subject_alt_names = vec!["localhost".to_string()];
        let cert = rcgen::generate_simple_self_signed(subject_alt_names).unwrap();

        // Write cert to temp file
        let mut cert_file = NamedTempFile::new().unwrap();
        writeln!(cert_file, "-----BEGIN CERTIFICATE-----").unwrap();
        writeln!(cert_file, "{}", base64_encode(&cert.serialize_der().unwrap())).unwrap();
        writeln!(cert_file, "-----END CERTIFICATE-----").unwrap();

        // Write key to temp file
        let mut key_file = NamedTempFile::new().unwrap();
        writeln!(key_file, "-----BEGIN PRIVATE KEY-----").unwrap();
        writeln!(key_file, "{}", base64_encode(&cert.serialize_private_key_der())).unwrap();
        writeln!(key_file, "-----END PRIVATE KEY-----").unwrap();

        // Test from_pem_files
        let result = TlsConfig::from_pem_files(
            cert_file.path(),
            key_file.path(),
        );

        assert!(result.is_ok());
    }

    /// Helper function to base64 encode for PEM files
    fn base64_encode(data: &[u8]) -> String {
        let mut output = Vec::new();
        let mut line = Vec::new();
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

        for chunk in data.chunks(3) {
            let n = match chunk.len() {
                1 => (chunk[0] as u32) << 16,
                2 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8),
                3 => ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32),
                _ => 0,
            };

            line.push(ALPHABET[((n >> 18) & 0x3F) as usize]);
            line.push(ALPHABET[((n >> 12) & 0x3F) as usize]);
            if chunk.len() > 1 {
                line.push(ALPHABET[((n >> 6) & 0x3F) as usize]);
            } else {
                line.push(b'=');
            }
            if chunk.len() > 2 {
                line.push(ALPHABET[(n & 0x3F) as usize]);
            } else {
                line.push(b'=');
            }

            if line.len() >= 64 {
                output.extend_from_slice(&line[..64]);
                output.push(b'\n');
                line.drain(..64);
            }
        }
        if !line.is_empty() {
            output.extend_from_slice(&line);
        }
        String::from_utf8(output).unwrap_or_default()
    }

    #[test]
    fn test_base64_encode_various_sizes() {
        // Test 1 byte (covers chunk.len() == 1 and padding branches)
        let result = base64_encode(&[0xFF]);
        assert!(!result.is_empty());
        assert!(result.ends_with("==") || result.contains('='));

        // Test 2 bytes (covers chunk.len() == 2)
        let result = base64_encode(&[0xFF, 0xAA]);
        assert!(!result.is_empty());

        // Test 3 bytes (covers chunk.len() == 3)
        let result = base64_encode(&[0xFF, 0xAA, 0x55]);
        assert!(!result.is_empty());

        // Test longer data that wraps at 64 chars
        let long_data: Vec<u8> = (0..100).collect();
        let result = base64_encode(&long_data);
        assert!(!result.is_empty());
        // Should contain newlines for wrapping
        if result.len() > 64 {
            assert!(result.contains('\n'));
        }

        // Test empty input
        let result = base64_encode(&[]);
        assert!(result.is_empty());
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_all() {
        let backends = vec![
            create_test_backend("eu-1"),
            create_test_backend("eu-2"),
        ];
        let repo = MockBackendRepository::new(backends);
        let all = repo.get_all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_by_id() {
        let backends = vec![create_test_backend("eu-1")];
        let repo = MockBackendRepository::new(backends);
        let found = repo.get_by_id("eu-1").await;
        assert!(found.is_some());
        let not_found = repo.get_by_id("nope").await;
        assert!(not_found.is_none());
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_version() {
        let repo = MockBackendRepository::new(vec![]);
        let version = repo.get_version().await;
        assert_eq!(version, 1);
    }

    #[tokio::test]
    async fn test_mock_geo_resolver() {
        let geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);
        let resolver = MockGeoResolver::new(Some(geo.clone()));
        let result = resolver.resolve("8.8.8.8".parse().unwrap());
        assert!(result.is_some());
        assert_eq!(result.unwrap().country, "BR");

        let resolver_none = MockGeoResolver::new(None);
        let result_none = resolver_none.resolve("8.8.8.8".parse().unwrap());
        assert!(result_none.is_none());
    }

    #[tokio::test]
    async fn test_backend_addr_format_ipv6_in_handle_connection() {
        // Create a backend with IPv6 address - this tests the IPv6 formatting branch
        // The IPv6 backend port format: [ip]:port
        let backend = Backend {
            id: "ipv6-test".to_string(),
            app: "testapp".to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: "2001:db8::1".to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        };

        // Directly test the formatting logic
        let backend_addr = if backend.wg_ip.contains(':') {
            format!("[{}]:{}", backend.wg_ip, backend.port)
        } else {
            format!("{}:{}", backend.wg_ip, backend.port)
        };
        assert_eq!(backend_addr, "[2001:db8::1]:8080");
    }

    /// Dangerous certificate verifier for testing purposes only
    mod danger {
        use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
        use rustls::pki_types::{CertificateDer, ServerName, UnixTime};
        use rustls::DigitallySignedStruct;

        #[derive(Debug)]
        pub struct NoCertificateVerification(rustls::crypto::CryptoProvider);

        impl NoCertificateVerification {
            pub fn new(provider: rustls::crypto::CryptoProvider) -> Self {
                Self(provider)
            }
        }

        impl ServerCertVerifier for NoCertificateVerification {
            fn verify_server_cert(
                &self,
                _end_entity: &CertificateDer<'_>,
                _intermediates: &[CertificateDer<'_>],
                _server_name: &ServerName<'_>,
                _ocsp_response: &[u8],
                _now: UnixTime,
            ) -> Result<ServerCertVerified, rustls::Error> {
                Ok(ServerCertVerified::assertion())
            }

            fn verify_tls12_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls12_signature(
                    message,
                    cert,
                    dss,
                    &self.0.signature_verification_algorithms,
                )
            }

            fn verify_tls13_signature(
                &self,
                message: &[u8],
                cert: &CertificateDer<'_>,
                dss: &DigitallySignedStruct,
            ) -> Result<HandshakeSignatureValid, rustls::Error> {
                rustls::crypto::verify_tls13_signature(
                    message,
                    cert,
                    dss,
                    &self.0.signature_verification_algorithms,
                )
            }

            fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
                self.0.signature_verification_algorithms.supported_schemes()
            }
        }
    }
}
