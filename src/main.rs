//! edgeProxy - Distributed TCP Proxy with Hexagonal Architecture
//!
//! This is the composition root that wires together all the components.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

mod adapters;
mod application;
mod config;
mod domain;

use crate::adapters::inbound::{ApiServer, DnsServer, TcpServer, TlsConfig, TlsServer};
use crate::adapters::outbound::{
    CorrosionBackendRepository, CorrosionConfig, DashMapBindingRepository, DashMapMetricsStore,
    MaxMindGeoResolver, SqliteBackendRepository,
};
use crate::domain::ports::BackendRepository;
use crate::application::ProxyService;
use crate::config::load_config;
use crate::domain::ports::GeoResolver;
use crate::domain::value_objects::RegionCode;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;

#[cfg_attr(coverage_nightly, coverage(off))]
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Load configuration from environment
    let cfg = load_config()?;

    // Setup logging
    let log_level = if cfg.debug {
        tracing::Level::DEBUG
    } else {
        tracing::Level::INFO
    };

    tracing_subscriber::fmt()
        .with_max_level(log_level)
        .with_span_events(FmtSpan::CLOSE)
        .init();

    tracing::info!(
        "starting edgeProxy region={} listen={} (hexagonal architecture)",
        cfg.region,
        cfg.listen_addr
    );

    // ===== COMPOSITION ROOT =====
    // Wire up all adapters and services

    // 1. Create outbound adapters

    // Backend repository - choose between SQLite (local) or Corrosion (distributed)
    let backend_repo: Arc<dyn BackendRepository> = if cfg.corrosion_enabled {
        tracing::info!(
            "using Corrosion backend repository (api_url={}, poll_secs={})",
            cfg.corrosion_api_url,
            cfg.corrosion_poll_secs
        );
        let corrosion_config = CorrosionConfig {
            api_url: cfg.corrosion_api_url.clone(),
            poll_interval_secs: cfg.corrosion_poll_secs,
        };
        let repo = Arc::new(CorrosionBackendRepository::new(corrosion_config));
        repo.start_sync();
        repo
    } else {
        tracing::info!("using SQLite backend repository (path={})", cfg.db_path);
        let repo = Arc::new(SqliteBackendRepository::new());
        repo.start_sync(cfg.db_path.clone(), cfg.db_reload_secs);
        repo
    };

    // Binding repository (DashMap)
    let binding_repo = Arc::new(DashMapBindingRepository::new());
    binding_repo.start_gc(
        Duration::from_secs(cfg.binding_ttl_secs),
        Duration::from_secs(cfg.binding_gc_interval_secs),
    );

    // GeoIP resolver (MaxMind)
    let geo_resolver: Option<Arc<dyn GeoResolver>> = match &cfg.geoip_path {
        Some(path) => match MaxMindGeoResolver::from_file(path) {
            Ok(g) => {
                tracing::info!("GeoIP DB loaded from {}", path);
                Some(Arc::new(g) as Arc<dyn GeoResolver>)
            }
            Err(e) => {
                tracing::error!("failed to load GeoIP DB from {}: {:?}", path, e);
                None
            }
        },
        None => match MaxMindGeoResolver::embedded() {
            Ok(g) => {
                tracing::info!("GeoIP DB loaded (embedded)");
                Some(Arc::new(g) as Arc<dyn GeoResolver>)
            }
            Err(e) => {
                tracing::error!("failed to load embedded GeoIP DB: {:?}", e);
                None
            }
        },
    };

    // Metrics store (DashMap)
    let metrics = Arc::new(DashMapMetricsStore::new());

    // 2. Create application service
    let proxy_service = Arc::new(ProxyService::new(
        backend_repo,
        binding_repo,
        geo_resolver.clone(),
        metrics,
        RegionCode::from_str(&cfg.region),
    ));

    // 3. Create inbound adapters and run

    // Start Auto-Discovery API server (optional)
    if cfg.api_enabled {
        let api_server = ApiServer::new(cfg.api_listen_addr.clone(), cfg.heartbeat_ttl_secs);
        api_server.start_cleanup_task(30); // Cleanup every 30 seconds

        tokio::spawn(async move {
            if let Err(e) = api_server.run().await {
                tracing::error!("API server error: {:?}", e);
            }
        });
        tracing::info!("Auto-Discovery API enabled on {}", cfg.api_listen_addr);
    }

    // Start DNS server (optional)
    if cfg.dns_enabled {
        let dns_server = DnsServer::new(
            cfg.dns_listen_addr.clone(),
            proxy_service.clone(),
            geo_resolver.clone(),
            cfg.dns_domain.clone(),
        );

        tokio::spawn(async move {
            if let Err(e) = dns_server.run().await {
                tracing::error!("DNS server error: {:?}", e);
            }
        });
        tracing::info!(
            "DNS server enabled on {} for .{} domain",
            cfg.dns_listen_addr,
            cfg.dns_domain
        );
    }

    // Start TLS server (optional)
    if cfg.tls_enabled {
        let tls_listen_addr = cfg
            .tls_listen_addr
            .clone()
            .unwrap_or_else(|| "0.0.0.0:8443".to_string());

        // Load TLS config from files or generate self-signed
        let tls_config = match (&cfg.tls_cert_path, &cfg.tls_key_path) {
            (Some(cert), Some(key)) => {
                TlsConfig::from_pem_files(Path::new(cert), Path::new(key))?
            }
            _ => {
                tracing::warn!("No TLS cert/key provided, generating self-signed certificate");
                TlsConfig::self_signed("edgeproxy.internal")?
            }
        };

        let tls_server = TlsServer::new(
            proxy_service.clone(),
            tls_listen_addr.clone(),
            geo_resolver.clone(),
            tls_config,
        );

        tokio::spawn(async move {
            if let Err(e) = tls_server.run().await {
                tracing::error!("TLS server error: {:?}", e);
            }
        });
        tracing::info!("TLS server enabled on {}", tls_listen_addr);
    }

    // Start main TCP server
    let server = TcpServer::new(proxy_service, cfg.listen_addr, geo_resolver);
    server.run().await
}
