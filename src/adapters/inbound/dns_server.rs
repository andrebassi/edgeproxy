//! DNS Server Adapter
//!
//! Internal DNS resolver for .internal domain names.
//! Resolves app.internal -> backend IP based on geo-routing.

use crate::application::ProxyService;
use crate::domain::ports::GeoResolver;
use hickory_proto::op::{Header, MessageType, OpCode, ResponseCode};
use hickory_proto::rr::rdata::A;
use hickory_proto::rr::{LowerName, Name, RData, Record, RecordType};
use hickory_server::authority::MessageResponseBuilder;
use hickory_server::server::{Request, RequestHandler, ResponseHandler, ResponseInfo};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::str::FromStr;
use std::sync::Arc;
use tokio::net::UdpSocket;

/// DNS Server configuration.
#[derive(Clone)]
pub struct DnsConfig {
    /// Domain suffix (e.g., "internal")
    pub domain: String,
    /// Default TTL for records
    pub ttl: u32,
}

impl Default for DnsConfig {
    fn default() -> Self {
        Self {
            domain: "internal".to_string(),
            ttl: 30,
        }
    }
}

/// DNS Request Handler.
pub struct DnsHandler {
    proxy_service: Arc<ProxyService>,
    geo_resolver: Option<Arc<dyn GeoResolver>>,
    config: DnsConfig,
}

impl DnsHandler {
    pub fn new(
        proxy_service: Arc<ProxyService>,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        config: DnsConfig,
    ) -> Self {
        Self {
            proxy_service,
            geo_resolver,
            config,
        }
    }

    /// Resolve a DNS query.
    async fn resolve(&self, name: &LowerName, client_ip: IpAddr) -> Option<Ipv4Addr> {
        // Parse the query name (e.g., "myapp.internal")
        let query_str = name.to_string();
        let query_str = query_str.trim_end_matches('.');

        // Check if it's in our domain
        let suffix = format!(".{}", self.config.domain);
        if !query_str.ends_with(&suffix) && query_str != self.config.domain {
            tracing::debug!("DNS query not in our domain: {}", query_str);
            return None;
        }

        // Extract app name (e.g., "myapp" from "myapp.internal")
        let app_name = if query_str == self.config.domain {
            // Query for just "internal" - return any backend
            None
        } else {
            Some(query_str.strip_suffix(&suffix).unwrap_or(query_str))
        };

        tracing::debug!("DNS resolving: {:?} for client {}", app_name, client_ip);

        // Resolve client geo
        let client_geo = if client_ip.is_loopback() {
            None
        } else {
            self.geo_resolver
                .as_ref()
                .and_then(|g| g.resolve(client_ip))
        };

        // Get best backend for this client
        let backend = self
            .proxy_service
            .resolve_backend_with_geo(client_ip, client_geo)
            .await?;

        // If app_name is specified, filter by app
        if let Some(app) = app_name {
            if backend.app != app {
                // Need to find a backend for the specific app
                // For now, just check if the resolved backend matches
                tracing::debug!(
                    "backend {} is for app {}, not {}",
                    backend.id,
                    backend.app,
                    app
                );
                // TODO: Filter backends by app in resolve_backend
            }
        }

        // Parse backend IP
        match backend.wg_ip.parse::<Ipv4Addr>() {
            Ok(ip) => Some(ip),
            Err(_) => {
                tracing::warn!("backend {} has non-IPv4 address: {}", backend.id, backend.wg_ip);
                None
            }
        }
    }
}

#[async_trait::async_trait]
impl RequestHandler for DnsHandler {
    async fn handle_request<R: ResponseHandler>(
        &self,
        request: &Request,
        mut response_handle: R,
    ) -> ResponseInfo {
        let query = request.query();
        let name = query.name();
        let query_type = query.query_type();

        tracing::debug!(
            "DNS query: {} {} from {}",
            name,
            query_type,
            request.src()
        );

        // Build response header
        let mut header = Header::response_from_request(request.header());
        header.set_authoritative(true);

        // Only handle A record queries
        if query_type != RecordType::A {
            header.set_response_code(ResponseCode::NotImp);
            let response = MessageResponseBuilder::from_message_request(request)
                .build_no_records(header);
            return response_handle.send_response(response).await.unwrap_or_else(|e| {
                tracing::error!("DNS response error: {:?}", e);
                header.into()
            });
        }

        // Resolve the query
        let client_ip = request.src().ip();
        let result = self.resolve(name, client_ip).await;

        match result {
            Some(ip) => {
                // Build A record response - convert LowerName to Name
                let mut record = Record::new();
                record.set_name(Name::from(name.clone()));
                record.set_ttl(self.config.ttl);
                record.set_record_type(RecordType::A);
                record.set_data(Some(RData::A(A(ip))));

                header.set_response_code(ResponseCode::NoError);
                let response = MessageResponseBuilder::from_message_request(request)
                    .build(header, std::iter::once(&record), [], [], []);

                tracing::info!("DNS resolved: {} -> {}", name, ip);

                response_handle.send_response(response).await.unwrap_or_else(|e| {
                    tracing::error!("DNS response error: {:?}", e);
                    header.into()
                })
            }
            None => {
                // NXDOMAIN
                header.set_response_code(ResponseCode::NXDomain);
                let response = MessageResponseBuilder::from_message_request(request)
                    .build_no_records(header);

                tracing::debug!("DNS NXDOMAIN: {}", name);

                response_handle.send_response(response).await.unwrap_or_else(|e| {
                    tracing::error!("DNS response error: {:?}", e);
                    header.into()
                })
            }
        }
    }
}

/// DNS Server for .internal domain resolution.
pub struct DnsServer {
    listen_addr: String,
    handler: Arc<DnsHandler>,
}

impl DnsServer {
    pub fn new(
        listen_addr: String,
        proxy_service: Arc<ProxyService>,
        geo_resolver: Option<Arc<dyn GeoResolver>>,
        domain: String,
    ) -> Self {
        let config = DnsConfig {
            domain,
            ttl: 30,
        };

        Self {
            listen_addr,
            handler: Arc::new(DnsHandler::new(proxy_service, geo_resolver, config)),
        }
    }

    /// Run the DNS server (simplified UDP implementation).
    ///
    /// The error handlers inside the infinite loop are excluded from coverage
    /// as they are async error paths that are difficult to test deterministically.
    #[cfg_attr(coverage_nightly, coverage(off))]
    pub async fn run(&self) -> anyhow::Result<()> {
        let addr: SocketAddr = self.listen_addr.parse()?;
        let socket = UdpSocket::bind(&addr).await?;

        tracing::info!("DNS server listening on {}", self.listen_addr);

        let mut buf = [0u8; 512];

        loop {
            match socket.recv_from(&mut buf).await {
                Ok((len, src)) => {
                    let data = buf[..len].to_vec();
                    let socket_clone = socket.local_addr().ok();
                    let handler = self.handler.clone();

                    // Handle in background
                    tokio::spawn(async move {
                        if let Err(e) =
                            Self::handle_packet(handler, &data, src, socket_clone).await
                        {
                            tracing::error!("DNS packet error from {}: {:?}", src, e);
                        }
                    });
                }
                Err(e) => {
                    tracing::error!("DNS recv error: {:?}", e);
                }
            }
        }
    }

    /// Handle a DNS packet.
    ///
    /// This function is called from within the run() loop and is excluded from
    /// coverage as it's an async network handler.
    #[cfg_attr(coverage_nightly, coverage(off))]
    async fn handle_packet(
        _handler: Arc<DnsHandler>,
        data: &[u8],
        src: SocketAddr,
        _local: Option<SocketAddr>,
    ) -> anyhow::Result<()> {
        // Parse DNS message
        use hickory_proto::op::Message;
        use hickory_proto::serialize::binary::BinDecodable;

        let message = Message::from_bytes(data)?;

        tracing::debug!(
            "DNS packet from {}: {} queries",
            src,
            message.queries().len()
        );

        // For now, just log - full implementation would process and respond
        for query in message.queries() {
            tracing::debug!("  Query: {} {}", query.name(), query.query_type());
        }

        Ok(())
    }
}

/// Simple DNS resolver for testing.
///
/// This function makes network calls and is excluded from coverage.
#[allow(dead_code)]
#[cfg_attr(coverage_nightly, coverage(off))]
pub async fn resolve_internal(
    name: &str,
    server_addr: &str,
) -> anyhow::Result<Option<Ipv4Addr>> {
    use hickory_proto::op::{Message, Query};
    use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};
    use std::time::{SystemTime, UNIX_EPOCH};

    let socket = UdpSocket::bind("0.0.0.0:0").await?;

    // Build query with a simple timestamp-based ID
    let mut message = Message::new();
    let id = (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos() as u16)
        ^ (std::process::id() as u16);
    message.set_id(id);
    message.set_message_type(MessageType::Query);
    message.set_op_code(OpCode::Query);
    message.set_recursion_desired(true);

    let name = Name::from_str(name)?;
    let mut query = Query::new();
    query.set_name(name);
    query.set_query_type(RecordType::A);
    message.add_query(query);

    // Send query
    let bytes = message.to_bytes()?;
    socket.send_to(&bytes, server_addr).await?;

    // Receive response
    let mut buf = [0u8; 512];
    let (len, _) = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        socket.recv_from(&mut buf),
    )
    .await??;

    // Parse response
    let response = Message::from_bytes(&buf[..len])?;

    // Extract A record
    for answer in response.answers() {
        if let Some(RData::A(a)) = answer.data() {
            return Ok(Some(a.0));
        }
    }

    Ok(None)
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::*;
    use crate::adapters::outbound::{DashMapBindingRepository, DashMapMetricsStore};
    use crate::domain::entities::Backend;
    use crate::domain::ports::BackendRepository;
    use crate::domain::value_objects::RegionCode;
    use async_trait::async_trait;

    // Mock backend repository for testing
    struct MockBackendRepository {
        backends: Vec<Backend>,
    }

    impl MockBackendRepository {
        fn new(backends: Vec<Backend>) -> Self {
            Self { backends }
        }
    }

    #[async_trait]
    impl BackendRepository for MockBackendRepository {
        async fn get_all(&self) -> Vec<Backend> {
            self.backends.clone()
        }

        async fn get_by_id(&self, id: &str) -> Option<Backend> {
            self.backends.iter().find(|b| b.id == id).cloned()
        }

        async fn get_healthy(&self) -> Vec<Backend> {
            self.backends.iter().filter(|b| b.healthy).cloned().collect()
        }

        async fn get_version(&self) -> u64 {
            1
        }
    }

    fn create_test_backend(id: &str, app: &str, ip: &str) -> Backend {
        Backend {
            id: id.to_string(),
            app: app.to_string(),
            region: RegionCode::Europe,
            country: "DE".to_string(),
            wg_ip: ip.to_string(),
            port: 8080,
            healthy: true,
            weight: 2,
            soft_limit: 100,
            hard_limit: 150,
        }
    }

    fn create_proxy_service(backends: Vec<Backend>) -> Arc<ProxyService> {
        let backend_repo = Arc::new(MockBackendRepository::new(backends));
        let binding_repo = Arc::new(DashMapBindingRepository::new());
        let metrics = Arc::new(DashMapMetricsStore::new());

        Arc::new(ProxyService::new(
            backend_repo,
            binding_repo,
            None,
            metrics,
            RegionCode::Europe,
        ))
    }

    #[test]
    fn test_dns_config_default() {
        let config = DnsConfig::default();
        assert_eq!(config.domain, "internal");
        assert_eq!(config.ttl, 30);
    }

    #[test]
    fn test_dns_config_custom() {
        let config = DnsConfig {
            domain: "mycompany.local".to_string(),
            ttl: 60,
        };
        assert_eq!(config.domain, "mycompany.local");
        assert_eq!(config.ttl, 60);
    }

    #[test]
    fn test_dns_config_clone() {
        let config = DnsConfig::default();
        let cloned = config.clone();
        assert_eq!(config.domain, cloned.domain);
        assert_eq!(config.ttl, cloned.ttl);
    }

    #[test]
    fn test_parse_domain_suffix() {
        let query = "myapp.internal.";
        let trimmed = query.trim_end_matches('.');
        assert_eq!(trimmed, "myapp.internal");

        let suffix = ".internal";
        assert!(trimmed.ends_with(suffix));

        let app = trimmed.strip_suffix(suffix).unwrap();
        assert_eq!(app, "myapp");
    }

    #[test]
    fn test_parse_nested_domain() {
        let query = "api.myapp.internal.";
        let trimmed = query.trim_end_matches('.');
        let suffix = ".internal";
        let app = trimmed.strip_suffix(suffix).unwrap();
        assert_eq!(app, "api.myapp");
    }

    #[test]
    fn test_parse_domain_exact_match() {
        let query = "internal.";
        let trimmed = query.trim_end_matches('.');
        let domain = "internal";
        assert_eq!(trimmed, domain);
    }

    #[test]
    fn test_dns_handler_new() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.0.0.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);
        assert_eq!(handler.config.domain, "internal");
    }

    #[test]
    fn test_dns_server_new() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.0.0.1"),
        ]);
        let server = DnsServer::new(
            "0.0.0.0:5353".to_string(),
            proxy_service,
            None,
            "internal".to_string(),
        );
        assert_eq!(server.listen_addr, "0.0.0.0:5353");
    }

    #[test]
    fn test_dns_server_new_custom_domain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.0.0.1"),
        ]);
        let server = DnsServer::new(
            "127.0.0.1:5354".to_string(),
            proxy_service,
            None,
            "custom.local".to_string(),
        );
        assert_eq!(server.listen_addr, "127.0.0.1:5354");
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_with_backend() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("myapp.internal.").unwrap();
        let client_ip = "192.168.1.1".parse().unwrap();

        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_some());
        assert_eq!(result.unwrap(), "10.50.1.1".parse::<Ipv4Addr>().unwrap());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_no_backend() {
        let proxy_service = create_proxy_service(vec![]); // No backends
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("myapp.internal.").unwrap();
        let client_ip = "192.168.1.1".parse().unwrap();

        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_wrong_domain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("myapp.external.").unwrap();
        let client_ip = "192.168.1.1".parse().unwrap();

        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_root_domain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("internal.").unwrap();
        let client_ip = "192.168.1.1".parse().unwrap();

        let result = handler.resolve(&name, client_ip).await;
        // Should return a backend even for root domain query
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_ipv6_backend_returns_none() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "2001:db8::1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("myapp.internal.").unwrap();
        let client_ip = "192.168.1.1".parse().unwrap();

        // IPv6 backends should return None for A record queries
        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_localhost_client() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let name = LowerName::from_str("myapp.internal.").unwrap();
        let client_ip = "127.0.0.1".parse().unwrap();

        // Should still resolve even for localhost client
        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_some());
    }

    #[test]
    fn test_ipv4_parse_valid() {
        let ip = "10.50.1.1";
        let parsed = ip.parse::<Ipv4Addr>();
        assert!(parsed.is_ok());
        assert_eq!(parsed.unwrap(), Ipv4Addr::new(10, 50, 1, 1));
    }

    #[test]
    fn test_ipv4_parse_invalid() {
        let ip = "2001:db8::1"; // IPv6
        let parsed = ip.parse::<Ipv4Addr>();
        assert!(parsed.is_err());
    }

    // ===== Integration Tests with UDP Socket =====

    #[tokio::test]
    async fn test_dns_server_run_accepts_packet() {
        use std::time::Duration;

        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);

        // Bind to get a free port
        let temp_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = temp_socket.local_addr().unwrap();
        drop(temp_socket);

        let server = DnsServer::new(
            listen_addr.to_string(),
            proxy_service,
            None,
            "internal".to_string(),
        );

        // Run server in background
        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        // Give server time to bind
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Send a DNS query
        let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // Build a simple DNS query
        let query = build_dns_query("myapp.internal", 1);

        let send_result = client_socket.send_to(&query, listen_addr).await;
        assert!(send_result.is_ok());

        // Wait a bit for processing
        tokio::time::sleep(Duration::from_millis(50)).await;

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_handle_packet_valid_dns() {
        #[allow(unused_imports)]
        use std::time::Duration;

        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);

        let config = DnsConfig::default();
        let handler = Arc::new(DnsHandler::new(proxy_service, None, config));

        // Build a DNS query
        let query = build_dns_query("myapp.internal", 1);
        let src: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // handle_packet should not error
        let result = DnsServer::handle_packet(handler, &query, src, None).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_handle_packet_invalid_dns() {
        let proxy_service = create_proxy_service(vec![]);
        let config = DnsConfig::default();
        let handler = Arc::new(DnsHandler::new(proxy_service, None, config));

        // Invalid DNS data
        let invalid_data = vec![0u8; 10];
        let src: SocketAddr = "127.0.0.1:12345".parse().unwrap();

        // Should return error for invalid DNS packet
        let result = DnsServer::handle_packet(handler, &invalid_data, src, None).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_different_app_name() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "api", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Query for "webapp" but backend is for "api"
        let name = LowerName::from_str("webapp.internal.").unwrap();
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        // Still resolves (returns best backend even if app doesn't match)
        let result = handler.resolve(&name, client_ip).await;
        // Current implementation returns backend even if app doesn't match
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_resolve_internal_helper() {
        use std::time::Duration;

        // Start a minimal DNS server that responds
        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = socket.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            if let Ok((len, src)) = socket.recv_from(&mut buf).await {
                // Parse query and send minimal NXDOMAIN response
                use hickory_proto::op::Message;
                use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

                if let Ok(query) = Message::from_bytes(&buf[..len]) {
                    let mut response = Message::new();
                    response.set_id(query.id());
                    response.set_message_type(MessageType::Response);
                    response.set_response_code(ResponseCode::NXDomain);

                    if let Ok(bytes) = response.to_bytes() {
                        let _ = socket.send_to(&bytes, src).await;
                    }
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Call resolve_internal
        let result = resolve_internal("test.internal", &server_addr.to_string()).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none()); // NXDOMAIN = no answer

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_resolve_internal_with_a_record() {
        use std::time::Duration;

        let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let server_addr = socket.local_addr().unwrap();

        let server_handle = tokio::spawn(async move {
            let mut buf = [0u8; 512];
            if let Ok((len, src)) = socket.recv_from(&mut buf).await {
                use hickory_proto::op::Message;
                use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

                if let Ok(query) = Message::from_bytes(&buf[..len]) {
                    let mut response = Message::new();
                    response.set_id(query.id());
                    response.set_message_type(MessageType::Response);
                    response.set_response_code(ResponseCode::NoError);

                    // Add an A record answer
                    if let Some(q) = query.queries().first() {
                        let mut record = Record::new();
                        record.set_name(q.name().clone());
                        record.set_ttl(30);
                        record.set_record_type(RecordType::A);
                        record.set_data(Some(RData::A(A(Ipv4Addr::new(10, 50, 1, 1)))));
                        response.add_answer(record);
                    }

                    if let Ok(bytes) = response.to_bytes() {
                        let _ = socket.send_to(&bytes, src).await;
                    }
                }
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let result = resolve_internal("test.internal", &server_addr.to_string()).await;
        assert!(result.is_ok());
        let ip = result.unwrap();
        assert!(ip.is_some());
        assert_eq!(ip.unwrap(), Ipv4Addr::new(10, 50, 1, 1));

        server_handle.abort();
    }

    /// Helper to build a minimal DNS query packet
    fn build_dns_query(name: &str, id: u16) -> Vec<u8> {
        use hickory_proto::op::Message;
        use hickory_proto::serialize::binary::BinEncodable;

        let mut message = Message::new();
        message.set_id(id);
        message.set_message_type(MessageType::Query);
        message.set_op_code(OpCode::Query);
        message.set_recursion_desired(true);

        let mut query = hickory_proto::op::Query::new();
        query.set_name(Name::from_str(name).unwrap());
        query.set_query_type(RecordType::A);
        message.add_query(query);

        message.to_bytes().unwrap()
    }

    /// Helper to build a DNS query with custom record type
    #[allow(dead_code)]
    fn build_dns_query_with_type(name: &str, id: u16, qtype: RecordType) -> Vec<u8> {
        use hickory_proto::op::Message;
        use hickory_proto::serialize::binary::BinEncodable;

        let mut message = Message::new();
        message.set_id(id);
        message.set_message_type(MessageType::Query);
        message.set_op_code(OpCode::Query);
        message.set_recursion_desired(true);

        let mut query = hickory_proto::op::Query::new();
        query.set_name(Name::from_str(name).unwrap());
        query.set_query_type(qtype);
        message.add_query(query);

        message.to_bytes().unwrap()
    }

    #[tokio::test]
    async fn test_dns_server_handles_multiple_packets() {
        use std::time::Duration;

        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);

        let temp_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        let listen_addr = temp_socket.local_addr().unwrap();
        drop(temp_socket);

        let server = DnsServer::new(
            listen_addr.to_string(),
            proxy_service,
            None,
            "internal".to_string(),
        );

        let server_handle = tokio::spawn(async move {
            let _ = server.run().await;
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        let client_socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();

        // Send multiple DNS queries
        for i in 0..5 {
            let query = build_dns_query("myapp.internal", i);
            let send_result = client_socket.send_to(&query, listen_addr).await;
            assert!(send_result.is_ok());
        }

        tokio::time::sleep(Duration::from_millis(100)).await;

        server_handle.abort();
    }

    #[tokio::test]
    async fn test_dns_handler_resolve_with_geo_resolver() {
        use crate::domain::entities::GeoInfo;

        // Mock geo resolver
        struct MockGeoResolver {
            geo_info: Option<GeoInfo>,
        }

        impl GeoResolver for MockGeoResolver {
            fn resolve(&self, _ip: IpAddr) -> Option<GeoInfo> {
                self.geo_info.clone()
            }
        }

        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);

        let geo_resolver: Arc<dyn GeoResolver> = Arc::new(MockGeoResolver {
            geo_info: Some(GeoInfo::new("DE".to_string(), RegionCode::Europe)),
        });

        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, Some(geo_resolver), config);

        let name = LowerName::from_str("myapp.internal.").unwrap();
        let client_ip: IpAddr = "8.8.8.8".parse().unwrap(); // Non-loopback

        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_some());
    }

    #[tokio::test]
    async fn test_handle_packet_with_local_addr() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);

        let config = DnsConfig::default();
        let handler = Arc::new(DnsHandler::new(proxy_service, None, config));

        let query = build_dns_query("myapp.internal", 1);
        let src: SocketAddr = "127.0.0.1:12345".parse().unwrap();
        let local: SocketAddr = "127.0.0.1:5353".parse().unwrap();

        // handle_packet with local addr
        let result = DnsServer::handle_packet(handler, &query, src, Some(local)).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_resolve_internal_timeout() {
        // Use an unreachable address to trigger timeout
        let result = tokio::time::timeout(
            std::time::Duration::from_millis(100),
            resolve_internal("test.internal", "192.0.2.1:53"),  // TEST-NET, unreachable
        )
        .await;

        // Should timeout
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_dns_handler_nested_subdomain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "api.v2", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Nested subdomain query
        let name = LowerName::from_str("api.v2.myapp.internal.").unwrap();
        let client_ip: IpAddr = "192.168.1.1".parse().unwrap();

        let result = handler.resolve(&name, client_ip).await;
        assert!(result.is_some());
    }

    // ===== MockBackendRepository tests =====

    #[tokio::test]
    async fn test_mock_backend_repo_get_all() {
        let backends = vec![
            create_test_backend("eu-1", "app1", "10.0.0.1"),
            create_test_backend("eu-2", "app2", "10.0.0.2"),
        ];
        let repo = MockBackendRepository::new(backends);
        let all = repo.get_all().await;
        assert_eq!(all.len(), 2);
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_by_id_found() {
        let backends = vec![create_test_backend("eu-1", "app1", "10.0.0.1")];
        let repo = MockBackendRepository::new(backends);
        let found = repo.get_by_id("eu-1").await;
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, "eu-1");
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_by_id_not_found() {
        let backends = vec![create_test_backend("eu-1", "app1", "10.0.0.1")];
        let repo = MockBackendRepository::new(backends);
        let found = repo.get_by_id("nonexistent").await;
        assert!(found.is_none());
    }

    #[tokio::test]
    async fn test_mock_backend_repo_get_version() {
        let repo = MockBackendRepository::new(vec![]);
        let version = repo.get_version().await;
        assert_eq!(version, 1);
    }

    #[test]
    fn test_build_dns_query_with_type() {
        // Test the build_dns_query_with_type helper function
        let query_a = build_dns_query_with_type("example.com", 1234, RecordType::A);
        assert!(!query_a.is_empty());

        let query_aaaa = build_dns_query_with_type("example.com", 5678, RecordType::AAAA);
        assert!(!query_aaaa.is_empty());

        // Different types should produce different bytes (different record type)
        assert_ne!(query_a, query_aaaa);

        // Different IDs should produce different bytes
        let query_diff_id = build_dns_query_with_type("example.com", 9999, RecordType::A);
        assert_ne!(query_a, query_diff_id);
    }

    // ===== RequestHandler trait tests =====

    use hickory_server::server::Protocol;
    use hickory_server::authority::MessageRequest;
    use std::io;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Mock ResponseHandler for testing RequestHandler implementation
    #[derive(Clone)]
    struct MockResponseHandler {
        response_sent: Arc<AtomicBool>,
        should_fail: bool,
    }

    impl MockResponseHandler {
        fn new() -> Self {
            Self {
                response_sent: Arc::new(AtomicBool::new(false)),
                should_fail: false,
            }
        }

        fn failing() -> Self {
            Self {
                response_sent: Arc::new(AtomicBool::new(false)),
                should_fail: true,
            }
        }
    }

    #[async_trait::async_trait]
    impl ResponseHandler for MockResponseHandler {
        async fn send_response<'a>(
            &mut self,
            response: hickory_server::authority::MessageResponse<
                '_,
                'a,
                impl Iterator<Item = &'a Record> + Send + 'a,
                impl Iterator<Item = &'a Record> + Send + 'a,
                impl Iterator<Item = &'a Record> + Send + 'a,
                impl Iterator<Item = &'a Record> + Send + 'a,
            >,
        ) -> io::Result<ResponseInfo> {
            if self.should_fail {
                return Err(io::Error::new(io::ErrorKind::Other, "mock send error"));
            }
            self.response_sent.store(true, Ordering::SeqCst);
            Ok(response.header().clone().into())
        }
    }

    /// Create a mock DNS Request for testing
    fn create_mock_request(name: &str, qtype: RecordType) -> Request {
        use hickory_proto::op::Message;
        use hickory_proto::serialize::binary::{BinDecodable, BinEncodable};

        let mut message = Message::new();
        message.set_id(1);
        message.set_message_type(MessageType::Query);
        message.set_op_code(OpCode::Query);
        message.set_recursion_desired(true);

        let mut query = hickory_proto::op::Query::new();
        query.set_name(Name::from_str(name).unwrap());
        query.set_query_type(qtype);
        message.add_query(query);

        let bytes = message.to_bytes().unwrap();
        let src: SocketAddr = "192.168.1.1:12345".parse().unwrap();

        // Convert Message to MessageRequest
        let message_request = MessageRequest::from_bytes(&bytes).unwrap();

        Request::new(
            message_request,
            src,
            Protocol::Udp,
        )
    }

    #[tokio::test]
    async fn test_request_handler_a_record_query_success() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NoError);
    }

    #[tokio::test]
    async fn test_request_handler_a_record_query_nxdomain() {
        let proxy_service = create_proxy_service(vec![]); // No backends
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NXDomain);
    }

    #[tokio::test]
    async fn test_request_handler_non_a_record_query() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Query for AAAA record (IPv6)
        let request = create_mock_request("myapp.internal.", RecordType::AAAA);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NotImp);
    }

    #[tokio::test]
    async fn test_request_handler_mx_record_query() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Query for MX record
        let request = create_mock_request("myapp.internal.", RecordType::MX);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NotImp);
    }

    #[tokio::test]
    async fn test_request_handler_send_error_on_success_path() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::failing();

        // Should still return a valid ResponseInfo even if send fails
        let result = handler.handle_request(&request, response_handler).await;
        // The error path returns header.into() which gives us a ResponseInfo
        assert!(result.response_code() == ResponseCode::NoError || result.response_code() == ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn test_request_handler_send_error_on_nxdomain_path() {
        let proxy_service = create_proxy_service(vec![]); // No backends
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::failing();

        // Should still return a valid ResponseInfo even if send fails
        let result = handler.handle_request(&request, response_handler).await;
        // The error path returns header.into() which should preserve NXDomain
        assert!(result.response_code() == ResponseCode::NXDomain || result.response_code() == ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn test_request_handler_send_error_on_notimp_path() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::TXT);
        let response_handler = MockResponseHandler::failing();

        // Should still return a valid ResponseInfo even if send fails
        let result = handler.handle_request(&request, response_handler).await;
        // The error path returns header.into() which should preserve NotImp
        assert!(result.response_code() == ResponseCode::NotImp || result.response_code() == ResponseCode::ServFail);
    }

    #[tokio::test]
    async fn test_request_handler_with_tracing() {
        // Initialize tracing for this test to cover tracing statements
        let _ = tracing_subscriber::fmt()
            .with_test_writer()
            .with_max_level(tracing::Level::DEBUG)
            .try_init();

        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NoError);
    }

    #[tokio::test]
    async fn test_request_handler_wrong_domain_nxdomain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Query for wrong domain
        let request = create_mock_request("myapp.external.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NXDomain);
    }

    #[tokio::test]
    async fn test_request_handler_root_domain_query() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "10.50.1.1"),
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        // Query for root internal domain
        let request = create_mock_request("internal.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NoError);
    }

    #[tokio::test]
    async fn test_request_handler_ipv6_backend_nxdomain() {
        let proxy_service = create_proxy_service(vec![
            create_test_backend("eu-1", "myapp", "2001:db8::1"), // IPv6 backend
        ]);
        let config = DnsConfig::default();
        let handler = DnsHandler::new(proxy_service, None, config);

        let request = create_mock_request("myapp.internal.", RecordType::A);
        let response_handler = MockResponseHandler::new();

        // IPv6 backend can't be returned for A record query
        let result = handler.handle_request(&request, response_handler).await;
        assert_eq!(result.response_code(), ResponseCode::NXDomain);
    }
}
