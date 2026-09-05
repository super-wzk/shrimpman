use std::error::Error;

use futures_util::FutureExt;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_world::{WorldConfig, WorldDatabase, WorldServer, WorldService};
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: ServiceName = ServiceName::from_static("world");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let world = load_config()?;
    init_tracing(&world.logging.filter)?;
    info!(
        world = ?world.key,
        shutdown_timeout = ?world.shutdown_timeout,
        "Starting World service"
    );

    let metadata = world.metadata();
    let database = WorldDatabase::connect(&world.database).await?;
    info!("Connected to World database");

    let lease_kv = LeaseKvClient::connect(world.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let service = WorldService::new(database.repositories())?;
    let shutdown_timeout = world.shutdown_timeout;
    let server = WorldServer::bind(&world.lands, service).await?;

    let instance_id = ServiceInstanceId::new();
    let instance = ServiceInstance::new(
        instance_id,
        SERVICE_NAME,
        ServiceState::Ready,
        None,
        metadata,
    )?;
    let draining_instance = ServiceInstance {
        state: ServiceState::Draining,
        ..instance.clone()
    };
    discovery.publish(instance)?;
    info!(lands = world.lands.len(), "World service is ready");

    let shutdown = {
        let discovery = discovery.clone();
        async move {
            shutdown_signal().await;

            match discovery.publish(draining_instance) {
                Ok(()) => info!("World service is draining"),
                Err(error) => error!(%error, "Failed to mark World service as draining"),
            }
        }
        .shared()
    };
    let shutdown_deadline = {
        let shutdown = shutdown.clone();
        async move {
            shutdown.await;
            tokio::time::sleep(shutdown_timeout).await;
        }
    };
    tokio::select! {
        result = server.run(shutdown) => result?,
        () = shutdown_deadline => {
            warn!(?shutdown_timeout, "World shutdown timed out; canceling active connections");
        }
    }
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
