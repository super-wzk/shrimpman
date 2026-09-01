use std::error::Error;

use futures_util::FutureExt;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_entrance::{EntranceConfig, EntranceServer, EntranceService, EntranceServiceContext};
use shrimpman_lease_kv::LeaseKvClient;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;

const SERVICE_NAME: ServiceName = ServiceName::from_static("entrance");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let entrance = load_config()?;
    init_tracing(&entrance.logging.filter)?;
    info!("Starting Entrance service");

    info!(
        listen_addr = %entrance.server.listen_addr,
        advertise_addr = %entrance.server.advertise_addr,
        shutdown_timeout = ?entrance.shutdown_timeout,
        "Loaded Entrance configuration"
    );

    let lease_kv = LeaseKvClient::connect(entrance.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let context = EntranceServiceContext::new(discovery.clone());
    let service = EntranceService::new(context)?;
    let advertise_addr = entrance.server.advertise_addr.clone();
    let shutdown_timeout = entrance.shutdown_timeout;
    let server = EntranceServer::bind(entrance.server, service).await?;
    let listen_addr = server.local_addr()?;

    let instance_id = ServiceInstanceId::new();
    let instance = ServiceInstance::new(
        instance_id,
        SERVICE_NAME,
        ServiceState::Ready,
        Some(advertise_addr.clone()),
        (),
    )?;
    let draining_instance = ServiceInstance {
        state: ServiceState::Draining,
        ..instance.clone()
    };
    discovery.publish(instance)?;
    info!(%listen_addr, %advertise_addr, "Entrance service is ready");

    let shutdown = {
        let discovery = discovery.clone();
        async move {
            shutdown_signal().await;

            match discovery.publish(draining_instance) {
                Ok(()) => info!("Entrance service is draining"),
                Err(error) => error!(%error, "Failed to mark Entrance service as draining"),
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
            warn!(?shutdown_timeout, "Entrance shutdown timed out; canceling active connections");
        }
    }
    discovery.withdraw(instance_id)?;
    info!("Entrance service stopped");

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

fn load_config() -> Result<EntranceConfig, config::ConfigError> {
    let config = config::Config::builder()
        .add_source(config::File::new("config.toml", config::FileFormat::Toml))
        .add_source(
            config::Environment::with_prefix("SHRIMPMAN")
                .prefix_separator("_")
                .separator("__")
                .try_parsing(true)
                .list_separator(",")
                .with_list_parse_key("entrance.lease_kv.endpoints"),
        )
        .build()?;

    EntranceConfig::try_from(&config)
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
