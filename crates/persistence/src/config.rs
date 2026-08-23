use serde::Deserialize;

/// Configuration used by database migration commands.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct MigrationConfig {
    /// Toasty connection URL.
    pub database_url: String,
}

impl MigrationConfig {
    /// Loads the migration section through the shared configuration loader.
    pub fn load() -> Result<Self, config::ConfigError> {
        Self::try_from(&shrimpman_config::load()?)
    }
}

impl TryFrom<&config::Config> for MigrationConfig {
    type Error = config::ConfigError;

    fn try_from(config: &config::Config) -> Result<Self, Self::Error> {
        config.get("migration")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_migration_section() {
        let config = config::Config::builder()
            .set_override("migration.database_url", "sqlite::memory:")
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            MigrationConfig::try_from(&config).unwrap(),
            MigrationConfig {
                database_url: "sqlite::memory:".to_owned(),
            }
        );
    }
}
