use crate::lb::BackendMetrics;
use crate::model::{Binding, ClientKey, RoutingState};
use dashmap::DashMap;
use maxminddb::Reader;
use serde::Deserialize;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tokio::time::sleep;

/// GeoDB (MaxMind)
#[derive(Clone)]
pub struct GeoDb {
    reader: Arc<Reader<Vec<u8>>>,
}

impl GeoDb {
    pub fn open(path: &str) -> anyhow::Result<Self> {
        let reader = Reader::open_readfile(path)?;
        Ok(Self {
            reader: Arc::new(reader),
        })
    }

    /// Mapeia IP -> região lógica ("sa", "us", "eu"...)
    pub fn region_for_ip(&self, ip: IpAddr) -> Option<String> {
        #[derive(Debug, Deserialize)]
        struct Country {
            iso_code: Option<String>,
        }
        #[derive(Debug, Deserialize)]
        struct CountryResp {
            country: Option<Country>,
        }

        let resp: CountryResp = self.reader.lookup(ip).ok()?;
        let iso = resp.country?.iso_code?;

        let region = match iso.as_str() {
            // LATAM
            "BR" | "AR" | "CL" | "PE" | "CO" | "UY" | "PY" | "BO" | "EC" => "sa",
            // NA
            "US" | "CA" | "MX" => "us",
            // EU (exemplo simplificado)
            "PT" | "ES" | "FR" | "DE" | "NL" | "IT" | "GB" | "IE" | "BE" | "CH" => "eu",
            _ => "us", // fallback
        };

        Some(region.to_string())
    }
}

/// Estado compartilhado do edgeProxy
#[derive(Clone)]
pub struct RcProxyState {
    pub routing: Arc<RwLock<RoutingState>>,
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    pub local_region: String,
    pub geo: Option<GeoDb>,
    pub metrics: Arc<DashMap<String, BackendMetrics>>, // backend_id -> métricas
}

impl RcProxyState {
    pub fn new(local_region: String, geo: Option<GeoDb>) -> Self {
        Self {
            routing: Arc::new(RwLock::new(RoutingState::default())),
            bindings: Arc::new(DashMap::new()),
            local_region,
            geo,
            metrics: Arc::new(DashMap::new()),
        }
    }
}

/// GC de bindings baseado em TTL
pub fn start_binding_gc(
    bindings: Arc<DashMap<ClientKey, Binding>>,
    ttl: Duration,
    interval: Duration,
) {
    tokio::spawn(async move {
        loop {
            let now = Instant::now();
            let mut to_remove = Vec::new();

            for entry in bindings.iter() {
                let b = entry.value();
                if now.duration_since(b.last_seen) > ttl {
                    to_remove.push(entry.key().clone());
                }
            }

            for key in to_remove {
                bindings.remove(&key);
            }

            sleep(interval).await;
        }
    });
}
