use shrimpman_persistence::MigrationConfig;
use toasty_cli::{Config as ToastyConfig, ToastyCli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let toasty = ToastyConfig::load()?;
    let migration = load_config()?;

    let mut builder = toasty::Db::builder();
    builder.models(shrimpman_persistence::models());
    let db = builder.connect(&migration.database_url).await?;

    ToastyCli::with_config(db, toasty).parse_and_run().await?;

    Ok(())
}

fn load_config() -> Result<MigrationConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(config::Environment::with_prefix("SHRIMPMAN").separator("__"))
        .build()?;

    MigrationConfig::try_from(&config)
}
