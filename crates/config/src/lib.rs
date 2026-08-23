//! Shared deployment configuration for Shrimpman services.

#![warn(unreachable_pub)]

/// Loads the shared TOML file followed by environment overrides.
pub fn load() -> Result<config::Config, config::ConfigError> {
    config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(config::Environment::with_prefix("SHRIMPMAN").separator("__"))
        .build()
}
