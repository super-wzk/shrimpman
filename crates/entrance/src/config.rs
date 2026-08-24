use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;
pub use shrimpman_discovery::client::DiscoveryClientConfig;

const DEFAULT_PORT: u16 = 53_310;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_LOG_FILTER: &str = "warn,shrimpman_entrance=info,shrimpman_discovery=info";

/// Configuration for the Entrance service.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EntranceConfig {
    /// Service discovery client configuration.
    pub discovery: DiscoveryClientConfig,
    /// Structured logging configuration for the Entrance process.
    pub logging: EntranceLoggingConfig,
    /// TCP server configuration.
    pub server: EntranceServerConfig,
}

/// Filtering configuration for structured Entrance process logs.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EntranceLoggingConfig {
    /// Comma-separated [`tracing_subscriber::EnvFilter`] directives.
    pub filter: String,
}

impl Default for EntranceLoggingConfig {
    fn default() -> Self {
        Self {
            filter: DEFAULT_LOG_FILTER.to_owned(),
        }
    }
}

/// TCP listener configuration for the Entrance server.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct EntranceServerConfig {
    /// Address on which the Entrance TCP listener accepts connections.
    pub listen_addr: SocketAddr,
    /// Public address of the Entrance server.
    pub advertise_addr: String,
    /// Time to wait for active connections before canceling them.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub shutdown_timeout: Duration,
}

impl Default for EntranceServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
            advertise_addr: format!("127.0.0.1:{DEFAULT_PORT}"),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

impl TryFrom<&config::Config> for EntranceConfig {
    type Error = config::ConfigError;

    fn try_from(config: &config::Config) -> Result<Self, Self::Error> {
        config.get("entrance")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_entrance_section() {
        let config = ::config::Config::builder()
            .set_override(
                "entrance.discovery.endpoints",
                vec!["http://etcd.internal:2379"],
            )
            .unwrap()
            .set_override("entrance.discovery.lease_ttl", "30s")
            .unwrap()
            .set_override("entrance.discovery.reconnect_delay", "500ms")
            .unwrap()
            .set_override(
                "entrance.logging.filter",
                "warn,shrimpman_entrance=debug,shrimpman_discovery=info",
            )
            .unwrap()
            .set_override("entrance.server.listen_addr", "127.0.0.1:60000")
            .unwrap()
            .set_override("entrance.server.advertise_addr", "entrance.internal:60001")
            .unwrap()
            .set_override("entrance.server.shutdown_timeout", "250ms")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            EntranceConfig::try_from(&config).unwrap(),
            EntranceConfig {
                discovery: DiscoveryClientConfig {
                    endpoints: vec!["http://etcd.internal:2379".to_owned()],
                    lease_ttl: Duration::from_secs(30),
                    reconnect_delay: Duration::from_millis(500),
                },
                logging: EntranceLoggingConfig {
                    filter: "warn,shrimpman_entrance=debug,shrimpman_discovery=info".to_owned(),
                },
                server: EntranceServerConfig {
                    listen_addr: "127.0.0.1:60000".parse().unwrap(),
                    advertise_addr: "entrance.internal:60001".to_owned(),
                    shutdown_timeout: Duration::from_millis(250),
                },
            }
        );
    }
}
