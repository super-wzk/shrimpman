use std::sync::Arc;

/// Dependencies shared by every Sign connection.
pub struct SignServiceContext;

/// State and dependencies belonging to one Sign connection.
pub struct SignSessionContext {
    service: Arc<SignServiceContext>,
}

impl SignSessionContext {
    pub(crate) fn new(service: Arc<SignServiceContext>) -> Self {
        Self { service }
    }

    /// Returns the dependencies shared by the Sign service.
    pub fn service_context(&self) -> &SignServiceContext {
        &self.service
    }
}
