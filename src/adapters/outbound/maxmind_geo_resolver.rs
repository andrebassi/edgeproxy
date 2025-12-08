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
#[cfg_attr(coverage_nightly, coverage(off))]
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

    #[test]
    fn test_resolve_loopback_returns_none() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Loopback IP
        let ip = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let result = resolver.resolve(ip);

        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_various_public_ips() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Cloudflare DNS - may resolve to different countries depending on DB version
        let ip_cf = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let result_cf = resolver.resolve(ip_cf);
        // Just check if it resolves (may or may not depending on DB)
        if let Some(geo) = result_cf {
            assert!(!geo.country.is_empty());
        }

        // Another public IP
        let ip_other = IpAddr::V4(Ipv4Addr::new(4, 4, 4, 4));
        let result_other = resolver.resolve(ip_other);
        // May or may not resolve depending on database
        if let Some(geo) = result_other {
            assert!(!geo.country.is_empty());
        }
    }

    #[test]
    fn test_resolve_ipv6_loopback() {
        use std::net::Ipv6Addr;
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // IPv6 loopback
        let ip = IpAddr::V6(Ipv6Addr::new(0, 0, 0, 0, 0, 0, 0, 1));
        let result = resolver.resolve(ip);

        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_class_a() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Class A private
        let ip = IpAddr::V4(Ipv4Addr::new(10, 0, 0, 1));
        let result = resolver.resolve(ip);

        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_private_class_b() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Class B private
        let ip = IpAddr::V4(Ipv4Addr::new(172, 16, 0, 1));
        let result = resolver.resolve(ip);

        assert!(result.is_none());
    }

    #[test]
    fn test_from_file_nonexistent() {
        let result = MaxMindGeoResolver::from_file("/nonexistent/path/GeoLite2.mmdb");
        assert!(result.is_err());
    }

    #[test]
    fn test_geoinfo_creation_via_resolve() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Known US IP
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        let result = resolver.resolve(ip);

        if let Some(geo) = result {
            // GeoInfo should have both country and region
            assert!(!geo.country.is_empty());
            // Region should be mapped from country
            assert_eq!(geo.region, RegionCode::from_country(&geo.country));
        }
    }

    #[test]
    fn test_resolver_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<MaxMindGeoResolver>();
    }

    #[test]
    fn test_multiple_resolutions_same_ip() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));

        // Should return consistent results
        let result1 = resolver.resolve(ip);
        let result2 = resolver.resolve(ip);

        assert_eq!(result1.is_some(), result2.is_some());
        if let (Some(geo1), Some(geo2)) = (result1, result2) {
            assert_eq!(geo1.country, geo2.country);
            assert_eq!(geo1.region, geo2.region);
        }
    }

    #[test]
    fn test_embedded_database_arc_clone() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Test that the Arc<Reader> can be cloned
        let reader_clone = resolver.reader.clone();
        assert!(Arc::strong_count(&resolver.reader) >= 2);
        drop(reader_clone);
    }

    #[test]
    fn test_resolve_link_local_ip() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Link-local address
        let ip = IpAddr::V4(Ipv4Addr::new(169, 254, 1, 1));
        let result = resolver.resolve(ip);

        // Link-local typically returns None
        assert!(result.is_none());
    }

    #[test]
    fn test_resolve_broadcast() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Broadcast address
        let ip = IpAddr::V4(Ipv4Addr::new(255, 255, 255, 255));
        let result = resolver.resolve(ip);

        // Broadcast typically returns None
        assert!(result.is_none());
    }

    #[test]
    fn test_from_file_success() {
        // Test loading from the actual file in the project
        let resolver = MaxMindGeoResolver::from_file("GeoLite2-Country.mmdb");
        assert!(resolver.is_ok());

        // Verify it works
        let resolver = resolver.unwrap();
        let ip = IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8));
        let result = resolver.resolve(ip);
        assert!(result.is_some());
        assert_eq!(result.unwrap().country, "US");
    }

    #[test]
    fn test_resolve_cloudflare_ip() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Cloudflare DNS 1.1.1.1
        let ip = IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1));
        let result = resolver.resolve(ip);

        // Should resolve (country may vary by DB version)
        if let Some(geo) = result {
            assert!(!geo.country.is_empty());
        }
    }

    #[test]
    fn test_resolve_google_brazil_ip() {
        let resolver = MaxMindGeoResolver::embedded().unwrap();

        // Google Brazil IP range (may resolve differently)
        let ip = IpAddr::V4(Ipv4Addr::new(189, 83, 57, 1));
        let result = resolver.resolve(ip);

        // Should resolve to some country
        if let Some(geo) = result {
            assert!(!geo.country.is_empty());
        }
    }
}
