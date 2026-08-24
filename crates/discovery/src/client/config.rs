use std::time::Duration;

use serde::Deserialize;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:2379";
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(15);
const DEFAULT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Configuration for a business service connecting to etcd.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct DiscoveryClientConfig {
    /// etcd endpoints used for registration and discovery.
    pub endpoints: Vec<String>,
    /// Lifetime of registrations after this client stops refreshing its lease.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub lease_ttl: Duration,
    /// Delay before reconnecting after the etcd connection is lost.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub reconnect_delay: Duration,
}

impl Default for DiscoveryClientConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![DEFAULT_ENDPOINT.to_owned()],
            lease_ttl: DEFAULT_LEASE_TTL,
            reconnect_delay: DEFAULT_RECONNECT_DELAY,
        }
    }
}
