//! Species-limit instruction patches for the verified ZZ HD client.
//! Base owns this adapter. Expanded native tables and resources are supplied
//! separately; these patches alone do not register additional monster species.

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF monster adapter requires i686 Windows");

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod module;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod native;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod patches;

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use module::MonsterMod;
