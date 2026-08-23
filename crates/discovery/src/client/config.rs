use std::time::Duration;

use serde::Deserialize;

const DEFAULT_ENDPOINT: &str = "127.0.0.1:7279";
const DEFAULT_RECONNECT_DELAY: Duration = Duration::from_secs(1);

/// Configuration for a business service connecting to Discovery.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct DiscoveryClientConfig {
    /// Discovery API address or resolvable host and port.
    pub endpoint: String,
    /// Delay before reconnecting after the API connection is lost.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub reconnect_delay: Duration,
}

impl Default for DiscoveryClientConfig {
    fn default() -> Self {
        Self {
            endpoint: DEFAULT_ENDPOINT.to_owned(),
            reconnect_delay: DEFAULT_RECONNECT_DELAY,
        }
    }
}
