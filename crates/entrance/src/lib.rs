//! Entrance service envelope decoding, routing, and process lifecycle.

#![warn(unreachable_pub)]

mod application;
mod config;
mod envelope;
mod router;
mod server;

pub use application::{
    ConnectionError, EntranceService, EntranceServiceContext, EntranceSessionContext, InternalError,
};
pub use config::{
    DiscoveryClientConfig, EntranceConfig, EntranceLoggingConfig, EntranceServerConfig,
};
pub use envelope::{Command, CommandDecodeError};
pub use router::{EntranceRouteError, EntranceRouterBuildError};
pub use server::EntranceServer;
