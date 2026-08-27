use std::net::{Ipv4Addr, SocketAddr};

use serde::Deserialize;
use shrimpman_domain::world::{
    ClientCompatibility, Land, LandKey, World, WorldContent, WorldKey, WorldSeason, WorldType,
};
pub use shrimpman_lease_kv::LeaseKvClientConfig;

const DEFAULT_LOG_FILTER: &str =
    "warn,shrimpman_world=info,shrimpman_discovery=info,shrimpman_lease_kv=info";
const DEFAULT_DATABASE_URL: &str = "sqlite://shrimpman.sqlite3";

/// Configuration for one World process.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorldConfig {
    /// Stable identity used to order this World in the Entrance list.
    pub key: WorldKey,
    /// IPv4 address advertised to game clients.
    pub address: Ipv4Addr,
    /// Client-visible World name.
    pub name: String,
    /// Client-visible World description.
    pub description: String,
    /// Client-visible World category.
    pub world_type: WorldType,
    /// Current World season.
    pub season: WorldSeason,
    /// Quest or minigame content offered by the World.
    pub content: WorldContent,
    /// Historical client-platform compatibility code.
    pub client_compatibility: ClientCompatibility,
    /// Persistent storage configuration.
    #[serde(default)]
    pub database: WorldDatabaseConfig,
    /// Land listeners owned by this process.
    pub lands: Vec<WorldLandConfig>,
    /// Process-wide leased key-value client configuration.
    #[serde(default)]
    pub lease_kv: LeaseKvClientConfig,
    /// Structured logging configuration for the World process.
    #[serde(default)]
    pub logging: WorldLoggingConfig,
}

/// Database configuration for the World service.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorldDatabaseConfig {
    /// Toasty connection URL.
    pub url: String,
}

impl Default for WorldDatabaseConfig {
    fn default() -> Self {
        Self {
            url: DEFAULT_DATABASE_URL.to_owned(),
        }
    }
}

impl WorldConfig {
    /// Builds the World metadata published through Discovery.
    pub fn metadata(&self) -> World {
        World {
            key: self.key.clone(),
            address: self.address,
            name: self.name.clone(),
            description: self.description.clone(),
            world_type: self.world_type,
            season: self.season,
            content: self.content,
            client_compatibility: self.client_compatibility,
            lands: self
                .lands
                .iter()
                .map(|land| Land {
                    key: land.key.clone(),
                    port: land.port,
                    max_players: land.max_players,
                    current_players: 0,
                })
                .collect(),
        }
    }
}

/// Configuration for one Land listener and its advertised capacity.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct WorldLandConfig {
    /// Stable identity used to order this Land in the Entrance list.
    pub key: LandKey,
    /// Address on which the Land listener accepts connections.
    pub listen_addr: SocketAddr,
    /// Port advertised to game clients.
    pub port: u16,
    /// Maximum player count advertised for this Land.
    pub max_players: u16,
}

/// Filtering configuration for structured World process logs.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct WorldLoggingConfig {
    /// Comma-separated [`tracing_subscriber::EnvFilter`] directives.
    pub filter: String,
}

impl Default for WorldLoggingConfig {
    fn default() -> Self {
        Self {
            filter: DEFAULT_LOG_FILTER.to_owned(),
        }
    }
}

impl TryFrom<&config::Config> for WorldConfig {
    type Error = config::ConfigError;

    fn try_from(config: &config::Config) -> Result<Self, Self::Error> {
        config.get("world")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_world_section_and_builds_discovery_metadata() {
        let config = ::config::Config::builder()
            .add_source(::config::File::from_str(
                r#"
                [world]
                key = "main"
                address = "192.0.2.10"
                name = "Main World"
                description = "Welcome"
                world_type = "Free"
                season = "Warm"
                content = "AllQuests"
                client_compatibility = "PC"

                [world.lease_kv]
                endpoints = ["http://etcd.internal:2379"]

                [world.logging]
                filter = "warn,shrimpman_world=debug"

                [[world.lands]]
                key = "land-1"
                listen_addr = "0.0.0.0:54001"
                port = 54001
                max_players = 100
                "#,
                ::config::FileFormat::Toml,
            ))
            .build()
            .unwrap();

        let config = WorldConfig::try_from(&config).unwrap();
        let metadata = config.metadata();

        assert_eq!(metadata.key, WorldKey::from("main".to_owned()));
        assert_eq!(metadata.address, "192.0.2.10".parse::<Ipv4Addr>().unwrap());
        assert_eq!(metadata.name, "Main World");
        assert_eq!(config.database, WorldDatabaseConfig::default());
        assert_eq!(metadata.client_compatibility, ClientCompatibility::PC);
        assert_eq!(metadata.lands.len(), 1);
        assert_eq!(metadata.lands[0].key, LandKey::from("land-1".to_owned()));
        assert_eq!(metadata.lands[0].port, 54_001);
        assert_eq!(metadata.lands[0].max_players, 100);
        assert_eq!(metadata.lands[0].current_players, 0);
    }
}
