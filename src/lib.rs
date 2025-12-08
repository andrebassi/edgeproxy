//! edgeProxy Library
//!
//! This module exposes the edgeProxy components for use in integration tests
//! and as a library.

#![cfg_attr(coverage_nightly, feature(coverage_attribute))]

pub mod adapters;
pub mod application;
pub mod config;
pub mod domain;
pub mod infrastructure;
pub mod replication;

// Re-export commonly used types
pub use application::ProxyService;
pub use config::load_config;
pub use domain::entities::{Backend, Binding, ClientKey, GeoInfo};
pub use domain::ports::{BackendRepository, BindingRepository, GeoResolver, MetricsStore};
pub use domain::services::LoadBalancer;
pub use domain::value_objects::RegionCode;
pub use replication::{ReplicationAgent, ReplicationConfig};
