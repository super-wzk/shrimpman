use std::sync::Arc;

use shrimpman_discovery::client::DiscoveryClient;

/// Dependencies shared by Entrance request handlers.
pub struct EntranceServiceContext {
    discovery: DiscoveryClient,
}

impl EntranceServiceContext {
    /// Creates a service context from explicitly assembled dependencies.
    pub fn new(discovery: DiscoveryClient) -> Self {
        Self { discovery }
    }

    pub(crate) const fn discovery(&self) -> &DiscoveryClient {
        &self.discovery
    }
}

/// State and dependencies belonging to one Entrance connection.
pub struct EntranceSessionContext {
    service: Arc<EntranceServiceContext>,
}

impl EntranceSessionContext {
    pub(crate) const fn new(service: Arc<EntranceServiceContext>) -> Self {
        Self { service }
    }

    /// Returns the dependencies shared by the Entrance service.
    pub fn service_context(&self) -> &EntranceServiceContext {
        &self.service
    }
}
