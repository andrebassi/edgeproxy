mod config;
mod db;
mod lb;
mod model;
mod proxy;
mod state;

use crate::config::load_config;
use crate::db::start_routing_sync_sqlite;
use crate::proxy::run_tcp_proxy;
use crate::state::{GeoDb, RcProxyState, start_binding_gc};
use std::time::Duration;
use tracing_subscriber::fmt::format::FmtSpan;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = load_config()?;

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
        "starting edgeProxy region={} listen={}",
        cfg.region,
        cfg.listen_addr
    );

    // Load GeoIP: prefer external file if specified, otherwise use embedded
    let geo = match &cfg.geoip_path {
        Some(path) => match GeoDb::open(path) {
            Ok(db) => {
                tracing::info!("GeoIP DB loaded from {}", path);
                Some(db)
            }
            Err(e) => {
                tracing::error!("failed to load GeoIP DB from {}: {:?}", path, e);
                None
            }
        },
        None => match GeoDb::embedded() {
            Ok(db) => {
                tracing::info!("GeoIP DB loaded (embedded)");
                Some(db)
            }
            Err(e) => {
                tracing::error!("failed to load embedded GeoIP DB: {:?}", e);
                None
            }
        },
    };

    let state = RcProxyState::new(cfg.region.clone(), geo);

    // Sync routing.db (Corrosion cuida de replicar)
    let routing = state.routing.clone();
    let db_path = cfg.db_path.clone();
    let interval = cfg.db_reload_secs;
    tokio::spawn(async move {
        if let Err(e) = start_routing_sync_sqlite(routing, db_path, interval).await {
            tracing::error!("routing sync error: {:?}", e);
        }
    });

    // GC de bindings
    start_binding_gc(
        state.bindings.clone(),
        Duration::from_secs(cfg.binding_ttl_secs),
        Duration::from_secs(cfg.binding_gc_interval_secs),
    );

    // Proxy TCP
    run_tcp_proxy(state, cfg.listen_addr.clone()).await?;

    Ok(())
}
