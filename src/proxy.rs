use crate::lb::{pick_backend, BackendMetrics};
use crate::model::{Binding, ClientKey};
use crate::state::{RcProxyState, fetch_public_ip, GeoInfo};
use std::net::SocketAddr;
use std::time::Instant;
use tokio::io::{self, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task;
use std::sync::atomic::Ordering;


pub async fn run_tcp_proxy(state: RcProxyState, listen_addr: String) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&listen_addr).await?;
    tracing::info!("edgeProxy listening on {}", listen_addr);

    loop {
        let (client_stream, client_addr) = listener.accept().await?;
        let state_clone = state.clone();

        task::spawn(async move {
            if let Err(e) = handle_connection(state_clone, client_stream, client_addr).await {
                tracing::error!("connection error from {}: {:?}", client_addr, e);
            }
        });
    }
}

async fn handle_connection(
    state: RcProxyState,
    client_stream: TcpStream,
    client_addr: SocketAddr,
) -> anyhow::Result<()> {
    let client_ip = client_addr.ip();
    let client_key = ClientKey { client_ip };
    let now = Instant::now();

    // For localhost clients, use public IP for geo routing
    // Always re-check public IP to support VPN switching during testing
    let client_geo: Option<GeoInfo> = if client_ip.is_loopback() {
        let new_geo = if let Some(public_ip) = fetch_public_ip().await {
            let geo_info = state.geo.as_ref()
                .and_then(|g| g.lookup_ip(public_ip))
                .map(|(country, region)| GeoInfo { country, region });

            // Check if geo changed (VPN switch)
            let cached = state.public_ip_geo.read().await;
            let geo_changed = match (&*cached, &geo_info) {
                (Some(old), Some(new)) => old.country != new.country,
                (None, Some(_)) => true,
                _ => false,
            };
            drop(cached);

            if geo_changed {
                // Clear localhost binding when VPN changes
                state.bindings.remove(&client_key);
                tracing::info!("VPN change detected -> cleared binding, new IP {} (country: {:?}, region: {:?})",
                    public_ip,
                    geo_info.as_ref().map(|g| &g.country),
                    geo_info.as_ref().map(|g| &g.region));
            }

            // Update cache
            let mut cached = state.public_ip_geo.write().await;
            *cached = geo_info.clone();
            geo_info
        } else {
            state.public_ip_geo.read().await.clone()
        };
        new_geo
    } else {
        // Direct client - lookup geo
        state.geo.as_ref()
            .and_then(|g| g.lookup_ip(client_ip))
            .map(|(country, region)| GeoInfo { country, region })
    };

    let client_country = client_geo.as_ref().map(|g| g.country.clone());
    let client_region = client_geo.as_ref().map(|g| g.region.clone());

    // 1. Verifica binding existente
    let mut chosen_backend_id: Option<String> = None;
    if let Some(mut entry) = state.bindings.get_mut(&client_key) {
        entry.last_seen = now;
        chosen_backend_id = Some(entry.backend_id.clone());
    }

    // 2. Resolve backend
    let backend = if let Some(backend_id) = chosen_backend_id {
        let rt = state.routing.read().await;
        rt.backends
            .iter()
            .find(|b| b.id == backend_id && b.healthy)
            .cloned()
    } else {
        let rt = state.routing.read().await;
        if rt.backends.is_empty() {
            tracing::warn!("no backends configured");
            return Ok(());
        }

        let backend_opt = pick_backend(
            &rt.backends,
            &state.local_region,
            client_region.as_deref(),
            client_country.as_deref(),
            &state.metrics,
        );
        let backend = match backend_opt {
            Some(b) => b,
            None => {
                tracing::warn!("no healthy backend available");
                return Ok(());
            }
        };

        // cria binding
        state.bindings.insert(
            client_key.clone(),
            Binding {
                backend_id: backend.id.clone(),
                created_at: now,
                last_seen: now,
            },
        );

        Some(backend)
    };

    let backend = match backend {
        Some(b) => b,
        None => {
            state.bindings.remove(&client_key);
            tracing::warn!("binding backend not found, dropping connection");
            return Ok(());
        }
    };

    // Support both IPv4 and IPv6 addresses
    let backend_addr = if backend.wg_ip.contains(':') {
        // IPv6 address - wrap in brackets
        format!("[{}]:{}", backend.wg_ip, backend.port)
    } else {
        // IPv4 address
        format!("{}:{}", backend.wg_ip, backend.port)
    };
    tracing::debug!(
        "proxying {} -> {} ({})",
        client_ip,
        backend.id,
        backend_addr
    );

    // Métricas: conexão + RTT
    let t0 = Instant::now();
    let backend_stream = match TcpStream::connect(&backend_addr).await {
        Ok(s) => s,
        Err(e) => {
            tracing::error!(
                "failed to connect backend {} at {}: {:?}",
                backend.id,
                backend_addr,
                e
            );
            state.bindings.remove(&client_key);
            return Ok(());
        }
    };
    let rtt_ms = t0.elapsed().as_millis() as u64;

    let backend_id = backend.id.clone();
    {
        let metrics_entry = state
            .metrics
            .entry(backend_id.clone())
            .or_insert_with(BackendMetrics::new);
        metrics_entry.current_conns.fetch_add(1, Ordering::Relaxed);
        metrics_entry.last_rtt_ms.store(rtt_ms, Ordering::Relaxed);
    }

    let (mut client_read, mut client_write) = client_stream.into_split();
    let (mut backend_read, mut backend_write) = backend_stream.into_split();

    // Spawn separate tasks for each direction
    let client_to_backend = tokio::spawn(async move {
        let result = io::copy(&mut client_read, &mut backend_write).await;
        // Shutdown write side when client closes
        let _ = backend_write.shutdown().await;
        result
    });

    let backend_to_client = tokio::spawn(async move {
        let result = io::copy(&mut backend_read, &mut client_write).await;
        result
    });

    // Wait for both to complete
    let (c2b, b2c) = tokio::join!(client_to_backend, backend_to_client);

    if let Ok(Err(e)) = c2b {
        tracing::debug!("{} client->backend error: {:?}", backend_id, e);
    }
    if let Ok(Err(e)) = b2c {
        tracing::debug!("{} backend->client error: {:?}", backend_id, e);
    }

    // Decrement connection count
    if let Some(metrics) = state.metrics.get(&backend_id) {
        metrics.current_conns.fetch_sub(1, Ordering::Relaxed);
    }

    Ok(())
}
