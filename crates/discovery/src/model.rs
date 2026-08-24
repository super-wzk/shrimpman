use std::borrow::Borrow;

use jiff::Timestamp;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;
use thiserror::Error;
use uuid::Uuid;

/// A validated service identifier used as a discovery key.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ServiceName(String);

impl ServiceName {
    /// Creates a service name containing lowercase ASCII letters, digits, or hyphens.
    pub fn new(value: impl Into<String>) -> Result<Self, InvalidServiceName> {
        let value = value.into();
        if value.is_empty()
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            return Err(InvalidServiceName);
        }

        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl Borrow<str> for ServiceName {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl TryFrom<String> for ServiceName {
    type Error = InvalidServiceName;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ServiceName> for String {
    fn from(value: ServiceName) -> Self {
        value.0
    }
}

/// The supplied service name cannot be used as a discovery key.
#[derive(Debug, Error)]
#[error("service names must contain only lowercase ASCII letters, digits, or hyphens")]
pub struct InvalidServiceName;

/// Identifies one running incarnation of a service.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct ServiceInstanceId(Uuid);

impl ServiceInstanceId {
    /// Generates a time-ordered identifier for a new service process.
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    pub(crate) const fn as_uuid(self) -> Uuid {
        self.0
    }

    pub(crate) const fn from_uuid(value: Uuid) -> Self {
        Self(value)
    }
}

impl Default for ServiceInstanceId {
    fn default() -> Self {
        Self::new()
    }
}

/// Lifecycle state advertised by a service instance.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub enum ServiceState {
    Starting,
    Ready,
    Draining,
}

/// A service advertisement whose metadata is opaque to Discovery.
#[derive(Clone, Debug, PartialEq)]
pub struct ServiceInstance {
    pub id: ServiceInstanceId,
    pub service: ServiceName,
    pub state: ServiceState,
    pub advertise_addr: Option<String>,
    pub registered_at: Timestamp,
    pub metadata: Value,
}

impl ServiceInstance {
    /// Creates an advertisement by serializing service-owned metadata.
    pub fn new<Metadata>(
        id: ServiceInstanceId,
        service: ServiceName,
        state: ServiceState,
        advertise_addr: Option<String>,
        metadata: Metadata,
    ) -> serde_json::Result<Self>
    where
        Metadata: Serialize,
    {
        Ok(Self {
            id,
            service,
            state,
            advertise_addr,
            registered_at: Timestamp::now(),
            metadata: serde_json::to_value(metadata)?,
        })
    }

    /// Decodes the opaque metadata into a service-owned type.
    pub fn decode_metadata<Metadata>(&self) -> serde_json::Result<Metadata>
    where
        Metadata: DeserializeOwned,
    {
        serde_json::from_value(self.metadata.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_service_names() {
        assert_eq!(
            ServiceName::new("entrance-2").unwrap().as_str(),
            "entrance-2"
        );

        for invalid in ["", "Entrance", "world_service", "world service"] {
            assert!(ServiceName::new(invalid).is_err());
        }
    }

    #[test]
    fn round_trips_typed_metadata() {
        #[derive(Debug, PartialEq, Serialize, serde::Deserialize)]
        struct Metadata {
            region: String,
        }

        let instance = ServiceInstance::new(
            ServiceInstanceId::new(),
            ServiceName::new("entrance").unwrap(),
            ServiceState::Ready,
            Some("entrance.example.com:53310".to_owned()),
            Metadata {
                region: "ap-east-1".to_owned(),
            },
        )
        .unwrap();

        assert_eq!(
            instance.advertise_addr.as_deref(),
            Some("entrance.example.com:53310")
        );
        assert_eq!(
            instance.decode_metadata::<Metadata>().unwrap(),
            Metadata {
                region: "ap-east-1".to_owned(),
            }
        );
    }
}
