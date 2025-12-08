//! edgeProxy - Distributed TCP Proxy with Hexagonal Architecture
//!
//! This is the composition root that wires together all the components.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

use edge_proxy::adapters::inbound::{ApiServer, DnsServer, TcpServer, TlsConfig, TlsServer};
use edge_proxy::adapters::outbound::{
    DashMapBindingRepository, DashMapMetricsStore,
    MaxMindGeoResolver, SqliteBackendRepository,
};
use edge_proxy::domain::ports::BackendRepository;
use edge_proxy::application::ProxyService;
use edge_proxy::config::load_config;
use edge_proxy::domain::ports::GeoResolver;
use edge_proxy::domain::value_objects::RegionCode;
use edge_proxy::replication::{ReplicationAgent, ReplicationConfig};
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

    // Backend repository - uses SQLite for local storage
    // When replication is enabled, the replication module syncs the state.db across nodes
    let backend_repo: Arc<dyn BackendRepository> = {
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

    // Start built-in replication (if enabled)
    if cfg.replication_enabled {
        let node_id = cfg.replication_node_id.clone()
            .unwrap_or_else(|| format!("{}-{}", cfg.region, uuid::Uuid::new_v4().to_string()[..8].to_string()));

        let replication_config = ReplicationConfig::new(&node_id)
            .gossip_addr(cfg.replication_gossip_addr.parse()?)
            .transport_addr(cfg.replication_transport_addr.parse()?)
            .bootstrap_peers(cfg.replication_bootstrap_peers.clone())
            .db_path(&cfg.replication_db_path)
            .cluster_name(&cfg.replication_cluster_name);

        let mut agent = ReplicationAgent::new(replication_config)?;

        tracing::info!(
            "starting built-in replication node_id={} gossip={} transport={}",
            node_id,
            cfg.replication_gossip_addr,
            cfg.replication_transport_addr
        );

        if let Err(e) = agent.start().await {
            tracing::error!("failed to start replication agent: {:?}", e);
        } else {
            tracing::info!("built-in replication started");
        }
    }

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
