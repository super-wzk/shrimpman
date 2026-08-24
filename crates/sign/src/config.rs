use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;
pub use shrimpman_discovery::client::DiscoveryClientConfig;

const DEFAULT_PORT: u16 = 53_312;
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const DEFAULT_SESSION_TTL: Duration = Duration::from_secs(5 * 60);
const DEFAULT_DATABASE_URL: &str = "sqlite://shrimpman.sqlite3";

/// Configuration for the Sign service.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignConfig {
    /// Whether a successful sign-in may create a missing account.
    pub auto_sign_up: bool,
    /// Persistent storage configuration.
    pub database: SignDatabaseConfig,
    /// Service registration and discovery client configuration.
    pub discovery: DiscoveryClientConfig,
    /// Sign session configuration.
    pub session: SignSessionConfig,
    /// TCP server configuration.
    pub server: SignServerConfig,
}

/// Database configuration for the Sign service.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignDatabaseConfig {
    /// Toasty connection URL.
    pub url: String,
}

impl Default for SignDatabaseConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_DATABASE_URL.to_owned(),
        }
    }
}

/// Configuration for credentials issued by the Sign service.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignSessionConfig {
    /// Lifetime of a Sign session.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub ttl: Duration,
}

impl Default for SignSessionConfig {
    fn default() -> Self {
        Self {
            ttl: DEFAULT_SESSION_TTL,
        }
    }
}

/// TCP listener configuration for the Sign server.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignServerConfig {
    /// Address on which the Sign TCP listener accepts connections.
    pub listen_addr: SocketAddr,
    /// Time to wait for active connections before canceling them.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub shutdown_timeout: Duration,
}

impl Default for SignServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

impl TryFrom<&config::Config> for SignConfig {
    type Error = config::ConfigError;

    fn try_from(config: &config::Config) -> Result<Self, Self::Error> {
        config.get("sign")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_sign_section() {
        let config = ::config::Config::builder()
            .set_override("sign.auto_sign_up", true)
            .unwrap()
            .set_override(
                "sign.discovery.endpoints",
                vec!["http://etcd.internal:2379"],
            )
            .unwrap()
            .set_override("sign.discovery.lease_ttl", "30s")
            .unwrap()
            .set_override("sign.discovery.reconnect_delay", "500ms")
            .unwrap()
            .set_override("sign.server.listen_addr", "127.0.0.1:60000")
            .unwrap()
            .set_override("sign.server.shutdown_timeout", "250ms")
            .unwrap()
            .set_override("sign.session.ttl", "10m")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            SignConfig::try_from(&config).unwrap(),
            SignConfig {
                auto_sign_up: true,
                database: SignDatabaseConfig::default(),
                discovery: DiscoveryClientConfig {
                    endpoints: vec!["http://etcd.internal:2379".to_owned()],
                    lease_ttl: Duration::from_secs(30),
                    reconnect_delay: Duration::from_millis(500),
                },
                session: SignSessionConfig {
                    ttl: Duration::from_secs(10 * 60),
                },
                server: SignServerConfig {
                    listen_addr: "127.0.0.1:60000".parse().unwrap(),
                    shutdown_timeout: Duration::from_millis(250),
                },
            }
        );
    }
}
