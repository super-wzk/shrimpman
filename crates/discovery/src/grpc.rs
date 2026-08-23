use thiserror::Error;

use crate::{InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName, api};

pub(crate) const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

pub(crate) fn decode_service_instance(
    instance: &api::ServiceInstance,
) -> Result<ServiceInstance, GrpcModelError> {
    let state = api::ServiceState::try_from(instance.state)
        .map_err(|_| GrpcModelError::State(instance.state))?;
    if state == api::ServiceState::Unspecified {
        return Err(GrpcModelError::State(instance.state));
    }

    Ok(ServiceInstance {
        id: ServiceInstanceId::from_bytes(&instance.id)?,
        service: ServiceName::new(instance.service.clone())?,
        state,
        metadata: serde_json::from_slice(&instance.metadata)?,
    })
}

pub(crate) fn encode_service_instance(
    instance: &ServiceInstance,
) -> Result<api::ServiceInstance, serde_json::Error> {
    Ok(api::ServiceInstance {
        id: instance.id.as_uuid().as_bytes().to_vec(),
        service: instance.service.as_str().to_owned(),
        state: instance.state.into(),
        metadata: serde_json::to_vec(&instance.metadata)?,
    })
}

#[derive(Debug, Error)]
pub(crate) enum GrpcModelError {
    #[error("invalid service instance ID: {0}")]
    Id(#[from] uuid::Error),
    #[error(transparent)]
    ServiceName(#[from] InvalidServiceName),
    #[error("invalid service state code {0}")]
    State(i32),
    #[error("invalid service metadata: {0}")]
    Metadata(#[from] serde_json::Error),
}
