mod config;
mod grpc;
mod registry;

use std::{future::Future, io, sync::Arc, time::Duration};

use chitchat::{ChitchatHandle, spawn_chitchat, transport::UdpTransport};
pub use config::{DiscoveryConfig, DiscoveryServerConfig};
use thiserror::Error;
use tokio::{
    net::TcpListener,
    task::{JoinError, JoinHandle},
};
use tokio_stream::wrappers::TcpListenerStream;
use tokio_util::sync::CancellationToken;
use tonic::transport::Server as TonicServer;
use tracing::warn;

use self::{
    grpc::GrpcService,
    registry::{Registry, chitchat_config, project_snapshots},
};
use crate::{
    api::discovery_server::DiscoveryServer as GrpcDiscoveryServer, grpc::MAX_MESSAGE_SIZE,
};

/// Standalone Discovery server backed by a Chitchat peer.
pub struct DiscoveryServer {
    listener: TcpListener,
    chitchat: ChitchatHandle,
    registry: Arc<Registry>,
    shutdown_timeout: Duration,
}

impl DiscoveryServer {
    /// Binds the gRPC API and starts the Chitchat peer.
    pub async fn bind(config: DiscoveryServerConfig) -> Result<Self, DiscoveryServerError> {
        let listener = TcpListener::bind(config.api_listen_addr).await?;
        let shutdown_timeout = config.shutdown_timeout;
        let chitchat = spawn_chitchat(chitchat_config(&config)?, Vec::new(), &UdpTransport).await?;
        let registry = Arc::new(Registry::new(chitchat.chitchat()));

        Ok(Self {
            listener,
            chitchat,
            registry,
            shutdown_timeout,
        })
    }

    /// Runs until shutdown is requested or either serving task stops.
    pub async fn run(self, shutdown: impl Future<Output = ()>) -> Result<(), DiscoveryServerError> {
        let Self {
            listener,
            chitchat,
            registry,
            shutdown_timeout,
        } = self;
        let cancellation = CancellationToken::new();
        let projection = tokio::spawn(project_snapshots(
            Arc::clone(&registry),
            cancellation.child_token(),
        ));
        let grpc_shutdown = cancellation.child_token();
        let grpc = GrpcDiscoveryServer::new(GrpcService::new(registry))
            .max_decoding_message_size(MAX_MESSAGE_SIZE)
            .max_encoding_message_size(MAX_MESSAGE_SIZE);
        let mut grpc_task = tokio::spawn(async move {
            TonicServer::builder()
                .add_service(grpc)
                .serve_with_incoming_shutdown(
                    TcpListenerStream::new(listener),
                    grpc_shutdown.cancelled_owned(),
                )
                .await
        });
        let gossip_termination = chitchat.termination_watcher();
        tokio::pin!(shutdown);
        tokio::pin!(gossip_termination);

        let (mut result, gossip_stopped, grpc_stopped) = tokio::select! {
            () = &mut shutdown => (Ok(()), false, false),
            result = &mut gossip_termination => (
                result.map_err(DiscoveryServerError::Chitchat),
                true,
                false,
            ),
            result = &mut grpc_task => (grpc_result(result), false, true),
        };

        cancellation.cancel();

        if let Err(error) = projection.await
            && result.is_ok()
        {
            result = Err(error.into());
        }
        if !grpc_stopped
            && let Err(error) = drain_grpc(&mut grpc_task, shutdown_timeout).await
            && result.is_ok()
        {
            result = Err(error);
        }
        if !gossip_stopped
            && let Err(error) = chitchat.shutdown().await
            && result.is_ok()
        {
            result = Err(DiscoveryServerError::Chitchat(error));
        }

        result
    }
}

async fn drain_grpc(
    task: &mut JoinHandle<Result<(), tonic::transport::Error>>,
    shutdown_timeout: Duration,
) -> Result<(), DiscoveryServerError> {
    match tokio::time::timeout(shutdown_timeout, &mut *task).await {
        Ok(result) => grpc_result(result),
        Err(_) => {
            warn!(
                ?shutdown_timeout,
                "forced Discovery gRPC shutdown after timeout"
            );
            task.abort();
            let _ = task.await;
            Ok(())
        }
    }
}

fn grpc_result(
    result: Result<Result<(), tonic::transport::Error>, JoinError>,
) -> Result<(), DiscoveryServerError> {
    result?.map_err(DiscoveryServerError::Grpc)
}

/// Failure while starting or serving Discovery.
#[derive(Debug, Error)]
pub enum DiscoveryServerError {
    #[error("failed to bind the Discovery gRPC API: {0}")]
    Io(#[from] io::Error),
    #[error("Discovery gRPC server failed: {0}")]
    Grpc(tonic::transport::Error),
    #[error("Discovery gossip failed: {0}")]
    Chitchat(#[from] anyhow::Error),
    #[error("Discovery task failed: {0}")]
    Task(#[from] JoinError),
    #[error("the current time does not fit in a Chitchat generation ID")]
    GenerationOverflow,
}

#[cfg(test)]
mod tests {
    use std::{net::SocketAddr, time::Duration};

    use serde_json::json;
    use tokio::sync::oneshot;

    use super::*;
    use crate::{
        ServiceInstance, ServiceInstanceId, ServiceName, ServiceState,
        client::{DiscoveryClient, DiscoveryClientConfig},
    };

    #[tokio::test]
    async fn publishes_and_watches_an_instance() {
        let server = DiscoveryServer::bind(DiscoveryServerConfig {
            api_listen_addr: localhost(0),
            listen_addr: localhost(0),
            advertise_addr: localhost(0),
            shutdown_timeout: Duration::from_secs(1),
            ..DiscoveryServerConfig::default()
        })
        .await
        .unwrap();
        let api_addr = server.listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = oneshot::channel();
        let server_task = tokio::spawn(server.run(async {
            let _ = shutdown_rx.await;
        }));

        let client = DiscoveryClient::connect(DiscoveryClientConfig {
            endpoint: api_addr.to_string(),
            reconnect_delay: Duration::from_millis(10),
        })
        .unwrap();
        let id = ServiceInstanceId::new();
        client
            .publish(
                ServiceInstance::new(
                    id,
                    ServiceName::new("entrance").unwrap(),
                    ServiceState::Ready,
                    json!({ "advertise_addr": "127.0.0.1:53310" }),
                )
                .unwrap(),
            )
            .unwrap();

        let mut snapshots = client.subscribe();
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if snapshots.borrow().as_ref().is_some_and(|snapshot| {
                    snapshot
                        .instances(&ServiceName::new("entrance").unwrap())
                        .iter()
                        .any(|instance| instance.id == id)
                }) {
                    break;
                }
                snapshots.changed().await.unwrap();
            }
        })
        .await
        .unwrap();

        drop(snapshots);
        drop(client);
        shutdown_tx.send(()).unwrap();
        server_task.await.unwrap().unwrap();
    }

    fn localhost(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }
}
