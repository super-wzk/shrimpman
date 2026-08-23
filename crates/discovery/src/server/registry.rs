use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use chitchat::{
    Chitchat, ChitchatConfig, ChitchatId, FailureDetectorConfig, NodeState, ProtocolVersion,
};
use jiff::Timestamp;
use tokio::sync::{Mutex, watch};
use tokio_util::sync::CancellationToken;
use tracing::warn;

use super::{DiscoveryServerConfig, DiscoveryServerError};
use crate::{
    DiscoverySnapshot, ServiceInstance, ServiceInstanceId, api, grpc::decode_service_instance,
};

const INSTANCE_KEY_PREFIX: &str = "instances/";
const TOMBSTONE_GRACE_PERIOD: Duration = Duration::from_secs(15 * 60);

pub(super) struct Registry {
    chitchat: Arc<Mutex<Chitchat>>,
    owners: Mutex<BTreeMap<ServiceInstanceId, u64>>,
    snapshot: watch::Sender<Arc<DiscoverySnapshot>>,
    next_connection_id: AtomicU64,
}

impl Registry {
    pub(super) fn new(chitchat: Arc<Mutex<Chitchat>>) -> Self {
        let (snapshot, _) = watch::channel(Arc::new(DiscoverySnapshot::default()));

        Self {
            chitchat,
            owners: Mutex::new(BTreeMap::new()),
            snapshot,
            next_connection_id: AtomicU64::new(1),
        }
    }

    pub(super) fn next_connection_id(&self) -> u64 {
        self.next_connection_id.fetch_add(1, Ordering::Relaxed)
    }

    pub(super) fn snapshots(&self) -> watch::Receiver<Arc<DiscoverySnapshot>> {
        self.snapshot.subscribe()
    }

    pub(super) async fn publish(
        &self,
        connection_id: u64,
        id: ServiceInstanceId,
        registration: api::PublishedServiceInstance,
    ) -> Result<(), serde_json::Error> {
        let key = instance_key(id);
        let value = serde_json::to_string(&registration)?;
        self.owners.lock().await.insert(id, connection_id);
        self.chitchat.lock().await.self_node_state().set(key, value);
        self.refresh_snapshot().await;
        Ok(())
    }

    pub(super) async fn withdraw(&self, connection_id: u64, id: ServiceInstanceId) -> bool {
        let removed = {
            let mut owners = self.owners.lock().await;
            if owners.get(&id) == Some(&connection_id) {
                owners.remove(&id);
                true
            } else {
                false
            }
        };
        if !removed {
            return false;
        }

        self.chitchat
            .lock()
            .await
            .self_node_state()
            .delete(&instance_key(id));
        self.refresh_snapshot().await;
        true
    }

    async fn refresh_snapshot(&self) {
        let snapshot = {
            let chitchat = self.chitchat.lock().await;
            snapshot_from_nodes(
                chitchat
                    .live_nodes()
                    .filter_map(|id| chitchat.node_state(id)),
            )
        };
        self.snapshot.send_replace(Arc::new(snapshot));
    }
}

pub(super) fn decode_published_instance(
    registration: &api::PublishedServiceInstance,
) -> Result<ServiceInstance, String> {
    if registration.revision == 0 {
        return Err("registration revision must be greater than zero".to_owned());
    }

    registration
        .instance
        .as_ref()
        .ok_or_else(|| "published service instance is missing".to_owned())
        .and_then(|instance| decode_service_instance(instance).map_err(|error| error.to_string()))
}

pub(super) async fn project_snapshots(registry: Arc<Registry>, cancellation: CancellationToken) {
    let mut live_nodes = registry.chitchat.lock().await.live_nodes_watcher();
    registry.refresh_snapshot().await;

    loop {
        tokio::select! {
            _ = cancellation.cancelled() => return,
            changed = live_nodes.changed() => {
                if changed.is_err() {
                    return;
                }
                registry.snapshot.send_replace(Arc::new(snapshot_from_nodes(
                    live_nodes.borrow().values(),
                )));
            }
        }
    }
}

fn snapshot_from_nodes<'a>(nodes: impl IntoIterator<Item = &'a NodeState>) -> DiscoverySnapshot {
    let mut registrations = BTreeMap::<ServiceInstanceId, (u64, ServiceInstance)>::new();
    for node in nodes {
        for (_, value) in node.iter_prefix(INSTANCE_KEY_PREFIX) {
            let registration =
                match serde_json::from_str::<api::PublishedServiceInstance>(&value.value) {
                    Ok(registration) => registration,
                    Err(error) => {
                        warn!(%error, "ignored invalid Discovery advertisement");
                        continue;
                    }
                };
            let instance = match decode_published_instance(&registration) {
                Ok(instance) => instance,
                Err(error) => {
                    warn!(%error, "ignored invalid Discovery advertisement");
                    continue;
                }
            };
            let replace = registrations
                .get(&instance.id)
                .is_none_or(|(revision, _)| registration.revision > *revision);
            if replace {
                registrations.insert(instance.id, (registration.revision, instance));
            }
        }
    }

    DiscoverySnapshot::from_instances(registrations.into_values().map(|(_, instance)| instance))
}

pub(super) fn chitchat_config(
    config: &DiscoveryServerConfig,
) -> Result<ChitchatConfig, DiscoveryServerError> {
    let generation_id = Timestamp::now()
        .as_millisecond()
        .try_into()
        .map_err(|_| DiscoveryServerError::GenerationOverflow)?;

    Ok(ChitchatConfig {
        chitchat_id: ChitchatId::new(config.node_id.clone(), generation_id, config.advertise_addr),
        cluster_id: config.cluster_id.clone(),
        gossip_interval: config.gossip_interval,
        listen_addr: config.listen_addr,
        seed_nodes: config.seed_nodes.clone(),
        failure_detector_config: FailureDetectorConfig::default(),
        marked_for_deletion_grace_period: TOMBSTONE_GRACE_PERIOD,
        catchup_callback: None,
        extra_liveness_predicate: None,
        protocol_version: ProtocolVersion::V1,
    })
}

fn instance_key(id: ServiceInstanceId) -> String {
    format!("{INSTANCE_KEY_PREFIX}{}", id.as_uuid())
}
