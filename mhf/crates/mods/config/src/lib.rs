//! Independent configuration registration, persistence and INI bridge.

pub mod api;
pub use api::*;
#[cfg(feature = "provider")]
mod service;
#[cfg(feature = "provider")]
pub use service::Store;
#[cfg(feature = "provider")]
mod provider;
#[cfg(feature = "provider")]
pub use provider::ConfigService;
#[cfg(all(feature = "provider", windows))]
mod module;
#[cfg(all(feature = "provider", windows))]
mod native;
#[cfg(all(feature = "provider", windows))]
pub use module::ConfigMod;
