use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_OWNDC, CS_VREDRAW, CallWindowProcW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GWLP_WNDPROC, GetCursorPos, RegisterClassExW, SetWindowLongPtrW,
    UnregisterClassW, WINDOW_EX_STYLE, WM_KEYDOWN, WM_KEYUP, WM_KILLFOCUS, WM_LBUTTONDBLCLK,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDBLCLK, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEHWHEEL,
    WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDBLCLK, WM_RBUTTONDOWN, WM_RBUTTONUP, WM_SYSKEYDOWN,
    WM_SYSKEYUP, WM_XBUTTONDBLCLK, WM_XBUTTONDOWN, WM_XBUTTONUP, WNDCLASSEXW, WS_OVERLAPPED,
    XBUTTON1,
};
use windows::core::w;

use crate::input::{InputState, WM_MOUSELEAVE, is_keyboard_message, is_pointer_message, scan_code};
use crate::{Error, InputCaptureState, InputPolicy, Result};

static WINDOW_ROUTE: Mutex<Option<WindowRoute>> = Mutex::new(None);

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
#[cfg(target_pointer_width = "32")]
type WindowLong = i32;
#[cfg(target_pointer_width = "64")]
type WindowLong = isize;

pub(super) struct WindowState {
    input: Mutex<InputState>,
    capture: InputCaptureState,
}

impl WindowState {
    pub(super) fn new(hwnd: HWND, capture: InputCaptureState) -> Self {
        capture.lock().window = hwnd.0 as usize;
        Self {
            input: Mutex::new(InputState::new()),
            capture,
        }
    }

    pub(super) fn take_input(&self, hwnd: HWND, pixels_per_point: f32) -> Result<egui::RawInput> {
        self.input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take(hwnd, pixels_per_point)
    }

    pub(super) fn update_capture(&self, policy: InputPolicy, context: &egui::Context) {
        self.capture.update(policy, context);
    }

    pub(super) fn clear_capture(&self) {
        self.capture.clear();
    }

    fn handle_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        self.input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .handle_message(message, wparam, lparam);
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
        match message {
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
        }
    }
}

struct WindowRoute {
    hwnd: usize,
    original: WindowProc,
    state: Arc<WindowState>,
}

pub(super) struct WindowBinding {
    hwnd: usize,
    original: WindowProc,
}

impl WindowBinding {
    pub(super) fn install(hwnd: HWND, state: Arc<WindowState>) -> Result<Self> {
        if hwnd.is_invalid() {
            return Err(Error::new("D3D9 device has no valid focus window"));
        }

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

        Ok(Self {
            hwnd: hwnd_raw,
            original: previous,
        })
    }
}

impl Drop for WindowBinding {
    fn drop(&mut self) {
        let hwnd = HWND(self.hwnd as *mut _);
        unsafe {
            restore_window_proc(hwnd, self.original);
        }
        let mut route = window_route();
        if let Some(current) = route.as_ref().filter(|route| route.hwnd == self.hwnd) {
            current.state.capture.reset();
            *route = None;
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

    let blocked = catch_unwind(AssertUnwindSafe(|| {
        state.handle_message(message, wparam, lparam)
    }))
    .unwrap_or_else(|_| {
        state.clear_capture();
        false
    });

    if blocked {
        LRESULT(1)
    } else {
        unsafe { CallWindowProcW(Some(original), hwnd, message, wparam, lparam) }
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
