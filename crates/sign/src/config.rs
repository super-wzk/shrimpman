use std::net::SocketAddr;

use serde::Deserialize;

const DEFAULT_PORT: u16 = 53_312;
const DEFAULT_SHUTDOWN_TIMEOUT_SECS: u64 = 5;

/// Configuration for the Sign service.
#[derive(Debug, Clone, Copy, Default, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct SignConfig {
    /// TCP server configuration.
    pub server: SignServerConfig,
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
            .set_override("sign.server.listen_addr", "127.0.0.1:60000")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            SignConfig::try_from(&config).unwrap(),
            SignConfig {
                server: SignServerConfig {
                    listen_addr: "127.0.0.1:60000".parse().unwrap(),
                    shutdown_timeout_secs: 5,
                },
            }
        );
    }
}
