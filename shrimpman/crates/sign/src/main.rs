use std::error::Error;

use futures_util::FutureExt;
use jiff::SignedDuration;
use shrimpman_discovery::{
    ServiceInstance, ServiceInstanceId, ServiceName, ServiceState, client::DiscoveryClient,
    selector::RoundRobinSelector,
};
use shrimpman_lease_kv::LeaseKvClient;
use shrimpman_runtime::{init_tracing, load_config, shutdown_signal};
use shrimpman_sign::{SignConfig, SignDatabase, SignServer, SignService, SignServiceContext, http};
use tracing::{error, info, warn};

const SERVICE_NAME: ServiceName = ServiceName::from_static("sign");

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let sign = SignConfig::try_from(&load_config("sign")?)?;
    init_tracing(&sign.logging.filter)?;
    info!("Starting Sign service");

    info!(
        auto_sign_up = sign.auto_sign_up,
        http_listen_addr = %sign.http.listen_addr,
        tcp_listen_addr = %sign.server.listen_addr,
        advertise_addr = %sign.server.advertise_addr,
        shutdown_timeout = ?sign.shutdown_timeout,
        "Loaded Sign configuration"
    );

    let database = SignDatabase::connect(&sign.database).await?;
    info!("Connected to Sign database");

    let lease_kv = LeaseKvClient::connect(sign.lease_kv)?;
    let discovery = DiscoveryClient::new(lease_kv);
    let context = SignServiceContext::new(
        sign.auto_sign_up,
        SignedDuration::try_from(sign.session.ttl)?,
        discovery.clone(),
        RoundRobinSelector::new(),
        database.repositories(),
    );
    let service = SignService::new(context)?;
    let advertise_addr = sign.server.advertise_addr.clone();
    let shutdown_timeout = sign.shutdown_timeout;
    let http_server = http::Server::bind(sign.http, service.clone()).await?;
    let server = SignServer::bind(sign.server, service).await?;
    let http_listen_addr = http_server.local_addr()?;
    let tcp_listen_addr = server.local_addr()?;

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
    info!(%tcp_listen_addr, %http_listen_addr, %advertise_addr, "Sign service is ready");

    let shutdown = {
        let discovery = discovery.clone();
        async move {
            shutdown_signal().await;

            match discovery.publish(draining_instance) {
                Ok(()) => info!("Sign service is draining"),
                Err(error) => error!(%error, "Failed to mark Sign service as draining"),
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
    let servers =
        async move { tokio::try_join!(server.run(shutdown.clone()), http_server.run(shutdown)) };
    tokio::select! {
        result = servers => {
            result?;
        }
        () = shutdown_deadline => {
            warn!(?shutdown_timeout, "Sign shutdown timed out; canceling active connections");
        }
    }
    discovery.withdraw(instance_id)?;
    info!("Sign service stopped");

    Ok(())
}
