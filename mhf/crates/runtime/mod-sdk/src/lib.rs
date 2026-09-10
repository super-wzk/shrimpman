//! Mod capabilities own their Rust API and native bridge together.

/// The independent host C protocol, owned by mhf-mod-api. Capability APIs and
/// their native adapters are defined once in their respective modules.
pub use mhf_mod_api as abi;

pub mod error;
pub mod hooks;
pub mod host;
pub mod interface;
pub mod lifecycle;

pub use error::{Error, ErrorKind, Result};
pub use host::{Dependencies, GameInfo, GameModule, Host, LogLevel, Phase};
pub use lifecycle::Mod;

#[cfg(test)]
mod tests;
