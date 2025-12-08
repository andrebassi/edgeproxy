//! TCP Server Adapter
//!
//! Accepts TCP connections and proxies them to backends
//! using the application service layer.

use crate::application::ProxyService;
use crate::domain::entities::GeoInfo;
use crate::domain::ports::GeoResolver;
use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::RwLock;

/// TCP Server - inbound adapter for handling client connections.
///
/// This adapter:
/// 1. Accepts incoming TCP connections
/// 2. Uses ProxyService to resolve the best backend
/// 3. Establishes connection to backend
/// 4. Performs bidirectional TCP copy (L4 passthrough)
pub struct TcpServer {
    proxy_service: Arc<ProxyService>,
    listen_addr: String,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,
}

impl TcpServer {
    /// Create a new TCP server.
    pub fn new(
        proxy_service: Arc<ProxyService>,
        listen_addr: String,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
    ) -> Self {
        Self {
            proxy_service,
            listen_addr,
            geo_resolver,
            public_ip_geo: Arc::new(RwLock::new(None)),
        }
    }

    /// Run the TCP server.
    ///
    /// This will listen for incoming connections and spawn
    /// a new task for each connection.
    pub async fn run(&self) -> anyhow::Result<()> {
        let listener = TcpListener::bind(&self.listen_addr).await?;
        tracing::info!("edgeProxy listening on {}", self.listen_addr);

        loop {
            let (stream, addr) = listener.accept().await?;
            let service = self.proxy_service.clone();
            let geo_resolver = self.geo_resolver.clone();
            let public_ip_geo = self.public_ip_geo.clone();

            tokio::spawn(async move {
                if let Err(e) =
                    Self::handle_connection(service, stream, addr, geo_resolver, public_ip_geo)
                        .await
                {
                    tracing::error!("connection error from {}: {:?}", addr, e);
                }
            });
        }
    }

    /// Handle a single client connection.
    async fn handle_connection(
        service: Arc<ProxyService>,
        client_stream: TcpStream,
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
                tracing::warn!("no backend available for {}", client_ip);
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
            "proxying {} -> {} ({})",
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
                    "failed to connect to backend {} at {}: {:?}",
                    backend.id,
                    backend_addr,
                    e
                );
                // Clear binding on connection failure
                service.clear_binding(client_ip).await;
                return Ok(());
            }
        };
        let rtt_ms = t0.elapsed().as_millis() as u64;

        // Record metrics
        let backend_id = backend.id.clone();
        service.record_connection_start(&backend_id);
        service.record_rtt(&backend_id, rtt_ms);

        // Perform bidirectional copy
        let result = Self::proxy_bidirectional(client_stream, backend_stream).await;

        // Record connection end
        service.record_connection_end(&backend_id);

        if let Err(e) = result {
            tracing::debug!("{} proxy error: {:?}", backend_id, e);
        }

        Ok(())
    }

    /// Resolve geo for localhost connections using public IP.
    async fn resolve_localhost_geo(
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,
    ) -> Option<GeoInfo> {
        // Try to get cached geo first
        {
            let cached = public_ip_geo.read().await;
            if cached.is_some() {
                return cached.clone();
            }
        }

        // Fetch public IP and resolve geo
        if let Some(public_ip) = Self::fetch_public_ip().await {
            let geo_info = geo_resolver.as_ref().and_then(|g| g.resolve(public_ip));

            if geo_info.is_some() {
                // Cache the result
                let mut cached = public_ip_geo.write().await;
                *cached = geo_info.clone();
            }

            return geo_info;
        }

        None
    }

    /// Fetch public IP from AWS checkip service.
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

    /// Perform bidirectional TCP copy between client and backend.
    async fn proxy_bidirectional(
        client_stream: TcpStream,
        backend_stream: TcpStream,
    ) -> io::Result<()> {
        let (mut client_read, mut client_write) = client_stream.into_split();
        let (mut backend_read, mut backend_write) = backend_stream.into_split();

        // Spawn tasks for each direction
        let client_to_backend = tokio::spawn(async move {
            let result = io::copy(&mut client_read, &mut backend_write).await;
            let _ = backend_write.shutdown().await;
            result
        });

        let backend_to_client = tokio::spawn(async move {
            io::copy(&mut backend_read, &mut client_write).await
        });

        // Wait for both to complete
        let (c2b, b2c) = tokio::join!(client_to_backend, backend_to_client);

        // Log errors but don't propagate (connection closing is normal)
        if let Ok(Err(e)) = c2b {
            tracing::trace!("client->backend copy error: {:?}", e);
        }
        if let Ok(Err(e)) = b2c {
            tracing::trace!("backend->client copy error: {:?}", e);
        }

        Ok(())
    }
}
