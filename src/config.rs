use serde::Deserialize;

#[derive(Debug, Deserialize, Clone)]
pub struct Config {
    // Core proxy settings
    pub listen_addr: String,
    pub db_path: String,
    pub region: String,
    pub db_reload_secs: u64,
    pub geoip_path: Option<String>,
    pub binding_ttl_secs: u64,
    pub binding_gc_interval_secs: u64,
    pub debug: bool,

    // TLS settings
    pub tls_enabled: bool,
    pub tls_cert_path: Option<String>,
    pub tls_key_path: Option<String>,
    pub tls_listen_addr: Option<String>,

    // Auto-Discovery API settings
    pub api_enabled: bool,
    pub api_listen_addr: String,
    pub heartbeat_ttl_secs: u64,

    // DNS server settings
    pub dns_enabled: bool,
    pub dns_listen_addr: String,
    pub dns_domain: String,

    // Built-in replication settings
    pub replication_enabled: bool,
    pub replication_node_id: Option<String>,
    pub replication_gossip_addr: String,
    pub replication_transport_addr: String,
    pub replication_bootstrap_peers: Vec<String>,
    pub replication_db_path: String,
    pub replication_cluster_name: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: "0.0.0.0:8080".to_string(),
            db_path: "routing.db".to_string(),
            region: "sa".to_string(),
            db_reload_secs: 5,
            geoip_path: None,
            binding_ttl_secs: 600,
            binding_gc_interval_secs: 60,
            debug: false,
            tls_enabled: false,
            tls_cert_path: None,
            tls_key_path: None,
            tls_listen_addr: None,
            api_enabled: false,
            api_listen_addr: "0.0.0.0:8081".to_string(),
            heartbeat_ttl_secs: 60,
            dns_enabled: false,
            dns_listen_addr: "0.0.0.0:5353".to_string(),
            dns_domain: "internal".to_string(),
            replication_enabled: false,
            replication_node_id: None,
            replication_gossip_addr: "0.0.0.0:4001".to_string(),
            replication_transport_addr: "0.0.0.0:4002".to_string(),
            replication_bootstrap_peers: Vec::new(),
            replication_db_path: "state.db".to_string(),
            replication_cluster_name: "edgeproxy".to_string(),
        }
    }
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
        .unwrap_or_else(|_| "600".to_string())
        .parse()
        .unwrap_or(600);

    let binding_gc_interval_secs = std::env::var("EDGEPROXY_BINDING_GC_INTERVAL_SECS")
        .unwrap_or_else(|_| "60".to_string())
        .parse()
        .unwrap_or(60);

    let debug = std::env::var("DEBUG").is_ok();

    // TLS settings
    let tls_enabled = std::env::var("EDGEPROXY_TLS_ENABLED")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let tls_cert_path = std::env::var("EDGEPROXY_TLS_CERT").ok();
    let tls_key_path = std::env::var("EDGEPROXY_TLS_KEY").ok();
    let tls_listen_addr = std::env::var("EDGEPROXY_TLS_LISTEN_ADDR").ok();

    // Auto-Discovery API settings
    let api_enabled = std::env::var("EDGEPROXY_API_ENABLED")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let api_listen_addr = std::env::var("EDGEPROXY_API_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:8081".to_string());

    let heartbeat_ttl_secs = std::env::var("EDGEPROXY_HEARTBEAT_TTL_SECS")
        .unwrap_or_else(|_| "60".to_string())
        .parse()
        .unwrap_or(60);

    // DNS server settings
    let dns_enabled = std::env::var("EDGEPROXY_DNS_ENABLED")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let dns_listen_addr = std::env::var("EDGEPROXY_DNS_LISTEN_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:5353".to_string());

    let dns_domain = std::env::var("EDGEPROXY_DNS_DOMAIN")
        .unwrap_or_else(|_| "internal".to_string());

    // Built-in replication settings
    let replication_enabled = std::env::var("EDGEPROXY_REPLICATION_ENABLED")
        .map(|v| v == "1" || v.to_lowercase() == "true")
        .unwrap_or(false);

    let replication_node_id = std::env::var("EDGEPROXY_REPLICATION_NODE_ID").ok();

    let replication_gossip_addr = std::env::var("EDGEPROXY_REPLICATION_GOSSIP_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4001".to_string());

    let replication_transport_addr = std::env::var("EDGEPROXY_REPLICATION_TRANSPORT_ADDR")
        .unwrap_or_else(|_| "0.0.0.0:4002".to_string());

    let replication_bootstrap_peers = std::env::var("EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS")
        .map(|v| v.split(',').map(|s| s.trim().to_string()).collect())
        .unwrap_or_default();

    let replication_db_path = std::env::var("EDGEPROXY_REPLICATION_DB_PATH")
        .unwrap_or_else(|_| "state.db".to_string());

    let replication_cluster_name = std::env::var("EDGEPROXY_REPLICATION_CLUSTER_NAME")
        .unwrap_or_else(|_| "edgeproxy".to_string());

    Ok(Config {
        listen_addr,
        db_path,
        region,
        db_reload_secs,
        geoip_path,
        binding_ttl_secs,
        binding_gc_interval_secs,
        debug,
        tls_enabled,
        tls_cert_path,
        tls_key_path,
        tls_listen_addr,
        api_enabled,
        api_listen_addr,
        heartbeat_ttl_secs,
        dns_enabled,
        dns_listen_addr,
        dns_domain,
        replication_enabled,
        replication_node_id,
        replication_gossip_addr,
        replication_transport_addr,
        replication_bootstrap_peers,
        replication_db_path,
        replication_cluster_name,
    })
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let cfg = Config::default();
        assert_eq!(cfg.listen_addr, "0.0.0.0:8080");
        assert_eq!(cfg.region, "sa");
        assert!(!cfg.tls_enabled);
        assert!(!cfg.api_enabled);
        assert!(!cfg.dns_enabled);
        assert!(!cfg.replication_enabled);
    }

    #[test]
    fn test_load_config_defaults() {
        std::env::remove_var("EDGEPROXY_LISTEN_ADDR");
        std::env::remove_var("EDGEPROXY_REGION");

        let cfg = load_config().unwrap();
        assert_eq!(cfg.listen_addr, "0.0.0.0:8080");
        assert_eq!(cfg.region, "sa");
        assert_eq!(cfg.binding_ttl_secs, 600);
    }

    #[test]
    fn test_tls_config_disabled_by_default() {
        std::env::remove_var("EDGEPROXY_TLS_ENABLED");
        let cfg = load_config().unwrap();
        assert!(!cfg.tls_enabled);
    }

    #[test]
    fn test_api_config_disabled_by_default() {
        std::env::remove_var("EDGEPROXY_API_ENABLED");
        let cfg = load_config().unwrap();
        assert!(!cfg.api_enabled);
    }

    #[test]
    fn test_dns_config_disabled_by_default() {
        std::env::remove_var("EDGEPROXY_DNS_ENABLED");
        let cfg = load_config().unwrap();
        assert!(!cfg.dns_enabled);
    }

    #[test]
    fn test_replication_config_disabled_by_default() {
        std::env::remove_var("EDGEPROXY_REPLICATION_ENABLED");
        let cfg = load_config().unwrap();
        assert!(!cfg.replication_enabled);
    }

    #[test]
    fn test_config_clone() {
        let cfg = Config::default();
        let cloned = cfg.clone();
        assert_eq!(cfg.listen_addr, cloned.listen_addr);
        assert_eq!(cfg.region, cloned.region);
    }

    #[test]
    fn test_config_debug() {
        let cfg = Config::default();
        let debug_str = format!("{:?}", cfg);
        assert!(debug_str.contains("listen_addr"));
        assert!(debug_str.contains("0.0.0.0:8080"));
    }

    #[test]
    fn test_load_config_with_tls_enabled_true() {
        std::env::set_var("EDGEPROXY_TLS_ENABLED", "true");
        let cfg = load_config().unwrap();
        assert!(cfg.tls_enabled);
        std::env::remove_var("EDGEPROXY_TLS_ENABLED");
    }

    #[test]
    fn test_load_config_with_tls_enabled_1() {
        std::env::set_var("EDGEPROXY_TLS_ENABLED", "1");
        let cfg = load_config().unwrap();
        assert!(cfg.tls_enabled);
        std::env::remove_var("EDGEPROXY_TLS_ENABLED");
    }

    #[test]
    fn test_load_config_with_api_enabled_true() {
        std::env::set_var("EDGEPROXY_API_ENABLED", "TRUE");
        let cfg = load_config().unwrap();
        assert!(cfg.api_enabled);
        std::env::remove_var("EDGEPROXY_API_ENABLED");
    }

    #[test]
    fn test_load_config_with_dns_enabled_true() {
        std::env::set_var("EDGEPROXY_DNS_ENABLED", "1");
        let cfg = load_config().unwrap();
        assert!(cfg.dns_enabled);
        std::env::remove_var("EDGEPROXY_DNS_ENABLED");
    }

    #[test]
    fn test_load_config_with_replication_enabled() {
        std::env::set_var("EDGEPROXY_REPLICATION_ENABLED", "true");
        std::env::set_var("EDGEPROXY_REPLICATION_NODE_ID", "pop-sa-1");
        std::env::set_var("EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS", "peer1:4001,peer2:4001");
        let cfg = load_config().unwrap();
        assert!(cfg.replication_enabled);
        assert_eq!(cfg.replication_node_id, Some("pop-sa-1".to_string()));
        assert_eq!(cfg.replication_bootstrap_peers.len(), 2);
        std::env::remove_var("EDGEPROXY_REPLICATION_ENABLED");
        std::env::remove_var("EDGEPROXY_REPLICATION_NODE_ID");
        std::env::remove_var("EDGEPROXY_REPLICATION_BOOTSTRAP_PEERS");
    }

    #[test]
    fn test_load_config_with_custom_listen_addr() {
        std::env::set_var("EDGEPROXY_LISTEN_ADDR", "127.0.0.1:9000");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.listen_addr, "127.0.0.1:9000");
        std::env::remove_var("EDGEPROXY_LISTEN_ADDR");
    }

    #[test]
    fn test_load_config_with_custom_db_path() {
        std::env::set_var("EDGEPROXY_DB_PATH", "/tmp/test.db");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.db_path, "/tmp/test.db");
        std::env::remove_var("EDGEPROXY_DB_PATH");
    }

    #[test]
    fn test_load_config_with_custom_region() {
        std::env::set_var("EDGEPROXY_REGION", "eu");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.region, "eu");
        std::env::remove_var("EDGEPROXY_REGION");
    }

    #[test]
    fn test_load_config_with_geoip_path() {
        std::env::set_var("EDGEPROXY_GEOIP_PATH", "/path/to/GeoLite2.mmdb");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.geoip_path, Some("/path/to/GeoLite2.mmdb".to_string()));
        std::env::remove_var("EDGEPROXY_GEOIP_PATH");
    }

    #[test]
    fn test_load_config_with_debug() {
        std::env::set_var("DEBUG", "1");
        let cfg = load_config().unwrap();
        assert!(cfg.debug);
        std::env::remove_var("DEBUG");
    }

    #[test]
    fn test_load_config_with_tls_paths() {
        std::env::set_var("EDGEPROXY_TLS_CERT", "/path/to/cert.pem");
        std::env::set_var("EDGEPROXY_TLS_KEY", "/path/to/key.pem");
        std::env::set_var("EDGEPROXY_TLS_LISTEN_ADDR", "0.0.0.0:8443");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.tls_cert_path, Some("/path/to/cert.pem".to_string()));
        assert_eq!(cfg.tls_key_path, Some("/path/to/key.pem".to_string()));
        assert_eq!(cfg.tls_listen_addr, Some("0.0.0.0:8443".to_string()));
        std::env::remove_var("EDGEPROXY_TLS_CERT");
        std::env::remove_var("EDGEPROXY_TLS_KEY");
        std::env::remove_var("EDGEPROXY_TLS_LISTEN_ADDR");
    }

    #[test]
    fn test_load_config_with_api_settings() {
        std::env::set_var("EDGEPROXY_API_LISTEN_ADDR", "0.0.0.0:9081");
        std::env::set_var("EDGEPROXY_HEARTBEAT_TTL_SECS", "120");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.api_listen_addr, "0.0.0.0:9081");
        assert_eq!(cfg.heartbeat_ttl_secs, 120);
        std::env::remove_var("EDGEPROXY_API_LISTEN_ADDR");
        std::env::remove_var("EDGEPROXY_HEARTBEAT_TTL_SECS");
    }

    #[test]
    fn test_load_config_with_dns_settings() {
        std::env::set_var("EDGEPROXY_DNS_LISTEN_ADDR", "0.0.0.0:5354");
        std::env::set_var("EDGEPROXY_DNS_DOMAIN", "edge.local");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.dns_listen_addr, "0.0.0.0:5354");
        assert_eq!(cfg.dns_domain, "edge.local");
        std::env::remove_var("EDGEPROXY_DNS_LISTEN_ADDR");
        std::env::remove_var("EDGEPROXY_DNS_DOMAIN");
    }

    #[test]
    fn test_load_config_with_binding_settings() {
        std::env::set_var("EDGEPROXY_BINDING_TTL_SECS", "1200");
        std::env::set_var("EDGEPROXY_BINDING_GC_INTERVAL_SECS", "120");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.binding_ttl_secs, 1200);
        assert_eq!(cfg.binding_gc_interval_secs, 120);
        std::env::remove_var("EDGEPROXY_BINDING_TTL_SECS");
        std::env::remove_var("EDGEPROXY_BINDING_GC_INTERVAL_SECS");
    }

    #[test]
    fn test_load_config_with_db_reload() {
        std::env::set_var("EDGEPROXY_DB_RELOAD_SECS", "30");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.db_reload_secs, 30);
        std::env::remove_var("EDGEPROXY_DB_RELOAD_SECS");
    }

    #[test]
    fn test_load_config_parse_error_uses_default() {
        std::env::set_var("EDGEPROXY_DB_RELOAD_SECS", "not_a_number");
        let cfg = load_config().unwrap();
        assert_eq!(cfg.db_reload_secs, 5); // default
        std::env::remove_var("EDGEPROXY_DB_RELOAD_SECS");
    }
}
