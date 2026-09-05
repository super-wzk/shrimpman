use std::mem;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_OWNDC, CS_VREDRAW, CallWindowProcW, CreateWindowExW, DefWindowProcW,
    DestroyWindow, GWLP_WNDPROC, RegisterClassExW, SetWindowLongPtrW, UnregisterClassW,
    WINDOW_EX_STYLE, WNDCLASSEXW, WS_OVERLAPPED,
};
use windows::core::w;

use crate::input::{InputState, is_keyboard_message, is_pointer_message};
use crate::{Error, Result};

static WINDOW_ROUTE: Mutex<Option<WindowRoute>> = Mutex::new(None);

type WindowProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
#[cfg(target_pointer_width = "32")]
type WindowLong = i32;
#[cfg(target_pointer_width = "64")]
type WindowLong = isize;

#[derive(Default)]
struct CaptureState {
    pointer: AtomicBool,
    keyboard: AtomicBool,
}

impl CaptureState {
    fn update(&self, pointer: bool, keyboard: bool) {
        self.pointer.store(pointer, Ordering::Release);
        self.keyboard.store(keyboard, Ordering::Release);
    }

    fn clear(&self) {
        self.update(false, false);
    }

    fn blocks(&self, message: u32) -> bool {
        (is_pointer_message(message) && self.pointer.load(Ordering::Acquire))
            || (is_keyboard_message(message) && self.keyboard.load(Ordering::Acquire))
    }
}

pub(super) struct WindowState {
    input: Mutex<InputState>,
    capture: CaptureState,
}

impl WindowState {
    pub(super) fn new() -> Self {
        Self {
            input: Mutex::new(InputState::new()),
            capture: CaptureState::default(),
        }
    }

    pub(super) fn take_input(&self, hwnd: HWND, pixels_per_point: f32) -> Result<egui::RawInput> {
        self.input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take(hwnd, pixels_per_point)
    }

    pub(super) fn update_capture(&self, pointer: bool, keyboard: bool) {
        self.capture.update(pointer, keyboard);
    }

    pub(super) fn clear_capture(&self) {
        self.capture.clear();
    }

    fn handle_message(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> bool {
        self.input
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .handle_message(message, wparam, lparam);
        self.capture.blocks(message)
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
        if route.as_ref().is_some_and(|route| route.hwnd == self.hwnd) {
            *route = None;
        }
    }
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
