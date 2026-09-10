//! Quest interfaces and the optional offline quest provider.

pub mod api;
pub use api::*;

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF quest provider requires i686 Windows");

#[cfg(feature = "provider")]
pub mod provider;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use provider::QuestMod;
#[cfg(feature = "provider")]
pub use provider::{QuestService, Session};
