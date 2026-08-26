//! Strongly typed service registration and discovery projected from leased key-value entries.

#![warn(unreachable_pub)]

pub mod client;
mod model;
pub mod selector;
mod snapshot;

pub use model::{
    InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName, ServiceState,
};
