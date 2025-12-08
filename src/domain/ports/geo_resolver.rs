//! GeoIP Resolver Port
//!
//! Defines the interface for resolving IP addresses to geographic locations.

use crate::domain::entities::GeoInfo;
use std::net::IpAddr;

/// Resolver for IP address to geographic location.
///
/// This is an outbound port that abstracts the GeoIP database.
/// Implementations may use MaxMind GeoLite2, IP2Location, or other databases.
pub trait GeoResolver: Send + Sync {
    /// Resolve an IP address to geographic information.
    ///
    /// Returns the country code and region for the given IP,
    /// or None if the IP cannot be resolved.
    fn resolve(&self, ip: IpAddr) -> Option<GeoInfo>;
}
