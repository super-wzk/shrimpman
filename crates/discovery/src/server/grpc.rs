use std::{collections::BTreeSet, sync::Arc};

use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Request, Response, Status, Streaming};

use super::registry::{Registry, decode_published_instance};
use crate::{
    DiscoverySnapshot, ServiceInstanceId, ServiceName, api,
    api::{
        discovery_server::Discovery, registration_command::Command as GrpcCommand,
        registration_event::Event as GrpcEvent,
    },
    grpc::encode_service_instance,
};

#[derive(Clone)]
pub(super) struct GrpcService {
    registry: Arc<Registry>,
}

impl GrpcService {
    pub(super) fn new(registry: Arc<Registry>) -> Self {
        Self { registry }
    }
}

#[tonic::async_trait]
impl Discovery for GrpcService {
    type RegisterStream = ReceiverStream<Result<api::RegistrationEvent, Status>>;
    async fn register(
        &self,
        request: Request<Streaming<api::RegistrationCommand>>,
    ) -> Result<Response<Self::RegisterStream>, Status> {
        let connection_id = self.registry.next_connection_id();
        let (events, response) = mpsc::channel(32);
        tokio::spawn(serve_registration_stream(
            request.into_inner(),
            events,
            connection_id,
            Arc::clone(&self.registry),
        ));

        Ok(Response::new(ReceiverStream::new(response)))
    }

    type WatchStream = ReceiverStream<Result<api::ServiceSnapshot, Status>>;

    async fn watch(
        &self,
        request: Request<api::WatchRequest>,
    ) -> Result<Response<Self::WatchStream>, Status> {
        let services = request
            .into_inner()
            .services
            .into_iter()
            .map(ServiceName::new)
            .collect::<Result<BTreeSet<_>, _>>()
            .map_err(|error| Status::invalid_argument(error.to_string()))?;
        let snapshots = self.registry.snapshots();
        let (output, response) = mpsc::channel(8);
        tokio::spawn(stream_snapshots(snapshots, services, output));

        Ok(Response::new(ReceiverStream::new(response)))
    }
}

async fn serve_registration_stream(
    mut commands: Streaming<api::RegistrationCommand>,
    events: mpsc::Sender<Result<api::RegistrationEvent, Status>>,
    connection_id: u64,
    registry: Arc<Registry>,
) {
    let mut owned = BTreeSet::new();

    loop {
        tokio::select! {
            _ = events.closed() => break,
            command = commands.message() => match command {
                Ok(Some(command)) => {
                    let event = handle_registration_command(
                        command,
                        connection_id,
                        &registry,
                        &mut owned,
                    ).await;
                    if events.send(Ok(event)).await.is_err() {
                        break;
                    }
                }
                Ok(None) => break,
                Err(error) => {
                    let _ = events.send(Err(error)).await;
                    break;
                }
            }
        }
    }

    for id in owned {
        registry.withdraw(connection_id, id).await;
    }
}

async fn handle_registration_command(
    command: api::RegistrationCommand,
    connection_id: u64,
    registry: &Registry,
    owned: &mut BTreeSet<ServiceInstanceId>,
) -> api::RegistrationEvent {
    match command.command {
        Some(GrpcCommand::Publish(published)) => {
            let instance_id = published
                .instance
                .as_ref()
                .map(|instance| instance.id.clone())
                .unwrap_or_default();
            match decode_published_instance(&published) {
                Ok(instance) => {
                    let id = instance.id;
                    let revision = published.revision;
                    match registry.publish(connection_id, id, published).await {
                        Ok(()) => {
                            owned.insert(id);
                            registration_published(id, revision)
                        }
                        Err(error) => registration_rejected(instance_id, error.to_string()),
                    }
                }
                Err(error) => registration_rejected(instance_id, error),
            }
        }
        Some(GrpcCommand::Withdraw(withdrawn)) => {
            let instance_id = withdrawn.value;
            match ServiceInstanceId::from_bytes(&instance_id) {
                Ok(id) if registry.withdraw(connection_id, id).await => {
                    owned.remove(&id);
                    registration_withdrawn(id)
                }
                Ok(_) => registration_rejected(
                    instance_id,
                    "instance is not owned by this registration stream".to_owned(),
                ),
                Err(error) => registration_rejected(instance_id, error.to_string()),
            }
        }
        None => registration_rejected(Vec::new(), "registration command is empty".to_owned()),
    }
}

async fn stream_snapshots(
    mut snapshots: watch::Receiver<Arc<DiscoverySnapshot>>,
    services: BTreeSet<ServiceName>,
    output: mpsc::Sender<Result<api::ServiceSnapshot, Status>>,
) {
    loop {
        let current = snapshots.borrow().clone();
        let snapshot = match grpc_snapshot(&current, &services) {
            Ok(snapshot) => snapshot,
            Err(error) => {
                let _ = output.send(Err(Status::internal(error.to_string()))).await;
                return;
            }
        };
        if output.send(Ok(snapshot)).await.is_err() {
            return;
        }

        tokio::select! {
            _ = output.closed() => return,
            changed = snapshots.changed() => {
                if changed.is_err() {
                    return;
                }
            }
        }
    }
}

fn grpc_snapshot(
    snapshot: &DiscoverySnapshot,
    services: &BTreeSet<ServiceName>,
) -> Result<api::ServiceSnapshot, serde_json::Error> {
    let instances = snapshot
        .all_instances()
        .filter(|instance| services.is_empty() || services.contains(&instance.service))
        .map(encode_service_instance)
        .collect::<Result<_, _>>()?;

    Ok(api::ServiceSnapshot { instances })
}

fn registration_published(id: ServiceInstanceId, revision: u64) -> api::RegistrationEvent {
    api::RegistrationEvent {
        event: Some(GrpcEvent::Published(api::RegistrationPublished {
            instance_id: id.as_uuid().as_bytes().to_vec(),
            revision,
        })),
    }
}

fn registration_withdrawn(id: ServiceInstanceId) -> api::RegistrationEvent {
    api::RegistrationEvent {
        event: Some(GrpcEvent::Withdrawn(api::RegistrationWithdrawn {
            instance_id: id.as_uuid().as_bytes().to_vec(),
        })),
    }
}

fn registration_rejected(instance_id: Vec<u8>, reason: String) -> api::RegistrationEvent {
    api::RegistrationEvent {
        event: Some(GrpcEvent::Rejected(api::RegistrationRejected {
            instance_id,
            reason,
        })),
    }
}
