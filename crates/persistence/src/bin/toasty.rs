use shrimpman_persistence::MigrationConfig;
use toasty_cli::{Config as ToastyConfig, ToastyCli};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let toasty = ToastyConfig::load()?;
    let migration = MigrationConfig::load()?;

    let mut builder = toasty::Db::builder();
    builder.models(shrimpman_persistence::models());
    let db = builder.connect(&migration.database_url).await?;

    ToastyCli::with_config(db, toasty).parse_and_run().await?;

    Ok(())
}
