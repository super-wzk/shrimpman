mod config;

use std::{collections::BTreeMap, sync::Arc};

pub use config::DiscoveryClientConfig;
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::ReceiverStream;
use tonic::transport::{Channel, Endpoint};
use tracing::warn;

use crate::{
    DiscoverySnapshot, ServiceInstance, ServiceInstanceId, ServiceName,
    api::{
        self, RegistrationCommand, discovery_client::DiscoveryClient as GrpcDiscoveryClient,
        registration_command::Command as GrpcCommand, registration_event::Event as GrpcEvent,
    },
    grpc::{MAX_MESSAGE_SIZE, decode_service_instance, encode_service_instance},
};

/// Cloneable handle used by a business service to publish and discover instances.
#[derive(Clone)]
pub struct DiscoveryClient {
    commands: mpsc::UnboundedSender<ClientCommand>,
    snapshot: watch::Receiver<Option<Arc<DiscoverySnapshot>>>,
}

impl DiscoveryClient {
    /// Starts a reconnecting Discovery API client on the current Tokio runtime.
    pub fn connect(config: DiscoveryClientConfig) -> Result<Self, DiscoveryClientError> {
        let endpoint = grpc_endpoint(&config.endpoint)?;
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(None);
        tokio::spawn(run(
            endpoint,
            config.reconnect_delay,
            command_rx,
            snapshot_tx,
        ));

        Ok(Self {
            commands: command_tx,
            snapshot: snapshot_rx,
        })
    }

    /// Publishes or replaces this process's current advertisement.
    pub fn publish(&self, instance: ServiceInstance) -> Result<(), DiscoveryClientError> {
        let id = instance.id;
        let instance = encode_service_instance(&instance)?;
        self.commands
            .send(ClientCommand::Publish { id, instance })
            .map_err(|_| DiscoveryClientError::Stopped)
    }

    /// Removes a previously published advertisement.
    pub fn withdraw(&self, id: ServiceInstanceId) -> Result<(), DiscoveryClientError> {
        self.commands
            .send(ClientCommand::Withdraw(id))
            .map_err(|_| DiscoveryClientError::Stopped)
    }

    /// Returns the latest synchronized snapshot, or `None` while disconnected.
    pub fn snapshot(&self) -> Option<Arc<DiscoverySnapshot>> {
        self.snapshot.borrow().clone()
    }

    /// Returns every currently known instance registered under `service`.
    pub fn instances(&self, service: &ServiceName) -> Arc<[ServiceInstance]> {
        self.snapshot()
            .map(|snapshot| snapshot.instances(service))
            .unwrap_or_else(|| Arc::from([]))
    }

    /// Subscribes to replacement snapshots from the background connection.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<DiscoverySnapshot>>> {
        self.snapshot.clone()
    }
}

/// Failure to create or send a command to a Discovery client.
#[derive(Debug, Error)]
pub enum DiscoveryClientError {
    #[error("invalid Discovery endpoint: {0}")]
    InvalidEndpoint(#[from] tonic::transport::Error),
    #[error("failed to encode service metadata: {0}")]
    Metadata(#[from] serde_json::Error),
    #[error("discovery client has stopped")]
    Stopped,
}

#[derive(Debug)]
enum ClientCommand {
    Publish {
        id: ServiceInstanceId,
        instance: api::ServiceInstance,
    },
    Withdraw(ServiceInstanceId),
}

async fn run(
    endpoint: Endpoint,
    reconnect_delay: std::time::Duration,
    mut commands: mpsc::UnboundedReceiver<ClientCommand>,
    snapshot: watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) {
    let mut registrations = BTreeMap::<ServiceInstanceId, api::PublishedServiceInstance>::new();

    loop {
        let channel = loop {
            tokio::select! {
                connection = endpoint.connect() => match connection {
                    Ok(channel) => break channel,
                    Err(error) => {
                        warn!(endpoint = %endpoint.uri(), %error, "failed to connect to Discovery");
                        if !wait_to_reconnect(
                            reconnect_delay,
                            &mut commands,
                            &mut registrations,
                        ).await {
                            return;
                        }
                    }
                },
                command = commands.recv() => match command {
                    Some(command) => {
                        apply_command(command, &mut registrations);
                    }
                    None => return,
                }
            }
        };

        if !run_connection(channel, &mut commands, &mut registrations, &snapshot).await {
            return;
        }
        snapshot.send_replace(None);

        if !wait_to_reconnect(reconnect_delay, &mut commands, &mut registrations).await {
            return;
        }
    }
}

async fn run_connection(
    channel: Channel,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    registrations: &mut BTreeMap<ServiceInstanceId, api::PublishedServiceInstance>,
    snapshot: &watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) -> bool {
    let mut registration_client = GrpcDiscoveryClient::new(channel.clone())
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);
    let mut snapshot_client = GrpcDiscoveryClient::new(channel)
        .max_decoding_message_size(MAX_MESSAGE_SIZE)
        .max_encoding_message_size(MAX_MESSAGE_SIZE);
    let (registration_tx, registration_rx) = mpsc::channel(32);
    let mut registration_events = match registration_client
        .register(ReceiverStream::new(registration_rx))
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            warn!(%error, "failed to open Discovery registration stream");
            return true;
        }
    };
    let mut snapshots = match snapshot_client
        .watch(api::WatchRequest {
            services: Vec::new(),
        })
        .await
    {
        Ok(response) => response.into_inner(),
        Err(error) => {
            warn!(%error, "failed to open Discovery snapshot stream");
            return true;
        }
    };

    for registration in registrations.values() {
        if registration_tx
            .send(publish_command(registration))
            .await
            .is_err()
        {
            return true;
        }
    }

    loop {
        tokio::select! {
            message = snapshots.message() => match message {
                Ok(Some(message)) => {
                    let instances = message.instances
                        .iter()
                        .map(decode_service_instance)
                        .collect::<Result<Vec<_>, _>>();
                    match instances {
                        Ok(instances) => {
                            snapshot.send_replace(Some(Arc::new(
                                DiscoverySnapshot::from_instances(instances),
                            )));
                        }
                        Err(error) => {
                            warn!(%error, "received an invalid Discovery snapshot");
                            return true;
                        }
                    }
                }
                Ok(None) | Err(_) => return true,
            },
            event = registration_events.message() => match event {
                Ok(Some(event)) => report_registration_event(event),
                Ok(None) | Err(_) => return true,
            },
            command = commands.recv() => match command {
                Some(command) => {
                    let message = apply_command(command, registrations);
                    if registration_tx.send(message).await.is_err() {
                        return true;
                    }
                }
                None => return false,
            },
        }
    }
}

fn apply_command(
    command: ClientCommand,
    registrations: &mut BTreeMap<ServiceInstanceId, api::PublishedServiceInstance>,
) -> RegistrationCommand {
    match command {
        ClientCommand::Publish { id, instance } => {
            let revision = registrations
                .get(&id)
                .map_or(1, |registration| registration.revision + 1);
            let registration = api::PublishedServiceInstance {
                revision,
                instance: Some(instance),
            };
            let command = publish_command(&registration);
            registrations.insert(id, registration);
            command
        }
        ClientCommand::Withdraw(id) => {
            registrations.remove(&id);
            RegistrationCommand {
                command: Some(GrpcCommand::Withdraw(api::ServiceInstanceId {
                    value: id.as_uuid().as_bytes().to_vec(),
                })),
            }
        }
    }
}

fn publish_command(registration: &api::PublishedServiceInstance) -> RegistrationCommand {
    RegistrationCommand {
        command: Some(GrpcCommand::Publish(registration.clone())),
    }
}

fn report_registration_event(event: api::RegistrationEvent) {
    if let Some(GrpcEvent::Rejected(rejected)) = event.event {
        warn!(reason = %rejected.reason, "Discovery rejected a registration command");
    }
}

async fn wait_to_reconnect(
    delay: std::time::Duration,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    registrations: &mut BTreeMap<ServiceInstanceId, api::PublishedServiceInstance>,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            () = &mut sleep => return true,
            command = commands.recv() => match command {
                Some(command) => {
                    apply_command(command, registrations);
                }
                None => return false,
            }
        }
    }
}

fn grpc_endpoint(endpoint: &str) -> Result<Endpoint, tonic::transport::Error> {
    let uri = if endpoint.contains("://") {
        endpoint.to_owned()
    } else {
        format!("http://{endpoint}")
    };
    Endpoint::from_shared(uri)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;
    use crate::ServiceState;

    #[test]
    fn keeps_only_the_latest_local_registration() {
        let id = ServiceInstanceId::new();
        let mut registrations = BTreeMap::new();

        apply_command(
            ClientCommand::Publish {
                id,
                instance: grpc_instance(id, ServiceState::Starting),
            },
            &mut registrations,
        );
        let message = apply_command(
            ClientCommand::Publish {
                id,
                instance: grpc_instance(id, ServiceState::Ready),
            },
            &mut registrations,
        );

        let Some(GrpcCommand::Publish(registration)) = message.command else {
            panic!("expected publication");
        };
        assert_eq!(registration.revision, 2);
        assert_eq!(
            registration.instance.unwrap().state,
            api::ServiceState::Ready as i32
        );
        assert_eq!(registrations.len(), 1);
    }

    #[test]
    fn accepts_endpoints_without_an_explicit_scheme() {
        let endpoint = grpc_endpoint("127.0.0.1:7279").unwrap();

        assert_eq!(endpoint.uri().scheme_str(), Some("http"));
    }

    fn grpc_instance(id: ServiceInstanceId, state: ServiceState) -> api::ServiceInstance {
        encode_service_instance(&ServiceInstance {
            id,
            service: ServiceName::new("entrance").unwrap(),
            state,
            metadata: json!({}),
        })
        .unwrap()
    }
}
