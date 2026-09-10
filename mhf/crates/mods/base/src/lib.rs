//! The built-in `mhf.base` runtime, assembled from the native support components.

pub mod config;
pub use config::*;

#[cfg(all(windows, not(target_arch = "x86")))]
compile_error!("the MHF base runtime requires i686 Windows");

#[cfg(all(windows, target_arch = "x86"))]
mod module;
#[cfg(all(windows, target_arch = "x86"))]
pub use module::BaseMod;
