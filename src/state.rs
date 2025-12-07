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

/// Embedded GeoLite2-Country database (compiled into binary)
const EMBEDDED_GEOIP: &[u8] = include_bytes!("../GeoLite2-Country.mmdb");

/// GeoDB (MaxMind)
#[derive(Clone)]
pub struct GeoDb {
    reader: Arc<Reader<Vec<u8>>>,
}

impl GeoDb {
    /// Load embedded GeoIP database from binary
    pub fn embedded() -> anyhow::Result<Self> {
        let reader = Reader::from_source(EMBEDDED_GEOIP.to_vec())?;
        Ok(Self {
            reader: Arc::new(reader),
        })
    }

    pub fn open(path: &str) -> anyhow::Result<Self> {
        let reader = Reader::open_readfile(path)?;
        Ok(Self {
            reader: Arc::new(reader),
        })
    }

    /// Mapeia IP -> (país, região) - ex: ("FR", "eu")
    pub fn lookup_ip(&self, ip: IpAddr) -> Option<(String, String)> {
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
            // EU
            "PT" | "ES" | "FR" | "DE" | "NL" | "IT" | "GB" | "IE" | "BE" | "CH" | "AT" | "PL" | "CZ" | "SE" | "NO" | "DK" | "FI" => "eu",
            // AP
            "JP" | "KR" | "TW" | "HK" | "SG" | "MY" | "TH" | "VN" | "ID" | "PH" | "AU" | "NZ" => "ap",
            _ => "us", // fallback
        };

        Some((iso.to_string(), region.to_string()))
    }

}

/// Cached geo info (country, region)
#[derive(Clone, Debug)]
pub struct GeoInfo {
    pub country: String,
    pub region: String,
}

/// Estado compartilhado do edgeProxy
#[derive(Clone)]
pub struct RcProxyState {
    pub routing: Arc<RwLock<RoutingState>>,
    pub bindings: Arc<DashMap<ClientKey, Binding>>,
    pub local_region: String,
    pub geo: Option<GeoDb>,
    pub metrics: Arc<DashMap<String, BackendMetrics>>, // backend_id -> métricas
    pub public_ip_geo: Arc<RwLock<Option<GeoInfo>>>,   // cached public IP geo (country, region)
}

impl RcProxyState {
    pub fn new(local_region: String, geo: Option<GeoDb>) -> Self {
        Self {
            routing: Arc::new(RwLock::new(RoutingState::default())),
            bindings: Arc::new(DashMap::new()),
            local_region,
            geo,
            metrics: Arc::new(DashMap::new()),
            public_ip_geo: Arc::new(RwLock::new(None)),
        }
    }
}

/// Fetch public IP from AWS checkip service
pub async fn fetch_public_ip() -> Option<IpAddr> {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
    {
        Ok(c) => c,
        Err(_) => return None,
    };

    let resp = match client
        .get("https://checkip.amazonaws.com/")
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("failed to fetch public IP: {}", e);
            return None;
        }
    };

    let text = match resp.text().await {
        Ok(t) => t.trim().to_string(),
        Err(_) => return None,
    };

    match text.parse::<IpAddr>() {
        Ok(ip) => {
            tracing::info!("public IP detected: {}", ip);
            Some(ip)
        }
        Err(_) => None,
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
