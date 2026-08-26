use std::error::Error;

use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_world::{WorldConfig, WorldServer};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: ServiceName = ServiceName::from_static("world");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let world = load_config()?;
    init_tracing(&world.logging.filter)?;
    info!(world = ?world.key, "Starting World service");

    let metadata = world.metadata();
    let lease_kv = LeaseKvClient::connect(world.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let server = WorldServer::bind(&world.lands).await?;

    let instance_id = ServiceInstanceId::new();
    discovery.publish(ServiceInstance::new(
        instance_id,
        SERVICE_NAME,
        ServiceState::Ready,
        None,
        metadata,
    )?)?;
    info!(lands = world.lands.len(), "World service is ready");

    server.run(shutdown_signal()).await?;
    discovery.withdraw(instance_id)?;
    info!("World service stopped");

    Ok(())
}

fn init_tracing(config_filter: &str) -> Result<(), Box<dyn Error + Send + Sync>> {
    let filter = match std::env::var(EnvFilter::DEFAULT_ENV) {
        Ok(filter) => filter,
        Err(std::env::VarError::NotPresent) => config_filter.to_owned(),
        Err(error) => return Err(error.into()),
    };
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_new(filter)?)
        .try_init()?;
    Ok(())
}

fn load_config() -> Result<WorldConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(
            config::Environment::with_prefix("SHRIMPMAN")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("world.lease_kv.endpoints"),
        )
        .build()?;

    WorldConfig::try_from(&config)
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        let mut terminate = match signal(SignalKind::terminate()) {
            Ok(terminate) => terminate,
            Err(error) => {
                error!(%error, "Failed to listen for SIGTERM");
                return;
            }
        };

        tokio::select! {
            result = tokio::signal::ctrl_c() => {
                match result {
                    Ok(()) => info!("Received Ctrl-C; shutting down"),
                    Err(error) => error!(%error, "Failed to listen for Ctrl-C"),
                }
            }
            _ = terminate.recv() => info!("Received SIGTERM; shutting down"),
        }
    }

    #[cfg(not(unix))]
    match tokio::signal::ctrl_c().await {
        Ok(()) => info!("Received Ctrl-C; shutting down"),
        Err(error) => error!(%error, "Failed to listen for Ctrl-C"),
    }
}
