//! Debug snapshots, commands and the optional in-process tools provider.

pub mod api;
pub use api::*;

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF debug tools provider requires i686 Windows");

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod provider;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use provider::DebugToolsMod;
