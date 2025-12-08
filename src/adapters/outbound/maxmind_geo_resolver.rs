//! MaxMind GeoIP Resolver
//!
//! Implements GeoResolver using MaxMind GeoLite2 database.

use crate::domain::entities::GeoInfo;
use crate::domain::ports::GeoResolver;
use crate::domain::value_objects::RegionCode;
use maxminddb::Reader;
use serde::Deserialize;
use std::net::IpAddr;
use std::sync::Arc;

/// Embedded GeoLite2-Country database (compiled into binary).
const EMBEDDED_GEOIP: &[u8] = include_bytes!("../../../GeoLite2-Country.mmdb");

/// MaxMind GeoIP resolver.
///
/// Uses the MaxMind GeoLite2 database to resolve IP addresses
/// to country codes and geographic regions.
pub struct MaxMindGeoResolver {
    reader: Arc<Reader<Vec<u8>>>,
}

impl MaxMindGeoResolver {
    /// Load the embedded GeoIP database from the binary.
    pub fn embedded() -> anyhow::Result<Self> {
        let reader = Reader::from_source(EMBEDDED_GEOIP.to_vec())?;
        Ok(Self {
            reader: Arc::new(reader),
        })
    }

    /// Load a GeoIP database from a file path.
    pub fn from_file(path: &str) -> anyhow::Result<Self> {
        let reader = Reader::open_readfile(path)?;
        Ok(Self {
            reader: Arc::new(reader),
        })
    }
}

impl GeoResolver for MaxMindGeoResolver {
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo> {
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

        let region = RegionCode::from_country(&iso);

        Some(GeoInfo::new(iso, region))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_embedded_database_loads() {
        let resolver = MaxMindGeoResolver::embedded();
        assert!(resolver.is_ok());
    }

    #[test]
    fn test_resolve_known_ip() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Google's public DNS (US)
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        let result = resolver.resolve(ip);

        assert!(result.is_some());
        let geo = result.unwrap();
        assert_eq!(geo.country, "US");
        assert_eq!(geo.region, RegionCode::NorthAmerica);
    }

    #[test]
    fn test_resolve_private_ip_returns_none() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Private IP
        let ip = IpAddr::V4(Ipv4Addr::new(192, 168, 1, 1));
        let result = resolver.resolve(ip);

        // Private IPs typically return None
        assert!(result.is_none());
    }
}
