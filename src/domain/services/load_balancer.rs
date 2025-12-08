//! Load Balancer Service
//!
//! Pure domain logic for selecting the optimal backend for a client.
//! This service has NO external dependencies - it's pure Rust.

use crate::domain::entities::{Backend, GeoInfo};
use crate::domain::value_objects::RegionCode;

/// Load balancer service for selecting optimal backends.
///
/// The load balancer uses a scoring algorithm that considers:
/// 1. Geographic proximity (country > region > local > fallback)
/// 2. Current load (connections / soft_limit)
/// 3. Backend weight (higher weight = preferred)
///
/// Lower scores are better.
pub struct LoadBalancer;

impl LoadBalancer {
    /// Select the best backend for a client.
    ///
    /// # Arguments
    /// * `backends` - List of available backends
    /// * `local_region` - Region of the local POP (Point of Presence)
    /// * `client_geo` - Geographic info for the client (if available)
    /// * `get_conn_count` - Closure to get current connection count for a backend
    ///
    /// # Returns
    /// The best backend, or None if no backends are available
    ///
    /// # Example
    /// ```ignore
    /// let backend = LoadBalancer::pick_backend(
    ///     &backends,
    ///     &RegionCode::SouthAmerica,
    ///     Some(&GeoInfo { country: "BR".to_string(), region: RegionCode::SouthAmerica }),
    ///     |id| metrics.get_connection_count(id),
    /// );
    /// ```
    pub fn pick_backend<F>(
        backends: &[Backend],
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
        get_conn_count: F,
    ) -> Option<Backend>
    where
        F: Fn(&str) -> usize,
    {
        let mut best: Option<(Backend, f64)> = None;

        for backend in backends.iter().filter(|b| b.healthy) {
            let current = get_conn_count(&backend.id) as f64;

            // Calculate limits
            let soft = if backend.soft_limit == 0 {
                1.0
            } else {
                backend.soft_limit as f64
            };

            let hard = if backend.hard_limit == 0 {
                f64::MAX
            } else {
                backend.hard_limit as f64
            };

            // Skip if at hard limit
            if current >= hard {
                continue;
            }

            // Calculate geo score (0-3 scale, lower is better)
            let geo_score = Self::calculate_geo_score(backend, local_region, client_geo);

            // Calculate load factor (0.0 = empty, 1.0 = at soft limit, >1.0 = overloaded)
            let load_factor = current / soft;

            // Weight factor (higher weight = lower score contribution)
            let weight = if backend.weight == 0 {
                1.0
            } else {
                backend.weight as f64
            };

            // Final score: geo_score * 100 + (load_factor / weight)
            // - Geo has much higher priority (100x multiplier)
            // - Load and weight fine-tune within the same geo tier
            let score = geo_score * 100.0 + (load_factor / weight);

            match &best {
                Some((_, best_score)) if score < *best_score => {
                    best = Some((backend.clone(), score));
                }
                None => {
                    best = Some((backend.clone(), score));
                }
                _ => {}
            }
        }

        best.map(|(backend, _)| backend)
    }

    /// Calculate geographic score for a backend.
    ///
    /// Score tiers:
    /// - 0.0: Same country as client (best)
    /// - 1.0: Same region as client
    /// - 2.0: Same region as local POP
    /// - 3.0: Fallback (different region)
    fn calculate_geo_score(
        backend: &Backend,
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
    ) -> f64 {
        match client_geo {
            // Best: backend is in the same country as the client
            Some(geo) if backend.country == geo.country => 0.0,
            // Good: backend is in the same region as the client
            Some(geo) if backend.region == geo.region => 1.0,
            // OK: backend is in the same region as the local POP
            _ if backend.region == *local_region => 2.0,
            // Fallback: different region entirely
            _ => 3.0,
        }
    }

    /// Calculate scores for all backends (useful for debugging/metrics).
    #[allow(dead_code)]
    pub fn calculate_all_scores<F>(
        backends: &[Backend],
        local_region: &RegionCode,
        client_geo: Option<&GeoInfo>,
        get_conn_count: F,
    ) -> Vec<(String, f64)>
    where
        F: Fn(&str) -> usize,
    {
        backends
            .iter()
            .filter(|b| b.healthy)
            .map(|backend| {
                let current = get_conn_count(&backend.id) as f64;
                let soft = if backend.soft_limit == 0 {
                    1.0
                } else {
                    backend.soft_limit as f64
                };
                let weight = if backend.weight == 0 {
                    1.0
                } else {
                    backend.weight as f64
                };

                let geo_score = Self::calculate_geo_score(backend, local_region, client_geo);
                let load_factor = current / soft;
                let score = geo_score * 100.0 + (load_factor / weight);

                (backend.id.clone(), score)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== Test Helpers =====

    fn create_backend(id: &str, region: &str, country: &str, healthy: bool) -> Backend {
        Backend {
            id: id.to_string(),
            app: "test".to_string(),
            region: RegionCode::from_str(region),
            country: country.to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy,
            weight: 1,
            soft_limit: 100,
            hard_limit: 200,
        }
    }

    fn create_backend_with_limits(
        id: &str,
        region: &str,
        country: &str,
        weight: u8,
        soft_limit: u32,
        hard_limit: u32,
    ) -> Backend {
        Backend {
            id: id.to_string(),
            app: "test".to_string(),
            region: RegionCode::from_str(region),
            country: country.to_string(),
            wg_ip: "10.0.0.1".to_string(),
            port: 8080,
            healthy: true,
            weight,
            soft_limit,
            hard_limit,
        }
    }

    // ===== Geo Score Tests =====

    #[test]
    fn test_pick_backend_same_country_priority() {
        // Backend in same country should win over same region
        let backends = vec![
            create_backend("br-1", "sa", "BR", true),
            create_backend("ar-1", "sa", "AR", true), // same region but different country
            create_backend("us-1", "us", "US", true),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-1");
    }

    #[test]
    fn test_pick_backend_same_region_when_no_country_match() {
        let backends = vec![
            create_backend("ar-1", "sa", "AR", true),
            create_backend("cl-1", "sa", "CL", true),
            create_backend("us-1", "us", "US", true),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        assert!(result.is_some());
        // Should pick one of the SA backends
        let id = result.unwrap().id;
        assert!(id == "ar-1" || id == "cl-1");
    }

    #[test]
    fn test_pick_backend_local_region_fallback() {
        // When no client geo match, use local POP region
        let backends = vec![
            create_backend("sa-1", "sa", "BR", true),
            create_backend("us-1", "us", "US", true),
        ];

        // No client geo, local region is us
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica, // local POP is US
            None,
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "us-1");
    }

    #[test]
    fn test_pick_backend_global_fallback() {
        // When nothing matches, pick any available
        let backends = vec![
            create_backend("jp-1", "ap", "JP", true),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "jp-1");
    }

    // ===== Hard Limit Tests =====

    #[test]
    fn test_pick_backend_respects_hard_limit() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", true),
            create_backend("us-1", "us", "US", true),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        // br-1 is at hard limit (200)
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |id| if id == "br-1" { 200 } else { 0 },
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "us-1");
    }

    #[test]
    fn test_pick_backend_all_at_hard_limit() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", true),
            create_backend("us-1", "us", "US", true),
        ];

        // All backends at hard limit
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 200,
        );

        assert!(result.is_none());
    }

    #[test]
    fn test_pick_backend_zero_hard_limit_means_unlimited() {
        let mut backend = create_backend("br-1", "sa", "BR", true);
        backend.hard_limit = 0; // should mean unlimited

        let backends = vec![backend];

        // Even with 1000 connections, should still be available
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 1000,
        );

        assert!(result.is_some());
    }

    // ===== Health Status Tests =====

    #[test]
    fn test_pick_backend_skips_unhealthy() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", false), // unhealthy
            create_backend("us-1", "us", "US", true),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "us-1");
    }

    #[test]
    fn test_pick_backend_all_unhealthy() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", false),
            create_backend("us-1", "us", "US", false),
        ];

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 0,
        );

        assert!(result.is_none());
    }

    // ===== Load Balancing Tests =====

    #[test]
    fn test_pick_backend_prefers_lower_load() {
        let backends = vec![
            create_backend_with_limits("br-1", "sa", "BR", 1, 100, 200),
            create_backend_with_limits("br-2", "sa", "BR", 1, 100, 200),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        // br-1 has 50 connections, br-2 has 10
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            Some(&client_geo),
            |id| if id == "br-1" { 50 } else { 10 },
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-2");
    }

    #[test]
    fn test_pick_backend_weight_affects_preference() {
        // Higher weight should receive more traffic
        let backends = vec![
            create_backend_with_limits("br-1", "sa", "BR", 1, 100, 200),
            create_backend_with_limits("br-2", "sa", "BR", 3, 100, 200), // 3x weight
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        // With same load, higher weight should win
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            Some(&client_geo),
            |_| 50,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-2");
    }

    #[test]
    fn test_pick_backend_zero_weight_treated_as_one() {
        let mut backend = create_backend("br-1", "sa", "BR", true);
        backend.weight = 0;

        let backends = vec![backend];

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 50,
        );

        assert!(result.is_some());
    }

    #[test]
    fn test_pick_backend_zero_soft_limit_treated_as_one() {
        let mut backend = create_backend("br-1", "sa", "BR", true);
        backend.soft_limit = 0;

        let backends = vec![backend];

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 0,
        );

        assert!(result.is_some());
    }

    // ===== Edge Cases =====

    #[test]
    fn test_pick_backend_no_backends() {
        let backends: Vec<Backend> = vec![];
        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        assert!(result.is_none());
    }

    #[test]
    fn test_pick_backend_single_backend() {
        let backends = vec![create_backend("only-1", "ap", "JP", true)];

        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "only-1");
    }

    #[test]
    fn test_pick_backend_no_client_geo() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", true),
            create_backend("us-1", "us", "US", true),
        ];

        // Without client geo, should prefer local region
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::SouthAmerica, // local POP is SA
            None,
            |_| 0,
        );

        assert!(result.is_some());
        assert_eq!(result.unwrap().id, "br-1");
    }

    // ===== Geo Priority Over Load Tests =====

    #[test]
    fn test_geo_priority_over_load() {
        // Even if local backend is loaded, should still prefer it over remote empty
        let backends = vec![
            create_backend_with_limits("br-1", "sa", "BR", 1, 100, 200),
            create_backend_with_limits("us-1", "us", "US", 1, 100, 200),
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        // br-1 at 90% load, us-1 empty
        let result = LoadBalancer::pick_backend(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |id| if id == "br-1" { 90 } else { 0 },
        );

        assert!(result.is_some());
        // Should still pick br-1 because geo score (0) beats us-1 geo score (3)
        assert_eq!(result.unwrap().id, "br-1");
    }

    // ===== calculate_all_scores Tests =====

    #[test]
    fn test_calculate_all_scores() {
        let backends = vec![
            create_backend("br-1", "sa", "BR", true),
            create_backend("us-1", "us", "US", true),
            create_backend("jp-1", "ap", "JP", false), // unhealthy - should be excluded
        ];

        let client_geo = GeoInfo::new("BR".to_string(), RegionCode::SouthAmerica);

        let scores = LoadBalancer::calculate_all_scores(
            &backends,
            &RegionCode::NorthAmerica,
            Some(&client_geo),
            |_| 0,
        );

        // Should only have 2 backends (unhealthy excluded)
        assert_eq!(scores.len(), 2);

        // br-1 should have lower score (same country)
        let br_score = scores.iter().find(|(id, _)| id == "br-1").unwrap().1;
        let us_score = scores.iter().find(|(id, _)| id == "us-1").unwrap().1;

        assert!(br_score < us_score);
    }

    #[test]
    fn test_calculate_all_scores_empty_backends() {
        let backends: Vec<Backend> = vec![];

        let scores = LoadBalancer::calculate_all_scores(
            &backends,
            &RegionCode::SouthAmerica,
            None,
            |_| 0,
        );

        assert!(scores.is_empty());
    }
}
