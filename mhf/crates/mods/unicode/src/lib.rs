//! Native Unicode text support and legacy-resource decoding.
//! Decoding is available before a game or Mod has been started.

mod encoding;
pub use encoding::decode_source;

#[cfg(all(feature = "provider", not(all(windows, target_arch = "x86"))))]
compile_error!("the Unicode provider requires i686 Windows");

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod provider;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use provider::{UnicodeMod, gdi_renderer};
