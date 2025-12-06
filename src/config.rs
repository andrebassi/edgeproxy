use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    pub listen_addr: String,
    pub db_path: String,
    pub region: String,
    pub db_reload_secs: u64,
    pub geoip_path: Option<String>,
    pub binding_ttl_secs: u64,
    pub binding_gc_interval_secs: u64,
    pub debug: bool,
}

pub fn load_config() -> anyhow::Result<Config> {
    let listen_addr = std::env::var("EDGEPROXY_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let db_path = std::env::var("EDGEPROXY_DB_PATH")
        .unwrap_or_else(|_| "routing.db".to_string());

    let region = std::env::var("EDGEPROXY_REGION")
        .unwrap_or_else(|_| "sa".to_string());

    let db_reload_secs = std::env::var("EDGEPROXY_DB_RELOAD_SECS")
        .unwrap_or_else(|_| "5".to_string())
        .parse()
        .unwrap_or(5);

    let geoip_path = std::env::var("EDGEPROXY_GEOIP_PATH").ok();

    let binding_ttl_secs = std::env::var("EDGEPROXY_BINDING_TTL_SECS")
        .unwrap_or_else(|_| "600".to_string()) // 10min default
        .parse()
        .unwrap_or(600);

    let binding_gc_interval_secs = std::env::var("EDGEPROXY_BINDING_GC_INTERVAL_SECS")
        .unwrap_or_else(|_| "60".to_string())
        .parse()
        .unwrap_or(60);

    let debug = std::env::var("DEBUG").is_ok();

    Ok(Config {
        listen_addr,
        db_path,
        region,
        db_reload_secs,
        geoip_path,
        binding_ttl_secs,
        binding_gc_interval_secs,
        debug,
    })
}
