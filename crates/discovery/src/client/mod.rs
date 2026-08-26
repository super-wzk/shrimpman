use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use shrimpman_kv::{KvEntry, KvWatch, KvWatchEvent, LeaseKvClient, LeaseKvClientError};
use thiserror::Error;
use tokio::sync::watch;
use tracing::warn;

use crate::{
    InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName, ServiceState,
    snapshot::DiscoverySnapshot,
};

const SERVICE_KEY_PREFIX: &str = "/shrimpman/services/";

/// Cloneable handle used by a business service to publish and discover instances.
#[derive(Clone)]
pub struct DiscoveryClient {
    kv: LeaseKvClient,
    published_keys: Arc<Mutex<BTreeMap<ServiceInstanceId, String>>>,
    snapshot: watch::Receiver<Option<Arc<DiscoverySnapshot>>>,
}

impl DiscoveryClient {
    /// Projects service registrations from a process-wide leased key-value client.
    pub fn new(kv: LeaseKvClient) -> Self {
        let events = kv.watch_prefix(SERVICE_KEY_PREFIX);
        let (snapshot_tx, snapshot_rx) = watch::channel(None);
        tokio::spawn(project_snapshots(events, snapshot_tx));

        Self {
            kv,
            published_keys: Arc::default(),
            snapshot: snapshot_rx,
        }
    }

    /// Publishes or replaces this process's current advertisement.
    pub fn publish(&self, instance: ServiceInstance) -> Result<(), DiscoveryClientError> {
        let id = instance.id;
        let key = instance_key(&instance);
        let value = encode_instance(&instance)?;
        let mut published_keys = self
            .published_keys
            .lock()
            .expect("published key lock poisoned");

        if let Some(previous_key) = published_keys.get(&id)
            && previous_key != &key
        {
            self.kv.delete(previous_key.clone())?;
        }
        self.kv.put(key.clone(), value)?;
        published_keys.insert(id, key);

        Ok(())
    }

    /// Removes a previously published advertisement.
    pub fn withdraw(&self, id: ServiceInstanceId) -> Result<(), DiscoveryClientError> {
        let mut published_keys = self
            .published_keys
            .lock()
            .expect("published key lock poisoned");
        let Some(key) = published_keys.remove(&id) else {
            return Ok(());
        };

        if let Err(error) = self.kv.delete(key.clone()) {
            published_keys.insert(id, key);
            return Err(error.into());
        }

        Ok(())
    }

    /// Returns every currently known instance registered under `service`.
    pub fn instances(&self, service: &ServiceName) -> Arc<[ServiceInstance]> {
        self.snapshot
            .borrow()
            .as_deref()
            .map(|snapshot| snapshot.instances(service))
            .unwrap_or_else(|| Arc::from([]))
    }
}

/// Failure to encode or publish a service registration.
#[derive(Debug, Error)]
pub enum DiscoveryClientError {
    #[error("failed to encode a service registration: {0}")]
    Encode(#[from] serde_json::Error),
    #[error(transparent)]
    Kv(#[from] LeaseKvClientError),
}

async fn project_snapshots(
    mut events: KvWatch,
    snapshots: watch::Sender<Option<Arc<DiscoverySnapshot>>>,
) {
    let mut registrations = BTreeMap::<String, ServiceInstance>::new();

    loop {
        let event = tokio::select! {
            event = events.recv() => event,
            () = snapshots.closed() => return,
        };
        let Some(event) = event else {
            break;
        };

        match event {
            KvWatchEvent::Synchronized(entries) => {
                registrations.clear();
                for entry in entries {
                    apply_entry(&mut registrations, entry);
                }
            }
            KvWatchEvent::Put(entry) => {
                apply_entry(&mut registrations, entry);
            }
            KvWatchEvent::Delete(key) => {
                registrations.remove(&key);
            }
            KvWatchEvent::Unavailable => {
                registrations.clear();
                snapshots.send_replace(None);
                continue;
            }
        }

        snapshots.send_replace(Some(Arc::new(DiscoverySnapshot::from_instances(
            registrations.values().cloned(),
        ))));
    }

    snapshots.send_replace(None);
}

fn apply_entry(registrations: &mut BTreeMap<String, ServiceInstance>, entry: KvEntry) {
    let (key, value) = entry.into_parts();
    match decode_instance(&key, &value) {
        Ok(instance) => {
            registrations.insert(key, instance);
        }
        Err(error) => {
            registrations.remove(&key);
            warn!(%key, %error, "Ignored invalid service registration");
        }
    }
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

fn decode_instance(key: &str, value: &[u8]) -> Result<ServiceInstance, StoredInstanceError> {
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

#[derive(Deserialize, Serialize)]
struct StoredServiceInstance<Metadata> {
    state: ServiceState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    advertise_addr: Option<String>,
    registered_at: Timestamp,
    metadata: Metadata,
}

#[derive(Debug, Error)]
enum StoredInstanceError {
    #[error("invalid service registration key")]
    Key,
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
    fn round_trips_key_value_registration() {
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

        assert_eq!(decode_instance(&key, &value).unwrap(), instance);
    }
}
