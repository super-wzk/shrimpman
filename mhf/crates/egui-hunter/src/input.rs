use std::time::Duration;

use egui::{Context, Event, Key, Modifiers, RawInput};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Right,
    Down,
    Left,
}

impl Direction {
    pub(crate) fn key(self) -> Key {
        match self {
            Self::Up => Key::ArrowUp,
            Self::Right => Key::ArrowRight,
            Self::Down => Key::ArrowDown,
            Self::Left => Key::ArrowLeft,
        }
    }
}

/// Held actions supplied by the host after dead zones and game/UI arbitration.
/// Supply `Default::default()` when the controller is disconnected or released.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GamepadState {
    pub direction: Option<Direction>,
    pub confirm: bool,
    pub cancel: bool,
    /// Map shoulder buttons to native Tab/Shift-Tab to leave a list or grid.
    pub next_focus: bool,
    pub previous_focus: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InputDevice {
    #[default]
    KeyboardMouse,
    Gamepad,
}

/// Converts controller actions into native egui key pulses. Keep one per viewport
/// and call `apply` once before `Context::run` (eframe: `App::raw_input_hook`).
/// Direction keys repeat; other actions only fire on the press edge.
#[derive(Debug)]
pub struct NavigationInput {
    held: GamepadState,
    device: InputDevice,
    time: f64,
    next_repeat: Option<f64>,
    repeat_delay: Duration,
    repeat_interval: Duration,
}

impl Default for NavigationInput {
    fn default() -> Self {
        Self {
            held: GamepadState::default(),
            device: InputDevice::default(),
            time: 0.0,
            next_repeat: None,
            repeat_delay: Duration::from_millis(350),
            repeat_interval: Duration::from_millis(90),
        }
    }
}

impl NavigationInput {
    pub fn repeat_timing(mut self, delay: Duration, interval: Duration) -> Self {
        self.repeat_delay = delay;
        self.repeat_interval = interval.max(Duration::from_millis(1));
        self
    }

    /// Use this to choose localized key/button hints. Bindings remain host-owned.
    pub fn device(&self) -> InputDevice {
        self.device
    }

    pub fn apply(&mut self, ctx: &Context, raw: &mut RawInput, state: GamepadState) {
        self.time = raw
            .time
            .filter(|time| time.is_finite())
            .unwrap_or(self.time + f64::from(raw.predicted_dt.max(0.0)))
            .max(self.time);
        if raw.events.iter().any(|event| {
            matches!(
                event,
                Event::Key { pressed: true, .. }
                    | Event::Text(_)
                    | Event::PointerMoved(_)
                    | Event::PointerButton { pressed: true, .. }
                    | Event::MouseWheel { .. }
            )
        }) {
            self.device = InputDevice::KeyboardMouse;
        }
        if !raw.focused {
            // Holding A while returning to the window must not activate a button.
            self.held = state;
            self.next_repeat = None;
            return;
        }

        let has_focus = ctx.memory(|m| m.focused().is_some());
        let changed_direction = state.direction != self.held.direction;
        let repeating = self.next_repeat.is_some_and(|next| self.time >= next);
        if let Some(direction) = state.direction {
            if changed_direction || repeating {
                let key = if has_focus { direction.key() } else { Key::Tab };
                self.pulse(ctx, raw, key, Modifiers::NONE, !changed_direction);
                self.next_repeat = Some(
                    self.time
                        + if changed_direction {
                            self.repeat_delay
                        } else {
                            self.repeat_interval
                        }
                        .as_secs_f64(),
                );
            }
        } else {
            self.next_repeat = None;
        }
        if state.confirm && !self.held.confirm {
            let key = if has_focus { Key::Enter } else { Key::Tab };
            self.pulse(ctx, raw, key, Modifiers::NONE, false);
        }
        if state.cancel && !self.held.cancel {
            self.pulse(ctx, raw, Key::Escape, Modifiers::NONE, false);
        }
        if state.next_focus && !self.held.next_focus {
            self.pulse(ctx, raw, Key::Tab, Modifiers::NONE, false);
        }
        if state.previous_focus && !self.held.previous_focus {
            self.pulse(ctx, raw, Key::Tab, Modifiers::SHIFT, false);
        }
        self.held = state;
        if let Some(next) = self.next_repeat {
            ctx.request_repaint_after(Duration::from_secs_f64((next - self.time).max(0.0)));
        }
    }

    fn pulse(
        &mut self,
        ctx: &Context,
        raw: &mut RawInput,
        key: Key,
        modifiers: Modifiers,
        repeat: bool,
    ) {
        // A synthetic release must not release a real, held keyboard key.
        if ctx.input(|i| i.key_down(key))
            || raw
                .events
                .iter()
                .any(|event| matches!(event, Event::Key { key: k, .. } if *k == key))
        {
            return;
        }
        for pressed in [true, false] {
            raw.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: repeat && pressed,
                modifiers,
            });
        }
        self.device = InputDevice::Gamepad;
        ctx.request_repaint();
    }
}
