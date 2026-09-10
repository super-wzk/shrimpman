//! Stable translation keys, provider bindings, configuration, and optional dictionaries.

pub mod api;
pub use api::*;
pub mod offline_quest;

#[cfg(feature = "provider")]
mod provider;
#[cfg(feature = "provider")]
pub use provider::{TranslationMod, TranslationService};
