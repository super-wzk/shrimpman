//! Service registration and discovery backed by Chitchat.

#![warn(unreachable_pub)]

pub mod client;
mod grpc;
mod model;
mod snapshot;

/// Generated gRPC request, response, client, and server types.
pub mod api {
    tonic::include_proto!("shrimpman.discovery.v1");
}

pub mod server;

pub use api::ServiceState;
pub use model::{InvalidServiceName, ServiceInstance, ServiceInstanceId, ServiceName};
pub use snapshot::DiscoverySnapshot;
