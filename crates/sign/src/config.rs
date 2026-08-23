use std::net::SocketAddr;

use serde::Deserialize;

const DEFAULT_PORT: u16 = 53_312;
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 5;
const DEFAULT_SESSION_TTL_SECS: u32 = 5 * 60;
const DEFAULT_DATABASE_URL: &str = "sqlite://shrimpman.sqlite3";

/// Configuration for the Sign service.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignConfig {
    /// Whether a successful sign-in may create a missing account.
    pub auto_sign_up: bool,
    /// Persistent storage configuration.
    pub database: SignDatabaseConfig,
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
    /// Lifetime of a Sign session in seconds.
    pub ttl_secs: u32,
}

impl Default for SignSessionConfig {
    fn default() -> Self {
        Self {
            ttl_secs: DEFAULT_SESSION_TTL_SECS,
        }
    }
}

/// TCP listener configuration for the Sign server.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignServerConfig {
    /// Address on which the Sign TCP listener accepts connections.
    pub listen_addr: SocketAddr,
    /// Seconds to wait for active connections before canceling them.
    pub shutdown_timeout_secs: u64,
}

impl Default for SignServerConfig {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
            shutdown_timeout_secs: DEFAULT_SHUTDOWN_TIMEOUT_SECS,
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
            .set_override("sign.server.listen_addr", "127.0.0.1:60000")
            .unwrap()
            .set_override("sign.session.ttl_secs", 600)
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            SignConfig::try_from(&config).unwrap(),
            SignConfig {
                auto_sign_up: true,
                database: SignDatabaseConfig::default(),
                session: SignSessionConfig { ttl_secs: 600 },
                server: SignServerConfig {
                    listen_addr: "127.0.0.1:60000".parse().unwrap(),
                    shutdown_timeout_secs: 5,
                },
            }
        );
    }
}
