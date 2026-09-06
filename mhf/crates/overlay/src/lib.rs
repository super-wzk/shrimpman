//! An `egui` overlay rendered inside a hooked Direct3D 9 device.
//!
//! The crate owns only its D3D9 `Present` and `Reset` hooks. It does not own
//! process injection and it never enables, disables, or uninitializes unrelated
//! `MinHook` hooks in the host process.

pub use egui;

mod capture;
pub use capture::{InputCapture, InputCaptureState, InputPolicy};

#[cfg(target_os = "windows")]
pub mod dx9;
#[cfg(target_os = "windows")]
mod input;
#[cfg(target_os = "windows")]
mod renderer;
#[cfg(target_os = "windows")]
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

    /// Builds the UI using the root `egui::Ui` supplied by `Context::run_ui`.
    /// Native layouts and panels can use this directly; floating windows and
    /// areas use `ui.ctx()`.
    fn ui(&mut self, ui: &mut egui::Ui);

    /// Selects which input is withheld from the game. The window procedure and
    /// `D3d9Hook::input_capture` share these decisions with native input adapters.
    /// Queried after `ui` each frame; raw input still reaches egui in every mode.
    /// The caller can choose a policy from its active window/page/modal state.
    /// DirectInput, Raw Input and device polling need a separate game adapter.
    fn input_policy(&self, _context: &egui::Context) -> InputPolicy {
        InputPolicy::default()
    }

    /// Receives non-rendering output such as clipboard and URL requests.
    ///
    /// The default implementation ignores it. Keeping this boundary explicit
    /// avoids opening URLs or changing the host clipboard without the caller's
    /// participation.
    fn platform_output(&mut self, _context: &egui::Context, _output: &egui::PlatformOutput) {}
}

impl<F> Overlay for F
where
    F: FnMut(&mut egui::Ui) + Send + 'static,
{
    fn ui(&mut self, ui: &mut egui::Ui) {
        self(ui);
    }
}

/// Failure while installing, rendering, or removing an overlay hook.
#[derive(Debug)]
pub struct Error {
    message: String,
}

impl Error {
    #[cfg(target_os = "windows")]
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

#[cfg(target_os = "windows")]
type Result<T> = std::result::Result<T, Error>;
