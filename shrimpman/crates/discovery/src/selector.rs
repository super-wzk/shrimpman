use std::sync::atomic::{AtomicUsize, Ordering};

use crate::{ServiceInstance, ServiceState};

/// Chooses one service instance from a discovered candidate set.
pub trait Selector: Send + Sync {
    fn select<'a>(&self, instances: &'a [ServiceInstance]) -> Option<&'a ServiceInstance>;
}

/// Selects ready instances in round-robin order.
#[derive(Debug, Default)]
pub struct RoundRobinSelector {
    cursor: AtomicUsize,
}

impl RoundRobinSelector {
    pub const fn new() -> Self {
        Self {
            cursor: AtomicUsize::new(0),
        }
    }
}

impl Selector for RoundRobinSelector {
    fn select<'a>(&self, instances: &'a [ServiceInstance]) -> Option<&'a ServiceInstance> {
        let ready_count = ready_instances(instances).count();
        if ready_count == 0 {
            return None;
        }

        let index = self.cursor.fetch_add(1, Ordering::Relaxed) % ready_count;
        ready_instances(instances).nth(index)
    }
}

fn ready_instances(instances: &[ServiceInstance]) -> impl Iterator<Item = &ServiceInstance> {
    instances
        .iter()
        .filter(|instance| instance.state == ServiceState::Ready)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ServiceInstanceId, ServiceName};

    #[test]
    fn selects_ready_instances_round_robin() {
        let instances = [
            instance(ServiceState::Ready),
            instance(ServiceState::Draining),
            instance(ServiceState::Ready),
        ];
        let selector = RoundRobinSelector::new();

        assert_eq!(selected_id(&selector, &instances), Some(instances[0].id));
        assert_eq!(selected_id(&selector, &instances), Some(instances[2].id));
        assert_eq!(selected_id(&selector, &instances), Some(instances[0].id));
    }

    #[test]
    fn returns_none_without_a_ready_instance() {
        let instances = [instance(ServiceState::Draining)];

        assert!(RoundRobinSelector::new().select(&instances).is_none());
    }

    fn instance(state: ServiceState) -> ServiceInstance {
        ServiceInstance::new(
            ServiceInstanceId::new(),
            ServiceName::new("entrance").unwrap(),
            state,
            None,
            (),
        )
        .unwrap()
    }

    fn selected_id(
        selector: &impl Selector,
        instances: &[ServiceInstance],
    ) -> Option<ServiceInstanceId> {
        selector.select(instances).map(|instance| instance.id)
    }
}
