use std::time::Duration;

use egui::{Context, Event, Key, Modifiers, RawInput};

use crate::primitives::focus::engagement::{self, ControllerAction, Forward, Route};

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
    /// Sequential controller navigation, normally bound to RB/LB. Engagement
    /// routes this within its regions; otherwise it becomes native Tab.
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
        engagement::prepare_input(ctx, raw.viewport_id);
        if raw.events.iter().any(engagement::native_activity) {
            self.device = InputDevice::KeyboardMouse;
            // Real input wins this frame. A held stick must be released or
            // changed before it can take over from keyboard/mouse again.
            self.held = state;
            self.next_repeat = None;
            return;
        }
        if !raw.focused {
            // Holding A while returning to the window must not activate a button.
            self.held = state;
            self.next_repeat = None;
            return;
        }

        let changed_direction = state.direction != self.held.direction;
        let repeating = self.next_repeat.is_some_and(|next| self.time >= next);
        if let Some(direction) = state.direction {
            if changed_direction || repeating {
                self.action(
                    ctx,
                    raw,
                    ControllerAction::Direction(direction, !changed_direction),
                );
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
            self.action(ctx, raw, ControllerAction::Confirm);
        }
        if state.cancel && !self.held.cancel {
            self.action(ctx, raw, ControllerAction::Cancel);
        }
        if state.next_focus && !self.held.next_focus {
            self.action(ctx, raw, ControllerAction::Next(true));
        }
        if state.previous_focus && !self.held.previous_focus {
            self.action(ctx, raw, ControllerAction::Next(false));
        }
        self.held = state;
        if let Some(next) = self.next_repeat {
            ctx.request_repaint_after(Duration::from_secs_f64((next - self.time).max(0.0)));
        }
    }

    fn action(&mut self, ctx: &Context, raw: &mut RawInput, action: ControllerAction) {
        let forward = match engagement::route(ctx, raw.viewport_id, action) {
            Some(Route::Handled) => {
                self.device = InputDevice::Gamepad;
                ctx.request_repaint();
                return;
            }
            Some(Route::Forward(forward)) => Some(forward),
            None => None,
        };
        let (key, modifiers, repeat) = if let Some(forward) = forward {
            (forward.key, Modifiers::NONE, forward.repeat)
        } else {
            let focused = ctx.memory(|memory| memory.focused().is_some());
            match action {
                ControllerAction::Direction(direction, repeat) => (
                    if focused { direction.key() } else { Key::Tab },
                    Modifiers::NONE,
                    repeat,
                ),
                ControllerAction::Confirm => (
                    if focused { Key::Enter } else { Key::Tab },
                    Modifiers::NONE,
                    false,
                ),
                ControllerAction::Cancel => (Key::Escape, Modifiers::NONE, false),
                ControllerAction::Next(next) => (
                    Key::Tab,
                    if next {
                        Modifiers::NONE
                    } else {
                        Modifiers::SHIFT
                    },
                    false,
                ),
            }
        };
        self.pulse(ctx, raw, key, modifiers, repeat, forward);
    }

    fn pulse(
        &mut self,
        ctx: &Context,
        raw: &mut RawInput,
        key: Key,
        modifiers: Modifiers,
        repeat: bool,
        forward: Option<Forward>,
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
        let start = raw.events.len();
        for pressed in [true, false] {
            raw.events.push(Event::Key {
                key,
                physical_key: None,
                pressed,
                repeat: repeat && pressed,
                modifiers,
            });
        }
        engagement::record_pulse(ctx, raw.viewport_id, start, &raw.events[start..], forward);
        self.device = InputDevice::Gamepad;
        ctx.request_repaint();
    }
}

/// Consume an unmodified Escape press after child content has handled its input.
/// Repeats are discarded, so holding Escape cannot dismiss multiple containers.
/// The caller decides whether this means leaving an editor, going back or closing.
pub fn consume_escape(ctx: &Context) -> bool {
    consume_press(ctx, &[Key::Escape])
}

pub(crate) fn consume_press(ctx: &Context, keys: &[Key]) -> bool {
    ctx.input_mut(|input| {
        let mut pressed = false;
        input.events.retain(|event| {
            if let egui::Event::Key {
                key,
                pressed: true,
                repeat,
                modifiers: Modifiers::NONE,
                ..
            } = event
                && keys.contains(key)
            {
                pressed |= !repeat;
                false
            } else {
                true
            }
        });
        pressed
    })
}

pub(crate) fn discard_escape_repeats(ctx: &Context) {
    discard_repeats(ctx, &[Key::Escape]);
}

pub(crate) fn discard_repeats(ctx: &Context, keys: &[Key]) {
    ctx.input_mut(|input| {
        input.events.retain(|event| {
            !matches!(
                event,
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: true,
                    modifiers: Modifiers::NONE,
                    ..
                } if keys.contains(key)
            )
        });
    });
}
