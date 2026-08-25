//! Shared persistent models and repository implementations.

#![warn(unreachable_pub)]

mod account;
mod character;
mod config;
mod mezeporta;
mod sign_in_notice;
mod sign_session;

pub use account::AccountRepository;
pub use character::CharacterRepository;
pub use config::MigrationConfig;
pub use mezeporta::MezeportaFestaRepository;
pub use sign_in_notice::SignInNoticeRepository;
pub use sign_session::SignSessionRepository;

/// Returns the complete persistent model graph.
pub fn models() -> toasty::ModelSet {
    toasty::models!(crate::*)
}

#[cfg(test)]
async fn test_database() -> toasty::Db {
    let mut builder = toasty::Db::builder();
    builder.models(models());
    let db = builder.connect("sqlite::memory:").await.unwrap();
    db.push_schema().await.unwrap();
    db
}
