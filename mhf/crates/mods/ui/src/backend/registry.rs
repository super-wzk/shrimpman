use std::sync::{Arc, Mutex, PoisonError};

use crate::backend::{InputCapture, InputPolicy, Overlay, egui};

type SharedContribution = Arc<Mutex<Option<Contribution>>>;

struct Contribution {
    overlay: Box<dyn Overlay>,
    initialized: bool,
}

/// UI contributions sharing one rendering context and input broker.
///
/// Contributions render in registration order. Registrations may be added
/// before or after D3D initialization; a new contribution initializes on the
/// rendering thread before its first UI callback.
#[derive(Clone, Default)]
pub struct OverlayRegistry {
    entries: Arc<Mutex<Vec<SharedContribution>>>,
}

impl OverlayRegistry {
    /// Adds a contribution until the returned registration is removed or dropped.
    pub fn register(&self, overlay: Box<dyn Overlay>) -> OverlayRegistration {
        let entry = Arc::new(Mutex::new(Some(Contribution {
            overlay,
            initialized: false,
        })));
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(Arc::clone(&entry));
        OverlayRegistration {
            registry: self.clone(),
            entry: Some(entry),
        }
    }

    fn snapshot(&self) -> Vec<SharedContribution> {
        self.entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

impl Overlay for OverlayRegistry {
    fn initialize(&mut self, context: &egui::Context) {
        for entry in self.snapshot() {
            if let Some(contribution) = entry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                contribution.overlay.initialize(context);
                contribution.initialized = true;
            }
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui) {
        for entry in self.snapshot() {
            if let Some(contribution) = entry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
            {
                if !contribution.initialized {
                    contribution.overlay.initialize(ui.ctx());
                    contribution.initialized = true;
                }
                contribution.overlay.ui(ui);
            }
        }
    }

    fn input_policy(&self, context: &egui::Context) -> InputPolicy {
        let mut policy = InputPolicy {
            pointer: InputCapture::PassThrough,
            keyboard: InputCapture::PassThrough,
        };
        for entry in self.snapshot() {
            if let Some(contribution) = entry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .filter(|contribution| contribution.initialized)
            {
                let requested = contribution.overlay.input_policy(context);
                policy.pointer = stronger_capture(policy.pointer, requested.pointer);
                policy.keyboard = stronger_capture(policy.keyboard, requested.keyboard);
            }
        }
        policy
    }

    fn platform_output(&mut self, context: &egui::Context, output: &egui::PlatformOutput) {
        for entry in self.snapshot() {
            if let Some(contribution) = entry
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_mut()
                .filter(|contribution| contribution.initialized)
            {
                contribution.overlay.platform_output(context, output);
            }
        }
    }
}

fn stronger_capture(left: InputCapture, right: InputCapture) -> InputCapture {
    match (left, right) {
        (InputCapture::Block, _) | (_, InputCapture::Block) => InputCapture::Block,
        (InputCapture::Auto, _) | (_, InputCapture::Auto) => InputCapture::Auto,
        _ => InputCapture::PassThrough,
    }
}

/// Owns one registered UI contribution.
///
/// Unregistering waits for its current callback to finish and prevents later
/// callbacks. Do not unregister a contribution from inside its own callback.
#[must_use = "dropping the registration removes the UI contribution"]
pub struct OverlayRegistration {
    registry: OverlayRegistry,
    entry: Option<SharedContribution>,
}

impl OverlayRegistration {
    /// Removes the contribution and synchronously releases its callback state.
    pub fn unregister(&mut self) {
        let Some(entry) = self.entry.take() else {
            return;
        };
        self.registry
            .entries
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .retain(|registered| !Arc::ptr_eq(registered, &entry));
        // Neither callbacks nor contribution destruction run under the registry
        // lock. A snapshot already taken by the renderer observes None after
        // removal, while taking this lock drains any callback already running.
        let contribution = entry.lock().unwrap_or_else(PoisonError::into_inner).take();
        drop(contribution);
    }
}

impl Drop for OverlayRegistration {
    fn drop(&mut self) {
        self.unregister();
    }
}

#[cfg(test)]
mod tests;
