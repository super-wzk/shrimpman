use std::{net::SocketAddr, time::Duration};

use serde::Deserialize;

const DEFAULT_API_PORT: u16 = 7_279;
const DEFAULT_GOSSIP_PORT: u16 = 7_280;
const DEFAULT_GOSSIP_INTERVAL: Duration = Duration::from_secs(1);
const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration loaded by the standalone Discovery service.
#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct DiscoveryConfig {
    pub server: DiscoveryServerConfig,
}

impl TryFrom<&config::Config> for DiscoveryConfig {
    type Error = config::ConfigError;

    fn try_from(config: &config::Config) -> Result<Self, Self::Error> {
        config.get("discovery")
    }
}

/// Discovery API and Chitchat peer configuration.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq)]
#[serde(default)]
pub struct DiscoveryServerConfig {
    /// Address accepting registration and snapshot clients.
    pub api_listen_addr: SocketAddr,
    /// Logical identifier shared by all Discovery peers.
    pub cluster_id: String,
    /// Stable identifier of this Discovery peer.
    pub node_id: String,
    /// Local UDP address used to receive Chitchat gossip.
    pub listen_addr: SocketAddr,
    /// UDP address advertised to other Discovery peers.
    pub advertise_addr: SocketAddr,
    /// Initial peers used to join the Chitchat cluster.
    pub seed_nodes: Vec<String>,
    /// Interval between gossip rounds.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub gossip_interval: Duration,
    /// Time to wait for active API connections during shutdown.
    #[serde(with = "jiff::fmt::serde::unsigned_duration::required")]
    pub shutdown_timeout: Duration,
}

impl Default for DiscoveryServerConfig {
    fn default() -> Self {
        Self {
            api_listen_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_API_PORT)),
            cluster_id: "shrimpman".to_owned(),
            node_id: "discovery-1".to_owned(),
            listen_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_GOSSIP_PORT)),
            advertise_addr: SocketAddr::from(([127, 0, 0, 1], DEFAULT_GOSSIP_PORT)),
            seed_nodes: Vec::new(),
            gossip_interval: DEFAULT_GOSSIP_INTERVAL,
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_client_and_server_configuration_separate() {
        let config = config::Config::builder()
            .set_override("discovery.server.node_id", "discovery-2")
            .unwrap()
            .set_override("discovery.server.seed_nodes", vec!["discovery-1:7280"])
            .unwrap()
            .build()
            .unwrap();

        let discovery = DiscoveryConfig::try_from(&config).unwrap();

        assert_eq!(discovery.server.node_id, "discovery-2");
        assert_eq!(discovery.server.seed_nodes, ["discovery-1:7280"]);
    }
}
