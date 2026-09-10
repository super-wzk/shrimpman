//! UI capability, panel provider, and its native rendering backend.

pub mod api;
pub use api::*;

#[cfg(feature = "provider")]
mod backend;
#[cfg(feature = "provider")]
pub use backend::{
    Error, InputCapture, InputCaptureState, InputPolicy, Overlay, OverlayRegistration,
    OverlayRegistry, egui,
};
#[cfg(all(feature = "provider", windows))]
pub use backend::{HostIme, HostImeTarget, dx9};

#[cfg(all(feature = "provider", windows))]
mod service;
#[cfg(all(feature = "provider", windows))]
pub(crate) use service::UiService;

#[cfg(all(feature = "provider", windows))]
mod module;
#[cfg(all(feature = "provider", windows))]
mod native;
#[cfg(all(feature = "provider", windows))]
pub use module::UiMod;
