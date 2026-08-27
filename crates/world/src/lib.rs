//! World process configuration, Land protocol routing, and service lifecycle.

#![warn(unreachable_pub)]

mod application;
mod config;
mod database;
mod envelope;
mod exchange;
mod response;
mod router;
mod server;

pub use application::{
    ConnectionError, InternalError, PacketDecodeError, WorldRepositories, WorldService,
    WorldSession,
};
pub use config::{
    LeaseKvClientConfig, WorldConfig, WorldDatabaseConfig, WorldLandConfig, WorldLoggingConfig,
};
pub use database::WorldDatabase;
pub use router::{LandRouteError, LandRouterBuildError};
pub use server::WorldServer;

#[cfg(test)]
async fn test_database() -> toasty::Db {
    let mut builder = toasty::Db::builder();
    builder.models(shrimpman_persistence::models());
    let db = builder.connect("sqlite::memory:").await.unwrap();
    db.push_schema().await.unwrap();
    db
}

#[cfg(test)]
fn test_repositories(db: &toasty::Db) -> WorldRepositories {
    WorldRepositories::new(
        shrimpman_persistence::CharacterRepository::new(db),
        shrimpman_persistence::SignSessionRepository::new(db),
    )
}
