use std::error::Error;

use futures_util::FutureExt;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
};
use shrimpman_entrance::{EntranceConfig, EntranceServer, EntranceService, EntranceServiceContext};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_runtime::{init_tracing, load_config, shutdown_signal};
use tracing::{error, info, warn};

const SERVICE_NAME: ServiceName = ServiceName::from_static("entrance");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let entrance = EntranceConfig::try_from(&load_config("entrance")?)?;
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
