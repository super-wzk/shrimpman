//! Font-family API with optional embedded resources and native font provider.

pub mod api;
pub use api::*;

#[cfg(feature = "provider")]
mod resources;
#[cfg(feature = "provider")]
pub use resources::install;

#[cfg(all(feature = "provider", windows))]
mod service;
#[cfg(all(feature = "provider", windows))]
pub(crate) use service::FontService;

#[cfg(all(feature = "provider", windows))]
mod native;
#[cfg(all(feature = "provider", windows))]
pub use native::{HookState, TextRenderer, corrected_y, install_game};
#[cfg(all(feature = "provider", windows))]
mod module;
#[cfg(all(feature = "provider", windows))]
pub use module::FontMod;
