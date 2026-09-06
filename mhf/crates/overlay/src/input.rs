use std::time::Instant;

use egui::{
    Event, Key, Modifiers, MouseWheelUnit, PointerButton, Pos2, RawInput, Rect, TouchPhase, Vec2,
    ViewportId,
};
use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VK_CONTROL, VK_LWIN, VK_MENU, VK_NUMLOCK, VK_PAUSE, VK_RWIN, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, WHEEL_DELTA, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SETFOCUS,
    WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP,
    XBUTTON1,
};

use crate::{Error, Result};

const SCROLL_POINTS_PER_NOTCH: f32 = 24.0;
pub(super) const WM_MOUSELEAVE: u32 = 0x02A3;
const NATIVE_PIXELS_PER_POINT: f32 = 1.0;

pub(super) fn scan_code(wparam: WPARAM, lparam: LPARAM) -> usize {
    match wparam.0 as u16 {
        code if code == VK_PAUSE.0 => 0xc5,
        code if code == VK_NUMLOCK.0 => 0x45,
        _ => ((lparam.0 as usize >> 16) & 0x7f) | ((lparam.0 as usize >> 17) & 0x80),
    }
}

pub(super) struct InputState {
    events: Vec<Event>,
    modifiers: Modifiers,
    focused: bool,
    pending_high_surrogate: Option<u16>,
    pixels_per_point: f32,
    started_at: Instant,
    last_frame_at: Instant,
}

impl InputState {
    pub(super) fn new() -> Self {
        let now = Instant::now();
        Self {
            events: Vec::new(),
            modifiers: Modifiers::NONE,
            focused: true,
            pending_high_surrogate: None,
            pixels_per_point: 1.0,
            started_at: now,
            last_frame_at: now,
        }
    }

    pub(super) fn handle_message(&mut self, message: u32, wparam: WPARAM, lparam: LPARAM) {
        match message {
            WM_MOUSEMOVE => {
                self.update_modifiers();
                self.events
                    .push(Event::PointerMoved(self.pointer_position(lparam)));
            }
            WM_MOUSELEAVE => self.events.push(Event::PointerGone),
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => {
                self.pointer_button(lparam, PointerButton::Primary, true);
            }
            WM_LBUTTONUP => self.pointer_button(lparam, PointerButton::Primary, false),
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => {
                self.pointer_button(lparam, PointerButton::Secondary, true);
            }
            WM_RBUTTONUP => self.pointer_button(lparam, PointerButton::Secondary, false),
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => {
                self.pointer_button(lparam, PointerButton::Middle, true);
            }
            WM_MBUTTONUP => self.pointer_button(lparam, PointerButton::Middle, false),
            WM_XBUTTONDOWN | WM_XBUTTONDBLCLK => {
                self.pointer_button(lparam, x_button(wparam), true);
            }
            WM_XBUTTONUP => self.pointer_button(lparam, x_button(wparam), false),
            WM_MOUSEWHEEL => self.mouse_wheel(wparam, false),
            WM_MOUSEHWHEEL => self.mouse_wheel(wparam, true),
            WM_KEYDOWN | WM_SYSKEYDOWN => self.key(wparam, lparam, true),
            WM_KEYUP | WM_SYSKEYUP => self.key(wparam, lparam, false),
            WM_CHAR | WM_SYSCHAR => self.character(wparam.0),
            WM_SETFOCUS => self.focus(true),
            WM_KILLFOCUS => self.focus(false),
            _ => {}
        }
    }

    pub(super) fn take(&mut self, hwnd: HWND, pixels_per_point: f32) -> Result<RawInput> {
        self.pixels_per_point = pixels_per_point.max(f32::EPSILON);

        let mut client = windows::Win32::Foundation::RECT::default();
        unsafe { GetClientRect(hwnd, &mut client) }
            .map_err(|error| Error::new(format!("GetClientRect failed: {error}")))?;

        let size = Vec2::new(
            (client.right - client.left).max(0) as f32 / self.pixels_per_point,
            (client.bottom - client.top).max(0) as f32 / self.pixels_per_point,
        );
        let screen_rect = Rect::from_min_size(Pos2::ZERO, size);
        let now = Instant::now();
        let predicted_dt = (now - self.last_frame_at).as_secs_f32().max(f32::EPSILON);
        self.last_frame_at = now;

        let mut input = RawInput {
            screen_rect: Some(screen_rect),
            time: Some((now - self.started_at).as_secs_f64()),
            predicted_dt,
            events: std::mem::take(&mut self.events),
            focused: self.focused,
            ..Default::default()
        };
        let viewport = input
            .viewports
            .get_mut(&ViewportId::ROOT)
            .expect("root egui viewport must exist");
        // The legacy host renders in its virtualized client-pixel coordinate
        // system. Egui's zoom factor is already included in `pixels_per_point`.
        viewport.native_pixels_per_point = Some(NATIVE_PIXELS_PER_POINT);
        viewport.inner_rect = Some(screen_rect);
        viewport.focused = Some(self.focused);

        Ok(input)
    }

    fn pointer_button(&mut self, lparam: LPARAM, button: PointerButton, pressed: bool) {
        self.update_modifiers();
        self.events.push(Event::PointerButton {
            pos: self.pointer_position(lparam),
            button,
            pressed,
            modifiers: self.modifiers,
        });
    }

    fn pointer_position(&self, lparam: LPARAM) -> Pos2 {
        let packed = lparam.0.cast_unsigned();
        Pos2::new(
            f32::from(low_word_signed(packed)) / self.pixels_per_point,
            f32::from(high_word_signed(packed)) / self.pixels_per_point,
        )
    }

    fn mouse_wheel(&mut self, wparam: WPARAM, horizontal: bool) {
        self.update_modifiers();
        let notches = f32::from(high_word_signed(wparam.0)) / WHEEL_DELTA as f32;
        let delta = if horizontal {
            Vec2::new(notches * SCROLL_POINTS_PER_NOTCH, 0.0)
        } else {
            Vec2::new(0.0, notches * SCROLL_POINTS_PER_NOTCH)
        };
        self.events.push(Event::MouseWheel {
            unit: MouseWheelUnit::Point,
            delta,
            phase: TouchPhase::Move,
            modifiers: self.modifiers,
        });
    }

    fn key(&mut self, wparam: WPARAM, lparam: LPARAM, pressed: bool) {
        self.update_modifiers();
        let Ok(key_code) = u16::try_from(wparam.0) else {
            return;
        };
        let Some(key) = virtual_key(key_code) else {
            return;
        };
        self.events.push(Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: pressed && (lparam.0.cast_unsigned() & (1 << 30)) != 0,
            modifiers: self.modifiers,
        });
    }

    fn character(&mut self, code_unit: usize) {
        let Ok(code_unit) = u16::try_from(code_unit) else {
            return;
        };

        if (0xD800..=0xDBFF).contains(&code_unit) {
            self.pending_high_surrogate = Some(code_unit);
            return;
        }

        let character = if (0xDC00..=0xDFFF).contains(&code_unit) {
            let Some(high) = self.pending_high_surrogate.take() else {
                return;
            };
            char::decode_utf16([high, code_unit])
                .next()
                .and_then(|result| result.ok())
        } else {
            self.pending_high_surrogate = None;
            char::from_u32(u32::from(code_unit))
        };

        if let Some(character) = character.filter(|character| !character.is_control()) {
            self.events.push(Event::Text(character.to_string()));
        }
    }

    fn focus(&mut self, focused: bool) {
        self.focused = focused;
        self.events.push(Event::WindowFocused(focused));
        if !focused {
            self.events.push(Event::PointerGone);
            self.pending_high_surrogate = None;
        }
    }

    fn update_modifiers(&mut self) {
        let modifiers = modifiers();
        if modifiers != self.modifiers {
            self.modifiers = modifiers;
            self.events.push(Event::ModifiersChanged(modifiers));
        }
    }
}

pub(super) fn is_pointer_message(message: u32) -> bool {
    matches!(
        message,
        WM_MOUSEMOVE
            | WM_MOUSELEAVE
            | WM_LBUTTONDOWN
            | WM_LBUTTONUP
            | WM_LBUTTONDBLCLK
            | WM_RBUTTONDOWN
            | WM_RBUTTONUP
            | WM_RBUTTONDBLCLK
            | WM_MBUTTONDOWN
            | WM_MBUTTONUP
            | WM_MBUTTONDBLCLK
            | WM_XBUTTONDOWN
            | WM_XBUTTONUP
            | WM_XBUTTONDBLCLK
            | WM_MOUSEWHEEL
            | WM_MOUSEHWHEEL
    )
}

pub(super) fn is_keyboard_message(message: u32) -> bool {
    matches!(
        message,
        WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR | WM_SYSCHAR
    )
}

fn modifiers() -> Modifiers {
    let ctrl = key_is_down(VK_CONTROL.0);
    let shift = key_is_down(VK_SHIFT.0);
    let alt = key_is_down(VK_MENU.0);
    let command = ctrl;
    Modifiers {
        alt,
        ctrl,
        shift,
        mac_cmd: false,
        command,
    }
}

fn key_is_down(key: u16) -> bool {
    unsafe { GetKeyState(i32::from(key)) < 0 }
}

fn x_button(wparam: WPARAM) -> PointerButton {
    if high_word(wparam.0) == XBUTTON1 {
        PointerButton::Extra1
    } else {
        PointerButton::Extra2
    }
}

fn virtual_key(key: u16) -> Option<Key> {
    Some(match key {
        0x08 => Key::Backspace,
        0x09 => Key::Tab,
        0x0D => Key::Enter,
        0x1B => Key::Escape,
        0x20 => Key::Space,
        0x21 => Key::PageUp,
        0x22 => Key::PageDown,
        0x23 => Key::End,
        0x24 => Key::Home,
        0x25 => Key::ArrowLeft,
        0x26 => Key::ArrowUp,
        0x27 => Key::ArrowRight,
        0x28 => Key::ArrowDown,
        0x2D => Key::Insert,
        0x2E => Key::Delete,
        0x30 | 0x60 => Key::Num0,
        0x31 | 0x61 => Key::Num1,
        0x32 | 0x62 => Key::Num2,
        0x33 | 0x63 => Key::Num3,
        0x34 | 0x64 => Key::Num4,
        0x35 | 0x65 => Key::Num5,
        0x36 | 0x66 => Key::Num6,
        0x37 | 0x67 => Key::Num7,
        0x38 | 0x68 => Key::Num8,
        0x39 | 0x69 => Key::Num9,
        0x41 => Key::A,
        0x42 => Key::B,
        0x43 => Key::C,
        0x44 => Key::D,
        0x45 => Key::E,
        0x46 => Key::F,
        0x47 => Key::G,
        0x48 => Key::H,
        0x49 => Key::I,
        0x4A => Key::J,
        0x4B => Key::K,
        0x4C => Key::L,
        0x4D => Key::M,
        0x4E => Key::N,
        0x4F => Key::O,
        0x50 => Key::P,
        0x51 => Key::Q,
        0x52 => Key::R,
        0x53 => Key::S,
        0x54 => Key::T,
        0x55 => Key::U,
        0x56 => Key::V,
        0x57 => Key::W,
        0x58 => Key::X,
        0x59 => Key::Y,
        0x5A => Key::Z,
        0x70 => Key::F1,
        0x71 => Key::F2,
        0x72 => Key::F3,
        0x73 => Key::F4,
        0x74 => Key::F5,
        0x75 => Key::F6,
        0x76 => Key::F7,
        0x77 => Key::F8,
        0x78 => Key::F9,
        0x79 => Key::F10,
        0x7A => Key::F11,
        0x7B => Key::F12,
        0x6B => Key::Plus,
        0x6D | 0xBD => Key::Minus,
        0x6E | 0xBE => Key::Period,
        0x6F | 0xBF => Key::Slash,
        0xBA => Key::Semicolon,
        0xBB => Key::Equals,
        0xBC => Key::Comma,
        0xC0 => Key::Backtick,
        0xDB => Key::OpenBracket,
        0xDC => Key::Backslash,
        0xDD => Key::CloseBracket,
        0xDE => Key::Quote,
        key if key == VK_LWIN.0 => Key::SuperLeft,
        key if key == VK_RWIN.0 => Key::SuperRight,
        _ => return None,
    })
}

fn low_word_signed(value: usize) -> i16 {
    ((value & 0xFFFF) as u16).cast_signed()
}

fn high_word_signed(value: usize) -> i16 {
    high_word(value).cast_signed()
}

fn high_word(value: usize) -> u16 {
    ((value >> 16) & 0xFFFF) as u16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_navigation_and_ascii_keys() {
        assert_eq!(virtual_key(0x25), Some(Key::ArrowLeft));
        assert_eq!(virtual_key(u16::from(b'R')), Some(Key::R));
        assert_eq!(virtual_key(u16::from(b'7')), Some(Key::Num7));
    }

    #[test]
    fn extracts_signed_pointer_coordinates() {
        let packed = usize::from(20_u16) << 16 | usize::from((-10_i16).cast_unsigned());
        assert_eq!(low_word_signed(packed), -10);
        assert_eq!(high_word_signed(packed), 20);
    }

    #[test]
    fn normalizes_native_keys_to_directinput_scan_codes() {
        assert_eq!(scan_code(WPARAM(0x41), LPARAM(0x001e_0001)), 0x1e);
        assert_eq!(scan_code(WPARAM(0x11), LPARAM(0x001d_0001)), 0x1d);
        assert_eq!(scan_code(WPARAM(0x11), LPARAM(0x011d_0001)), 0x9d);
        assert_eq!(scan_code(WPARAM(0x26), LPARAM(0x0148_0001)), 0xc8);
        assert_eq!(
            scan_code(WPARAM(VK_NUMLOCK.0.into()), LPARAM(0x0145_0001)),
            0x45
        );
        assert_eq!(
            scan_code(WPARAM(VK_PAUSE.0.into()), LPARAM(0x0045_0001)),
            0xc5
        );
    }
}
