//! Strongly typed service registration and discovery backed by etcd.

#![warn(unreachable_pub)]

pub mod client;
mod model;
pub mod selector;
mod snapshot;

pub use model::{
    InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName, ServiceState,
};
pub use snapshot::DiscoverySnapshot;
