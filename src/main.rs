//! edgeProxy - Distributed TCP Proxy with Hexagonal Architecture
//!
//! This is the composition root that wires together all the components.

mod adapters;
mod application;
mod config;
mod domain;

use crate::adapters::inbound::TcpServer;
use crate::adapters::outbound::{
    DashMapBindingRepository, DashMapMetricsStore, MaxMindGeoResolver, SqliteBackendRepository,
};
use crate::application::ProxyService;
use crate::config::load_config;
use crate::domain::ports::GeoResolver;
use crate::domain::value_objects::RegionCode;
use std::sync::Arc;
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;

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

    // Backend repository (SQLite)
    let backend_repo = Arc::new(SqliteBackendRepository::new());
    backend_repo.start_sync(cfg.db_path.clone(), cfg.db_reload_secs);

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

    // 3. Create inbound adapter and run
    let server = TcpServer::new(proxy_service, cfg.listen_addr, geo_resolver);

    server.run().await
}
