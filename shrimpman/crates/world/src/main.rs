use std::error::Error;

use futures_util::FutureExt;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_runtime::{init_tracing, load_config, shutdown_signal};
use shrimpman_world::{WorldConfig, WorldDatabase, WorldServer, WorldService};
use tracing::{error, info, warn};

const SERVICE_NAME: ServiceName = ServiceName::from_static("world");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let world = WorldConfig::try_from(&load_config("world")?)?;
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
