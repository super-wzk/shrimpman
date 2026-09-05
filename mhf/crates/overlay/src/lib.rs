//! An `egui` overlay rendered inside a hooked Direct3D 9 device.
//!
//! The crate owns only its D3D9 `Present` and `Reset` hooks. It does not own
//! process injection and it never enables, disables, or uninitializes unrelated
//! `MinHook` hooks in the host process.

#![cfg(target_os = "windows")]

pub use egui;

pub mod dx9;
mod input;
mod renderer;
mod window;

use std::fmt;

/// User interface rendered by the overlay on every hooked frame.
///
/// Callbacks run synchronously on the host's D3D9 rendering thread and should
/// not perform blocking I/O or other long-running work. They must not uninstall
/// or drop the active hook from inside a callback.
pub trait Overlay: Send + 'static {
    /// Configures the context once, after the target D3D9 device is observed.
    fn initialize(&mut self, _context: &egui::Context) {}

    /// Builds one frame of `egui` UI.
    fn ui(&mut self, context: &egui::Context);

    /// Receives non-rendering output such as clipboard and URL requests.
    ///
    /// The default implementation ignores it. Keeping this boundary explicit
    /// avoids opening URLs or changing the host clipboard without the caller's
    /// participation.
    fn platform_output(&mut self, _context: &egui::Context, _output: &egui::PlatformOutput) {}
}

impl<F> Overlay for F
where
    F: FnMut(&egui::Context) + Send + 'static,
{
    fn ui(&mut self, context: &egui::Context) {
        self(context);
    }
}

/// Failure while installing, rendering, or removing an overlay hook.
#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

type Result<T> = std::result::Result<T, Error>;
