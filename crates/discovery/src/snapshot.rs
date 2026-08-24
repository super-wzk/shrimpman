use std::{collections::BTreeMap, sync::Arc};

use crate::{ServiceInstance, ServiceName};

/// Immutable local view of the currently discoverable services.
#[derive(Clone, Debug, Default)]
pub struct DiscoverySnapshot {
    services: BTreeMap<ServiceName, Arc<[ServiceInstance]>>,
}

impl DiscoverySnapshot {
    pub(crate) fn from_instances(instances: impl IntoIterator<Item = ServiceInstance>) -> Self {
        let mut grouped = BTreeMap::<ServiceName, Vec<ServiceInstance>>::new();
        for instance in instances {
            grouped
                .entry(instance.service.clone())
                .or_default()
                .push(instance);
        }

        let services = grouped
            .into_iter()
            .map(|(service, mut instances)| {
                instances.sort_by_key(|instance| instance.id);
                (service, Arc::from(instances))
            })
            .collect();

        Self { services }
    }

    /// Returns every live instance registered under `service`.
    pub fn instances(&self, service: &ServiceName) -> Arc<[ServiceInstance]> {
        self.services
            .get(service)
            .cloned()
            .unwrap_or_else(|| Arc::from([]))
    }
}
