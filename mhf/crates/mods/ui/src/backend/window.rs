use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::Ime::ISC_SHOWUIALL;
use windows::Win32::UI::Input::KeyboardAndMouse::GetFocus;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_OWNDC, CS_VREDRAW, CallWindowProcW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GWLP_WNDPROC, GetCursorPos, GetWindowThreadProcessId, PostMessageW,
    RegisterClassExW, SendMessageW, SetWindowLongPtrW, UnregisterClassW, WINDOW_EX_STYLE, WM_CHAR,
    WM_IME_SETCONTEXT, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN,
    WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL, WM_MOUSEMOVE,
    WM_MOUSEWHEEL, WM_NCDESTROY, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_OVERLAPPED,
    XBUTTON1,
};
use windows::core::w;

use crate::backend::ime::{Ime, control_message};
use crate::backend::input::{
    InputState, WM_MOUSELEAVE, is_keyboard_message, is_pointer_message, scan_code,
};
use crate::backend::{Error, HostIme, InputCaptureState, InputPolicy, Result};

static WINDOW_ROUTE: Mutex<Option<WindowRoute>> = Mutex::new(None);

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
#[cfg(target_pointer_width = "32")]
type WindowLong = i32;
#[cfg(target_pointer_width = "64")]
type WindowLong = isize;

pub(super) struct WindowState {
    hwnd: usize,
    input: Mutex<InputState>,
    capture: InputCaptureState,
    ime: Ime,
}

impl WindowState {
    pub(super) fn new(
        hwnd: HWND,
        capture: InputCaptureState,
        host: Option<Arc<dyn HostIme>>,
    ) -> Self {
        capture.lock().window = hwnd.0 as usize;
        Self {
            hwnd: hwnd.0 as usize,
            input: Mutex::new(InputState::new()),
            capture,
            ime: Ime::new(host),
        }
    }

    pub(super) fn take_input(&self, pixels_per_point: f32) -> Result<egui::RawInput> {
        self.input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take(HWND(self.hwnd as *mut _), pixels_per_point)
    }

    pub(super) fn update_capture(&self, policy: InputPolicy, context: &egui::Context) {
        self.capture.update(policy, context);
    }

    pub(super) fn update_ime(&self, context: &egui::Context, output: &egui::PlatformOutput) {
        let block_host = self.capture.captures_keyboard();
        let ime = output.ime.filter(|_| context.input(|input| input.focused));
        self.ime.update(
            HWND(self.hwnd as *mut _),
            context.memory(|memory| memory.focused()),
            ime,
            context.pixels_per_point(),
            block_host,
        );
    }

    pub(super) fn clear_capture(&self) {
        self.capture.clear();
        self.ime
            .update(HWND(self.hwnd as *mut _), None, None, 1.0, false);
    }

    fn ime_events(&self, events: Vec<egui::ImeEvent>) {
        if !events.is_empty() {
            self.input
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .ime_events(events);
        }
    }

    fn finish_ime_sync(
        &self,
        hwnd: HWND,
        original: WindowProc,
        update: (Vec<egui::ImeEvent>, bool),
    ) {
        let (events, restored) = update;
        self.ime_events(events);
        if restored && unsafe { GetFocus() } == hwnd {
            unsafe {
                CallWindowProcW(
                    Some(original),
                    hwnd,
                    WM_IME_SETCONTEXT,
                    WPARAM(1),
                    LPARAM(ISC_SHOWUIALL as isize),
                );
            }
        }
    }

    fn handle_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        // Navigation/confirm keys belong to the IME during composition. Keep
        // recording their host/overlay ownership below, but don't edit egui text.
        let composing_key = self.ime.composing() && is_keyboard_message(message);
        if !composing_key {
            self.input
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .handle_message(message, wparam, lparam);
        }
        let mut capture = self.capture.lock();
        if is_pointer_message(message) {
            let mut position = POINT {
                x: lparam.0 as i16 as i32,
                y: (lparam.0 >> 16) as i16 as i32,
            };
            let has_position = if matches!(message, WM_MOUSEWHEEL | WM_MOUSEHWHEEL) {
                unsafe { ScreenToClient(HWND(capture.window as *mut _), &mut position).as_bool() }
            } else {
                message != WM_MOUSELEAVE
            };
            if has_position {
                capture.move_pointer(egui::pos2(position.x as f32, position.y as f32));
            }
        }
        let captured = match message {
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => capture.pointer_button(0, true),
            WM_LBUTTONUP => capture.pointer_button(0, false),
            WM_RBUTTONDOWN | WM_RBUTTONDBLCLK => capture.pointer_button(1, true),
            WM_RBUTTONUP => capture.pointer_button(1, false),
            WM_MBUTTONDOWN | WM_MBUTTONDBLCLK => capture.pointer_button(2, true),
            WM_MBUTTONUP => capture.pointer_button(2, false),
            WM_XBUTTONDOWN | WM_XBUTTONDBLCLK | WM_XBUTTONUP => {
                let button = if (wparam.0 >> 16) as u16 == XBUTTON1 {
                    3
                } else {
                    4
                };
                capture.pointer_button(button, message != WM_XBUTTONUP)
            }
            WM_MOUSEMOVE => capture.pointer_motion(),
            WM_KEYDOWN | WM_SYSKEYDOWN => capture.key(scan_code(wparam, lparam), true),
            WM_KEYUP | WM_SYSKEYUP => capture.key(scan_code(wparam, lparam), false),
            WM_KILLFOCUS => {
                capture.reset();
                false
            }
            _ => {
                (is_pointer_message(message) && capture.pointer())
                    || (is_keyboard_message(message) && capture.keyboard())
            }
        };
        // A host editor consumes IME navigation too, even when the caller lets
        // ordinary game keyboard input through its global capture policy.
        captured || composing_key
    }
}

struct WindowRoute {
    hwnd: usize,
    original: WindowProc,
    state: Arc<WindowState>,
}

pub(super) struct WindowBinding {
    hwnd: usize,
}

impl WindowBinding {
    pub(super) fn install(hwnd: HWND, state: Arc<WindowState>) -> Result<Self> {
        if hwnd.is_invalid() {
            return Err(Error::new("D3D9 device has no valid focus window"));
        }
        let message = control_message()?;

        // Keep the route locked until both the procedure and its forwarding
        // state are installed, so a concurrent message cannot observe half of
        // the transition.
        let mut route = window_route();
        if route.is_some() {
            return Err(Error::new(
                "an overlay window procedure is already installed",
            ));
        }

        let previous = unsafe { replace_window_proc(hwnd) }
            .ok_or_else(|| Error::new("SetWindowLongPtrW(GWLP_WNDPROC) returned null"))?;
        let hwnd_raw = hwnd.0 as usize;

        *route = Some(WindowRoute {
            hwnd: hwnd_raw,
            original: previous,
            state,
        });
        drop(route);

        // Apply initial IME ownership before returning on the window thread.
        // A rendering thread must dispatch initialization to that owning thread.
        unsafe {
            if GetWindowThreadProcessId(hwnd, None) == GetCurrentThreadId() {
                SendMessageW(hwnd, message, Some(WPARAM(0)), Some(LPARAM(0)));
            } else {
                let _ = PostMessageW(Some(hwnd), message, WPARAM(0), LPARAM(0));
            }
        }

        Ok(Self { hwnd: hwnd_raw })
    }
}

impl Drop for WindowBinding {
    fn drop(&mut self) {
        let hwnd = HWND(self.hwnd as *mut _);
        // Finish on the window thread before the caller may unload the host
        // module containing the original procedure. Same-thread sends dispatch
        // directly; cross-thread uninstall requires that thread to pump messages.
        if let Ok(message) = control_message() {
            unsafe {
                SendMessageW(hwnd, message, Some(WPARAM(1)), Some(LPARAM(0)));
            }
        }
    }
}

pub(super) fn cursor_position(window: usize) -> Option<egui::Pos2> {
    if window == 0 {
        return None;
    }
    let mut position = POINT::default();
    unsafe { GetCursorPos(&mut position) }.ok()?;
    unsafe { ScreenToClient(HWND(window as *mut _), &mut position) }
        .as_bool()
        .then(|| egui::pos2(position.x as f32, position.y as f32))
}

unsafe extern "system" fn overlay_window_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let hwnd_raw = hwnd.0 as usize;
    let snapshot = {
        let route = window_route();
        route.as_ref().and_then(|route| {
            (route.hwnd == hwnd_raw).then(|| (route.original, Arc::clone(&route.state)))
        })
    };

    let Some((original, state)) = snapshot else {
        return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
    };

    let handled = catch_unwind(AssertUnwindSafe(|| {
        if Some(message) == control_message().ok() {
            let update = if wparam.0 == 1 {
                state.ime.stop(hwnd, true)
            } else {
                state.ime.sync(hwnd)
            };
            state.finish_ime_sync(hwnd, original, update);
            if wparam.0 == 1 {
                detach_window(hwnd, original, &state);
            }
            return Some(LRESULT(0));
        }
        if matches!(message, WM_KILLFOCUS | WM_NCDESTROY) {
            let (events, _) = state.ime.stop(hwnd, message == WM_NCDESTROY);
            state.ime_events(events);
            if message == WM_NCDESTROY {
                detach_window(hwnd, original, &state);
            }
        } else {
            state.finish_ime_sync(hwnd, original, state.ime.refresh(hwnd));
        }
        if let Some(reply) = state.ime.handle_message(message, lparam) {
            state.ime_events(reply.events);
            return Some(if reply.default_proc {
                unsafe { DefWindowProcW(hwnd, message, wparam, reply.lparam) }
            } else {
                LRESULT(0)
            });
        }
        if message == WM_CHAR && state.ime.handle_character(wparam.0) {
            // Stay on the Unicode side of CallWindowProcW's ANSI thunk. The
            // native editor otherwise truncates wParam to one byte at 114D3CCB.
            return Some(LRESULT(0));
        }
        state
            .handle_message(message, wparam, lparam)
            .then_some(LRESULT(1))
    }))
    .unwrap_or_else(|_| {
        state.clear_capture();
        None
    });

    handled.unwrap_or_else(|| {
        let result = unsafe { CallWindowProcW(Some(original), hwnd, message, wparam, lparam) };
        // A native click/key may open or close an editor in the host procedure.
        // Apply that change before the next keystroke reaches the IME.
        if !matches!(message, WM_KILLFOCUS | WM_NCDESTROY)
            && catch_unwind(AssertUnwindSafe(|| {
                state.finish_ime_sync(hwnd, original, state.ime.refresh(hwnd));
            }))
            .is_err()
        {
            state.clear_capture();
        }
        result
    })
}

fn detach_window(hwnd: HWND, original: WindowProc, state: &Arc<WindowState>) {
    unsafe { restore_window_proc(hwnd, original) };
    state.capture.reset();
    let mut route = window_route();
    if route
        .as_ref()
        .is_some_and(|route| Arc::ptr_eq(&route.state, state))
    {
        *route = None;
    }
}

unsafe fn replace_window_proc(hwnd: HWND) -> Option<WindowProc> {
    let replacement = overlay_window_proc as *const () as usize as WindowLong;
    let previous = unsafe { SetWindowLongPtrW(hwnd, GWLP_WNDPROC, replacement) };
    if previous == 0 {
        None
    } else {
        Some(unsafe { mem::transmute::<WindowLong, WindowProc>(previous) })
    }
}

unsafe fn restore_window_proc(hwnd: HWND, original: WindowProc) {
    let original = original as *const () as usize as WindowLong;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_WNDPROC, original);
    }
}

fn window_route() -> MutexGuard<'static, Option<WindowRoute>> {
    WINDOW_ROUTE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(super) struct DummyWindow {
    hwnd: HWND,
    instance: HINSTANCE,
}

impl DummyWindow {
    pub(super) fn create() -> Result<Self> {
        unsafe extern "system" fn dummy_window_proc(
            hwnd: HWND,
            message: u32,
            wparam: WPARAM,
            lparam: LPARAM,
        ) -> LRESULT {
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }

        let instance: HINSTANCE = unsafe { GetModuleHandleW(None) }
            .map_err(|error| Error::new(format!("GetModuleHandleW failed: {error}")))?
            .into();
        let class = WNDCLASSEXW {
            cbSize: mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW | CS_OWNDC,
            lpfnWndProc: Some(dummy_window_proc),
            hInstance: instance,
            lpszClassName: w!("ShrimpmanOverlayD3d9Dummy"),
            ..WNDCLASSEXW::default()
        };

        if unsafe { RegisterClassExW(&class) } == 0 {
            return Err(Error::new(format!(
                "RegisterClassExW failed: {}",
                windows::core::Error::from_thread()
            )));
        }

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class.lpszClassName,
                w!("Shrimpman D3D9 hook discovery"),
                WS_OVERLAPPED,
                0,
                0,
                100,
                100,
                None,
                None,
                Some(instance),
                None,
            )
        };

        match hwnd {
            Ok(hwnd) => Ok(Self { hwnd, instance }),
            Err(error) => {
                unsafe {
                    let _ = UnregisterClassW(class.lpszClassName, Some(instance));
                }
                Err(Error::new(format!("CreateWindowExW failed: {error}")))
            }
        }
    }

    pub(super) fn hwnd(&self) -> HWND {
        self.hwnd
    }
}

impl Drop for DummyWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.hwnd);
            let _ = UnregisterClassW(w!("ShrimpmanOverlayD3d9Dummy"), Some(self.instance));
        }
    }
}

#[cfg(test)]
mod tests;
