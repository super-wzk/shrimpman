use std::time::Duration;

use serde::Deserialize;

const DEFAULT_ENDPOINT: &str = "http://127.0.0.1:2379";
const DEFAULT_LEASE_TTL: Duration = Duration::from_secs(15);
const DEFAULT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Configuration for the process-wide key-value client.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct LeaseKvClientConfig {
    /// etcd endpoints used by the key-value client.
    pub endpoints: Vec<String>,
    /// Lifetime of process-owned values after this client stops refreshing its lease.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub lease_ttl: Duration,
    /// Delay before reconnecting after the etcd connection is lost.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub reconnect_delay: Duration,
}

impl Default for LeaseKvClientConfig {
    fn default() -> Self {
        Self {
            endpoints: vec![DEFAULT_ENDPOINT.to_owned()],
            lease_ttl: DEFAULT_LEASE_TTL,
            reconnect_delay: DEFAULT_RECONNECT_DELAY,
        }
    }
}
