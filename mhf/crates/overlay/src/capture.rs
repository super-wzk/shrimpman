use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

#[cfg(any(target_os = "windows", test))]
use egui::Context;
use egui::{Pos2, Rect};

/// Whether one input channel is withheld from the game. Egui still receives it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputCapture {
    /// Follow egui's current hover, drag or keyboard-focus intent.
    #[default]
    Auto,
    /// Withhold input even when egui has no hovered or focused control.
    Block,
    /// Forward input even when egui is handling it.
    PassThrough,
}

impl InputCapture {
    /// Resolve against the current UI intent. Game-specific input adapters can
    /// use the same decision for their own input path.
    pub fn captures(self, egui_wants_input: bool) -> bool {
        match self {
            Self::Auto => egui_wants_input,
            Self::Block => true,
            Self::PassThrough => false,
        }
    }
}

/// Caller-owned policy for the overlay's mouse and keyboard channels.
/// Defaults to automatic capture for both. A policy change applies to new
/// presses; a held button/key keeps its original recipient through release.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputPolicy {
    pub pointer: InputCapture,
    pub keyboard: InputCapture,
}

/// Shared capture decisions for native input adapters. Obtain this from
/// `D3d9Hook::input_capture`; it follows the same caller policy as window messages.
/// Adapters must retain each sampled press's owner until the device releases it.
#[derive(Clone, Default)]
pub struct InputCaptureState(Arc<Mutex<CaptureState>>);

impl InputCaptureState {
    /// Whether a new mouse press or wheel movement belongs to the overlay.
    /// On Windows, hit-testing uses the current cursor position in the game's
    /// client area, even if no UI frame has processed that movement yet.
    pub fn captures_pointer(&self) -> bool {
        let state = self.lock();
        #[cfg(target_os = "windows")]
        let position = crate::window::cursor_position(state.window)
            .map(|position| position / state.pixels_per_point);
        #[cfg(not(target_os = "windows"))]
        let position = None;
        state.pointer_at(position)
    }

    /// Whether a newly pressed keyboard key belongs to the overlay.
    pub fn captures_keyboard(&self) -> bool {
        self.lock().keyboard
    }

    /// Recipient of a mouse press already observed by the window procedure.
    /// Indices are left, right, middle, X1, X2; `true` means the overlay owns it.
    /// Device adapters use this for their first sample if the cursor has moved
    /// since the native press, then retain that owner through physical release.
    pub fn pointer_button_owner(&self, button: usize) -> Option<bool> {
        self.lock().buttons.get(button).copied().flatten()
    }

    /// Recipient of a keyboard press already observed by the window procedure.
    /// Uses DirectInput scan codes (extended keys set bit 7), not virtual keys.
    pub fn scan_code_owner(&self, scan_code: usize) -> Option<bool> {
        self.lock().keys.get(scan_code).copied().flatten()
    }

    pub(super) fn lock(&self) -> MutexGuard<'_, CaptureState> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn update(&self, policy: InputPolicy, context: &Context) {
        // Overlay callbacks build floating windows/areas; run_ui's background
        // remains the game. Copy geometry so native callbacks never lock egui.
        let layers = context.memory(|memory| {
            memory
                .areas()
                .visible_layer_ids()
                .into_iter()
                .filter(|layer| {
                    *layer != egui::LayerId::background() && memory.allows_interaction(*layer)
                })
                .collect::<Vec<_>>()
        });
        let clip = context.content_rect();
        let rects = layers
            .into_iter()
            .filter_map(|layer| {
                let area = <egui::Area as egui::WidgetWithState>::State::load(context, layer.id)?;
                if !area.interactable {
                    return None;
                }
                let rect = context
                    .layer_transform_to_global(layer)
                    .map_or(area.rect(), |transform| transform * area.rect());
                Some(rect.intersect(clip))
            })
            .collect();
        let pointer = context.egui_wants_pointer_input();
        let keyboard = context.egui_wants_keyboard_input();
        let pixels_per_point = context.pixels_per_point();
        let mut state = self.lock();
        state.update(policy, pointer, keyboard);
        state.rects = rects;
        state.pixels_per_point = pixels_per_point;
    }

    #[cfg(target_os = "windows")]
    pub(super) fn clear(&self) {
        self.lock().clear();
    }

    #[cfg(target_os = "windows")]
    pub(super) fn reset(&self) {
        self.lock().reset();
    }
}

pub(super) struct CaptureState {
    #[cfg(target_os = "windows")]
    pub(super) window: usize,
    #[cfg(any(target_os = "windows", test))]
    pixels_per_point: f32,
    policy: InputPolicy,
    rects: Vec<Rect>,
    pointer: bool,
    keyboard: bool,
    buttons: [Option<bool>; 5],
    keys: [Option<bool>; 256],
}

impl Default for CaptureState {
    fn default() -> Self {
        Self {
            #[cfg(target_os = "windows")]
            window: 0,
            #[cfg(any(target_os = "windows", test))]
            pixels_per_point: 1.0,
            policy: InputPolicy {
                pointer: InputCapture::PassThrough,
                keyboard: InputCapture::PassThrough,
            },
            rects: Vec::new(),
            pointer: false,
            keyboard: false,
            buttons: [None; 5],
            keys: [None; 256],
        }
    }
}

impl CaptureState {
    fn pointer_at(&self, position: Option<Pos2>) -> bool {
        self.policy
            .pointer
            .captures(position.map_or(self.pointer, |position| {
                self.rects.iter().any(|rect| rect.contains(position))
            }))
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn move_pointer(&mut self, client_position: Pos2) {
        self.pointer = self.pointer_at(Some(client_position / self.pixels_per_point));
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn update(&mut self, policy: InputPolicy, pointer: bool, keyboard: bool) {
        self.policy = policy;
        self.pointer = policy.pointer.captures(pointer);
        self.keyboard = policy.keyboard.captures(keyboard);
    }

    /// Release capture for new input on a render failure. Existing presses must
    /// still receive a matching release at their original recipient.
    #[cfg(any(target_os = "windows", test))]
    pub(super) fn clear(&mut self) {
        self.policy.pointer = InputCapture::PassThrough;
        self.policy.keyboard = InputCapture::PassThrough;
        self.rects.clear();
        self.pointer = false;
        self.keyboard = false;
    }

    #[cfg(target_os = "windows")]
    pub(super) fn reset(&mut self) {
        self.clear();
        self.buttons.fill(None);
        self.keys.fill(None);
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn pointer(&self) -> bool {
        self.pointer
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn keyboard(&self) -> bool {
        self.keyboard
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn pointer_motion(&self) -> bool {
        if self.buttons.iter().any(Option::is_some) {
            self.buttons.contains(&Some(true))
        } else {
            self.pointer
        }
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn pointer_button(&mut self, button: usize, pressed: bool) -> bool {
        Self::route_press(&mut self.buttons, button, pressed, self.pointer)
    }

    #[cfg(any(target_os = "windows", test))]
    pub(super) fn key(&mut self, key: usize, pressed: bool) -> bool {
        Self::route_press(&mut self.keys, key, pressed, self.keyboard)
    }

    #[cfg(any(target_os = "windows", test))]
    fn route_press(
        owners: &mut [Option<bool>],
        index: usize,
        pressed: bool,
        capture: bool,
    ) -> bool {
        let Some(owner) = owners.get_mut(index) else {
            return capture;
        };
        if pressed {
            *owner.get_or_insert(capture)
        } else {
            // A press may predate overlay installation: let the game release it.
            owner.take().unwrap_or(false)
        }
    }
}

#[cfg(test)]
mod tests;
