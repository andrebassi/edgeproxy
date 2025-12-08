//! Value Objects - Immutable domain primitives
//!
//! Value objects are identified by their value rather than identity.
//! They are immutable and can be freely shared.

use serde::{Deserialize, Serialize};

/// Geographic region code for routing decisions.
///
/// Regions are used to group backends and clients for geo-aware routing.
/// Clients are preferentially routed to backends in the same region.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RegionCode {
    /// South America (sa) - Brazil, Argentina, Chile, etc.
    SouthAmerica,
    /// North America (us) - USA, Canada, Mexico
    NorthAmerica,
    /// Europe (eu) - Western and Central Europe
    Europe,
    /// Asia Pacific (ap) - Japan, Korea, Southeast Asia, Australia
    AsiaPacific,
}

impl RegionCode {
    /// Parse a region code from a string.
    ///
    /// # Examples
    /// ```
    /// use edgeproxy::domain::RegionCode;
    ///
    /// assert_eq!(RegionCode::from_str("sa"), RegionCode::SouthAmerica);
    /// assert_eq!(RegionCode::from_str("unknown"), RegionCode::NorthAmerica); // fallback
    /// ```
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "sa" => Self::SouthAmerica,
            "us" => Self::NorthAmerica,
            "eu" => Self::Europe,
            "ap" => Self::AsiaPacific,
            _ => Self::NorthAmerica, // fallback
        }
    }

    /// Convert to string representation.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SouthAmerica => "sa",
            Self::NorthAmerica => "us",
            Self::Europe => "eu",
            Self::AsiaPacific => "ap",
        }
    }

    /// Map a country code (ISO 3166-1 alpha-2) to a region.
    pub fn from_country(country: &str) -> Self {
        match country.to_uppercase().as_str() {
            // South America
            "BR" | "AR" | "CL" | "PE" | "CO" | "UY" | "PY" | "BO" | "EC" | "VE" => {
                Self::SouthAmerica
            }
            // North America
            "US" | "CA" | "MX" => Self::NorthAmerica,
            // Europe
            "PT" | "ES" | "FR" | "DE" | "NL" | "IT" | "GB" | "IE" | "BE" | "CH" | "AT" | "PL"
            | "CZ" | "SE" | "NO" | "DK" | "FI" | "RU" | "UA" => Self::Europe,
            // Asia Pacific
            "JP" | "KR" | "TW" | "HK" | "SG" | "MY" | "TH" | "VN" | "ID" | "PH" | "AU" | "NZ"
            | "CN" | "IN" => Self::AsiaPacific,
            // Fallback to US for unknown countries
            _ => Self::NorthAmerica,
        }
    }
}

impl Default for RegionCode {
    fn default() -> Self {
        Self::NorthAmerica
    }
}

impl std::fmt::Display for RegionCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Score calculated for a backend during load balancing.
///
/// Lower scores are better. The score combines:
/// - Geographic proximity (0-3 scale * 100)
/// - Current load factor
/// - Backend weight
#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct BackendScore {
    /// Backend ID this score applies to
    pub backend_id: String,
    /// Calculated score (lower is better)
    pub score: f64,
}

impl BackendScore {
    #[allow(dead_code)]
    pub fn new(backend_id: String, score: f64) -> Self {
        Self { backend_id, score }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== RegionCode::from_str Tests =====

    #[test]
    fn test_region_from_str_lowercase() {
        let tests = vec![
            ("sa", RegionCode::SouthAmerica),
            ("us", RegionCode::NorthAmerica),
            ("eu", RegionCode::Europe),
            ("ap", RegionCode::AsiaPacific),
        ];

        for (input, expected) in tests {
            assert_eq!(
                RegionCode::from_str(input),
                expected,
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_region_from_str_uppercase() {
        let tests = vec![
            ("SA", RegionCode::SouthAmerica),
            ("US", RegionCode::NorthAmerica),
            ("EU", RegionCode::Europe),
            ("AP", RegionCode::AsiaPacific),
        ];

        for (input, expected) in tests {
            assert_eq!(
                RegionCode::from_str(input),
                expected,
                "Failed for input: {}",
                input
            );
        }
    }

    #[test]
    fn test_region_from_str_mixed_case() {
        assert_eq!(RegionCode::from_str("Sa"), RegionCode::SouthAmerica);
        assert_eq!(RegionCode::from_str("Us"), RegionCode::NorthAmerica);
        assert_eq!(RegionCode::from_str("Eu"), RegionCode::Europe);
        assert_eq!(RegionCode::from_str("Ap"), RegionCode::AsiaPacific);
    }

    #[test]
    fn test_region_from_str_fallback() {
        let invalid_inputs = vec!["invalid", "xx", "", "north", "south", "asia"];

        for input in invalid_inputs {
            assert_eq!(
                RegionCode::from_str(input),
                RegionCode::NorthAmerica,
                "Fallback failed for input: {}",
                input
            );
        }
    }

    // ===== RegionCode::from_country Tests =====

    #[test]
    fn test_region_from_country_south_america() {
        let countries = vec!["BR", "AR", "CL", "PE", "CO", "UY", "PY", "BO", "EC", "VE"];

        for country in countries {
            assert_eq!(
                RegionCode::from_country(country),
                RegionCode::SouthAmerica,
                "Failed for country: {}",
                country
            );
        }
    }

    #[test]
    fn test_region_from_country_north_america() {
        let countries = vec!["US", "CA", "MX"];

        for country in countries {
            assert_eq!(
                RegionCode::from_country(country),
                RegionCode::NorthAmerica,
                "Failed for country: {}",
                country
            );
        }
    }

    #[test]
    fn test_region_from_country_europe() {
        let countries = vec![
            "PT", "ES", "FR", "DE", "NL", "IT", "GB", "IE", "BE", "CH", "AT", "PL", "CZ", "SE",
            "NO", "DK", "FI", "RU", "UA",
        ];

        for country in countries {
            assert_eq!(
                RegionCode::from_country(country),
                RegionCode::Europe,
                "Failed for country: {}",
                country
            );
        }
    }

    #[test]
    fn test_region_from_country_asia_pacific() {
        let countries = vec![
            "JP", "KR", "TW", "HK", "SG", "MY", "TH", "VN", "ID", "PH", "AU", "NZ", "CN", "IN",
        ];

        for country in countries {
            assert_eq!(
                RegionCode::from_country(country),
                RegionCode::AsiaPacific,
                "Failed for country: {}",
                country
            );
        }
    }

    #[test]
    fn test_region_from_country_lowercase() {
        // Should handle lowercase country codes
        assert_eq!(RegionCode::from_country("br"), RegionCode::SouthAmerica);
        assert_eq!(RegionCode::from_country("us"), RegionCode::NorthAmerica);
        assert_eq!(RegionCode::from_country("fr"), RegionCode::Europe);
        assert_eq!(RegionCode::from_country("jp"), RegionCode::AsiaPacific);
    }

    #[test]
    fn test_region_from_country_unknown_fallback() {
        let unknown_countries = vec!["XX", "ZZ", "??", ""];

        for country in unknown_countries {
            assert_eq!(
                RegionCode::from_country(country),
                RegionCode::NorthAmerica,
                "Fallback failed for country: {}",
                country
            );
        }
    }

    // ===== RegionCode::as_str Tests =====

    #[test]
    fn test_region_as_str() {
        assert_eq!(RegionCode::SouthAmerica.as_str(), "sa");
        assert_eq!(RegionCode::NorthAmerica.as_str(), "us");
        assert_eq!(RegionCode::Europe.as_str(), "eu");
        assert_eq!(RegionCode::AsiaPacific.as_str(), "ap");
    }

    // ===== RegionCode Display Tests =====

    #[test]
    fn test_region_display() {
        assert_eq!(format!("{}", RegionCode::SouthAmerica), "sa");
        assert_eq!(format!("{}", RegionCode::NorthAmerica), "us");
        assert_eq!(format!("{}", RegionCode::Europe), "eu");
        assert_eq!(format!("{}", RegionCode::AsiaPacific), "ap");
    }

    // ===== RegionCode Default Tests =====

    #[test]
    fn test_region_default() {
        assert_eq!(RegionCode::default(), RegionCode::NorthAmerica);
    }

    // ===== RegionCode Clone and Eq Tests =====

    #[test]
    fn test_region_clone() {
        let region = RegionCode::Europe;
        let cloned = region.clone();
        assert_eq!(region, cloned);
    }

    #[test]
    fn test_region_equality() {
        assert_eq!(RegionCode::SouthAmerica, RegionCode::SouthAmerica);
        assert_ne!(RegionCode::SouthAmerica, RegionCode::NorthAmerica);
    }

    // ===== BackendScore Tests =====

    #[test]
    fn test_backend_score_new() {
        let score = BackendScore::new("backend-1".to_string(), 150.5);

        assert_eq!(score.backend_id, "backend-1");
        assert!((score.score - 150.5).abs() < f64::EPSILON);
    }

    #[test]
    fn test_backend_score_clone() {
        let score = BackendScore::new("test".to_string(), 99.9);
        let cloned = score.clone();

        assert_eq!(score.backend_id, cloned.backend_id);
        assert!((score.score - cloned.score).abs() < f64::EPSILON);
    }

    // ===== Roundtrip Tests =====

    #[test]
    fn test_region_roundtrip() {
        let regions = vec![
            RegionCode::SouthAmerica,
            RegionCode::NorthAmerica,
            RegionCode::Europe,
            RegionCode::AsiaPacific,
        ];

        for region in regions {
            let str_repr = region.as_str();
            let parsed = RegionCode::from_str(str_repr);
            assert_eq!(region, parsed);
        }
    }
}
