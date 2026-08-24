mod config;

use std::{collections::BTreeMap, str, sync::Arc, time::Duration};

pub use config::DiscoveryClientConfig;
use etcd_client::{Client, GetOptions, PutOptions, WatchOptions};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::{
    DiscoverySnapshot, InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName,
    ServiceState,
};

const SERVICE_KEY_PREFIX: &str = "/shrimpman/services/";

/// Cloneable handle used by a business service to publish and discover instances.
#[derive(Clone)]
pub struct DiscoveryClient {
    commands: mpsc::UnboundedSender<ClientCommand>,
    snapshot: watch::Receiver<Option<Arc<DiscoverySnapshot>>>,
}

impl DiscoveryClient {
    /// Starts a reconnecting etcd client on the current Tokio runtime.
    pub fn connect(config: DiscoveryClientConfig) -> Result<Self, DiscoveryClientError> {
        let lease_ttl = lease_ttl_seconds(config.lease_ttl)?;
        if config.endpoints.is_empty()
            || config
                .endpoints
                .iter()
                .any(|endpoint| endpoint.trim().is_empty())
        {
            return Err(DiscoveryClientError::InvalidEndpoints);
        }
        if config.reconnect_delay.is_zero() {
            return Err(DiscoveryClientError::InvalidReconnectDelay);
        }

        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (snapshot_tx, snapshot_rx) = watch::channel(None);
        tokio::spawn(run(config, lease_ttl, command_rx, snapshot_tx));

        Ok(Self {
            commands: command_tx,
            snapshot: snapshot_rx,
        })
    }

    /// Publishes or replaces this process's current advertisement.
    pub fn publish(&self, instance: ServiceInstance) -> Result<(), DiscoveryClientError> {
        self.commands
            .send(ClientCommand::Publish(instance))
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

/// Failure to configure or send a command to a Discovery client.
#[derive(Debug, Error)]
pub enum DiscoveryClientError {
    #[error("at least one non-empty etcd endpoint is required")]
    InvalidEndpoints,
    #[error("the Discovery lease TTL must be a positive whole number of seconds")]
    InvalidLeaseTtl,
    #[error("the Discovery reconnect delay must be positive")]
    InvalidReconnectDelay,
    #[error("discovery client has stopped")]
    Stopped,
}

#[derive(Debug)]
enum ClientCommand {
    Publish(ServiceInstance),
    Withdraw(ServiceInstanceId),
}

async fn run(
    config: DiscoveryClientConfig,
    lease_ttl: i64,
    mut commands: mpsc::UnboundedReceiver<ClientCommand>,
    snapshot: watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) {
    let mut registrations = BTreeMap::<ServiceInstanceId, ServiceInstance>::new();
    info!(
        endpoint_count = config.endpoints.len(),
        lease_ttl_seconds = lease_ttl,
        reconnect_delay = ?config.reconnect_delay,
        "Starting discovery client"
    );

    loop {
        match Client::connect(&config.endpoints, None).await {
            Ok(mut client) => {
                match serve_connection(
                    &mut client,
                    lease_ttl,
                    &mut commands,
                    &mut registrations,
                    &snapshot,
                )
                .await
                {
                    Ok(()) => break,
                    Err(error) => {
                        warn!(
                            %error,
                            reconnect_delay = ?config.reconnect_delay,
                            "Lost the etcd connection; reconnecting"
                        );
                    }
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    reconnect_delay = ?config.reconnect_delay,
                    "Failed to connect to etcd; retrying"
                );
            }
        }

        snapshot.send_replace(None);
        if !wait_to_reconnect(config.reconnect_delay, &mut commands, &mut registrations).await {
            break;
        }
    }

    info!("Discovery client stopped");
}

async fn serve_connection(
    client: &mut Client,
    lease_ttl: i64,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    registrations: &mut BTreeMap<ServiceInstanceId, ServiceInstance>,
    snapshot: &watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) -> Result<(), ConnectionError> {
    let lease_id = client.lease_grant(lease_ttl, None).await?.id();
    let (mut lease_keeper, mut lease_responses) = client.lease_keep_alive(lease_id).await?;

    for instance in registrations.values() {
        put_instance(client, lease_id, instance).await?;
    }

    let revision = refresh_snapshot(client, snapshot).await?;
    let start_revision = revision
        .checked_add(1)
        .ok_or(ConnectionError::RevisionOverflow)?;
    let mut watches = client
        .watch(
            SERVICE_KEY_PREFIX,
            Some(
                WatchOptions::new()
                    .with_prefix()
                    .with_start_revision(start_revision),
            ),
        )
        .await?;
    let first_watch = watches
        .message()
        .await?
        .ok_or(ConnectionError::WatchClosed)?;
    validate_watch(&first_watch)?;
    if !first_watch.created() {
        return Err(ConnectionError::WatchNotCreated);
    }
    info!(
        lease_id,
        revision,
        published_instances = registrations.len(),
        "Connected to etcd and synchronized service discovery"
    );

    let keep_alive_interval = Duration::from_secs((lease_ttl / 3).max(1) as u64);
    let mut keep_alive = tokio::time::interval(keep_alive_interval);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = keep_alive.tick() => {
                lease_keeper.keep_alive().await?;
            }
            response = lease_responses.message() => {
                let response = response?.ok_or(ConnectionError::LeaseClosed)?;
                if response.ttl() <= 0 {
                    return Err(ConnectionError::LeaseExpired);
                }
            }
            response = watches.message() => {
                let response = response?.ok_or(ConnectionError::WatchClosed)?;
                validate_watch(&response)?;
                if !response.events().is_empty() {
                    refresh_snapshot(client, snapshot).await?;
                }
            }
            command = commands.recv() => match command {
                Some(ClientCommand::Publish(instance)) => {
                    if let Some(previous) = registrations.insert(instance.id, instance.clone())
                        && previous.service != instance.service
                    {
                        client.delete(instance_key(&previous), None).await?;
                    }
                    put_instance(client, lease_id, &instance).await?;
                    info!(
                        instance_id = ?instance.id,
                        service = instance.service.as_str(),
                        advertise_addr = instance.advertise_addr.as_deref(),
                        "Published service instance"
                    );
                }
                Some(ClientCommand::Withdraw(id)) => {
                    if let Some(instance) = registrations.remove(&id) {
                        client.delete(instance_key(&instance), None).await?;
                        info!(
                            instance_id = ?instance.id,
                            service = instance.service.as_str(),
                            "Withdrew service instance"
                        );
                    }
                }
                None => {
                    let _ = client.lease_revoke(lease_id).await;
                    return Ok(());
                }
            }
        }
    }
}

async fn refresh_snapshot(
    client: &mut Client,
    snapshot: &watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) -> Result<i64, ConnectionError> {
    let response = client
        .get(SERVICE_KEY_PREFIX, Some(GetOptions::new().with_prefix()))
        .await?;
    let revision = response.header().map_or(0, |header| header.revision());
    let instances: Vec<_> = response
        .kvs()
        .iter()
        .filter_map(|entry| match decode_instance(entry.key(), entry.value()) {
            Ok(instance) => Some(instance),
            Err(error) => {
                warn!(%error, "ignored invalid etcd service registration");
                None
            }
        })
        .collect();
    debug!(
        revision,
        instance_count = instances.len(),
        "Refreshed service discovery snapshot"
    );
    snapshot.send_replace(Some(Arc::new(DiscoverySnapshot::from_instances(instances))));

    Ok(revision)
}

async fn put_instance(
    client: &mut Client,
    lease_id: i64,
    instance: &ServiceInstance,
) -> Result<(), ConnectionError> {
    let value = encode_instance(instance)?;
    client
        .put(
            instance_key(instance),
            value,
            Some(PutOptions::new().with_lease(lease_id)),
        )
        .await?;
    Ok(())
}

async fn wait_to_reconnect(
    delay: Duration,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    registrations: &mut BTreeMap<ServiceInstanceId, ServiceInstance>,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            () = &mut sleep => return true,
            command = commands.recv() => match command {
                Some(ClientCommand::Publish(instance)) => {
                    registrations.insert(instance.id, instance);
                }
                Some(ClientCommand::Withdraw(id)) => {
                    registrations.remove(&id);
                }
                None => return false,
            }
        }
    }
}

fn lease_ttl_seconds(ttl: Duration) -> Result<i64, DiscoveryClientError> {
    if ttl.is_zero() || ttl.subsec_nanos() != 0 {
        return Err(DiscoveryClientError::InvalidLeaseTtl);
    }

    i64::try_from(ttl.as_secs()).map_err(|_| DiscoveryClientError::InvalidLeaseTtl)
}

fn instance_key(instance: &ServiceInstance) -> String {
    format!(
        "{SERVICE_KEY_PREFIX}{}/{}",
        instance.service.as_str(),
        instance.id.as_uuid()
    )
}

fn encode_instance(instance: &ServiceInstance) -> serde_json::Result<Vec<u8>> {
    serde_json::to_vec(&StoredServiceInstance {
        state: instance.state,
        advertise_addr: instance.advertise_addr.clone(),
        registered_at: instance.registered_at,
        metadata: &instance.metadata,
    })
}

fn decode_instance(key: &[u8], value: &[u8]) -> Result<ServiceInstance, StoredInstanceError> {
    let key = str::from_utf8(key)?;
    let path = key
        .strip_prefix(SERVICE_KEY_PREFIX)
        .ok_or(StoredInstanceError::Key)?;
    let (service, id) = path.split_once('/').ok_or(StoredInstanceError::Key)?;
    if id.contains('/') {
        return Err(StoredInstanceError::Key);
    }

    let stored = serde_json::from_slice::<StoredServiceInstance<Value>>(value)?;
    Ok(ServiceInstance {
        id: ServiceInstanceId::from_uuid(uuid::Uuid::parse_str(id)?),
        service: ServiceName::new(service)?,
        state: stored.state,
        advertise_addr: stored.advertise_addr,
        registered_at: stored.registered_at,
        metadata: stored.metadata,
    })
}

fn validate_watch(response: &etcd_client::WatchResponse) -> Result<(), ConnectionError> {
    if response.canceled() {
        return Err(ConnectionError::WatchCanceled(
            response.cancel_reason().to_owned(),
        ));
    }
    Ok(())
}

#[derive(Deserialize, Serialize)]
struct StoredServiceInstance<Metadata> {
    state: ServiceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    advertise_addr: Option<String>,
    registered_at: Timestamp,
    metadata: Metadata,
}

#[derive(Debug, Error)]
enum ConnectionError {
    #[error(transparent)]
    Etcd(#[from] etcd_client::Error),
    #[error("failed to encode a service registration: {0}")]
    Encode(#[from] serde_json::Error),
    #[error("etcd lease keep-alive stream closed")]
    LeaseClosed,
    #[error("etcd lease expired")]
    LeaseExpired,
    #[error("etcd watch stream closed")]
    WatchClosed,
    #[error("etcd did not acknowledge the service watch")]
    WatchNotCreated,
    #[error("etcd canceled the service watch: {0}")]
    WatchCanceled(String),
    #[error("etcd revision overflowed")]
    RevisionOverflow,
}

#[derive(Debug, Error)]
enum StoredInstanceError {
    #[error("invalid service registration key")]
    Key,
    #[error("service registration key is not UTF-8: {0}")]
    Utf8(#[from] str::Utf8Error),
    #[error(transparent)]
    ServiceName(#[from] InvalidServiceName),
    #[error("invalid service instance ID: {0}")]
    Id(#[from] uuid::Error),
    #[error("invalid service registration value: {0}")]
    Value(#[from] serde_json::Error),
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn validates_client_configuration() {
        assert!(
            DiscoveryClient::connect(DiscoveryClientConfig {
                endpoints: Vec::new(),
                ..DiscoveryClientConfig::default()
            })
            .is_err()
        );
        assert!(
            DiscoveryClient::connect(DiscoveryClientConfig {
                lease_ttl: Duration::from_millis(500),
                ..DiscoveryClientConfig::default()
            })
            .is_err()
        );
        assert!(
            DiscoveryClient::connect(DiscoveryClientConfig {
                reconnect_delay: Duration::ZERO,
                ..DiscoveryClientConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn round_trips_etcd_registration() {
        let instance = ServiceInstance {
            id: ServiceInstanceId::new(),
            service: ServiceName::new("entrance").unwrap(),
            state: ServiceState::Ready,
            advertise_addr: Some("entrance.internal:53310".to_owned()),
            registered_at: Timestamp::new(1_700_000_000, 0).unwrap(),
            metadata: json!({ "endpoint": "127.0.0.1:53310" }),
        };
        let key = instance_key(&instance);
        let value = encode_instance(&instance).unwrap();

        assert_eq!(decode_instance(key.as_bytes(), &value).unwrap(), instance);
    }
}
