mod api_server;
mod dns_server;
mod tcp_server;
mod tls_server;

pub use api_server::ApiServer;
pub use dns_server::DnsServer;
pub use tcp_server::TcpServer;
pub use tls_server::{TlsConfig, TlsServer};

// Re-export for external use (e.g., integration tests)
#[allow(unused_imports)]
pub use api_server::{ApiState, RegisterRequest};
#[allow(unused_imports)]
pub use dns_server::{DnsConfig, DnsHandler};
