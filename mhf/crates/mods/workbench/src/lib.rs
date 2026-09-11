//! Resource inspection and native previews, independently selectable from Debug.

pub mod catalog;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
#[cfg_attr(
    not(all(feature = "provider", windows, target_arch = "x86")),
    allow(dead_code)
)]
mod guides;
pub mod inspect;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
#[cfg_attr(
    not(all(feature = "provider", windows, target_arch = "x86")),
    allow(dead_code)
)]
mod preview;
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
#[cfg_attr(
    not(all(feature = "provider", windows, target_arch = "x86")),
    allow(dead_code)
)]
mod settings;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod ui;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod worker;

#[cfg(all(feature = "provider", windows, not(target_arch = "x86")))]
compile_error!("the MHF workbench requires i686 Windows");

#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod module;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
mod native;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub use module::WorkbenchModule;
