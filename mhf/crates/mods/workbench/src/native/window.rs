//! Workbench window controls and resizing through the game's device lifecycle.

mod presentation;

use super::{BASE, Client, SLOT, State, put};
use crate::preview::Command;
use mhf_hooks::HookSet;
use std::{
    ffi::c_void,
    mem::transmute,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{
            ERROR_SUCCESS, GetLastError, HWND, LPARAM, LRESULT, RECT, SetLastError, WPARAM,
        },
        Graphics::Direct3D9::{D3DBACKBUFFER_TYPE_MONO, D3DSURFACE_DESC, IDirect3DDevice9},
        UI::WindowsAndMessaging::{
            DrawMenuBar, GWL_STYLE, GetClientRect, GetSystemMenu, GetWindowLongW, IsIconic,
            IsWindow, PostMessageW, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SWP_NOZORDER, SetWindowLongW, SetWindowPos, WM_CLOSE, WM_ENTERSIZEMOVE,
            WM_EXITSIZEMOVE, WM_NULL, WM_SIZE, WM_SYSKEYDOWN, WS_MAXIMIZEBOX, WS_POPUP, WS_SYSMENU,
            WS_THICKFRAME,
        },
    },
    core::Interface,
};

const RESTRICT: usize = 0x114d_5660;
const WNDPROC: usize = 0x114d_5720;
const POLL: usize = 0x114d_6580;
const RESET: usize = 0x114d_6e20;
const WINDOW: usize = 0x1e81_1a38;
// Created by 1000A160 during native initialization; Reset dereferences it
// unconditionally in 114D3250 before releasing the device resources.
const FONT_OBJECT: usize = 0x11b8_04a8;
const CLOSE_ALLOWED: usize = (-999_i32) as usize;
const DEVICE_LOST: i32 = 0x8876_0868_u32 as i32;
const WINDOW_CONTROLS: u32 = WS_THICKFRAME.0 | WS_MAXIMIZEBOX.0 | WS_SYSMENU.0;

type Restrict = unsafe extern "C" fn(HWND) -> i32;
type WndProc = unsafe extern "system" fn(HWND, u32, WPARAM, LPARAM) -> LRESULT;
type Poll = unsafe extern "C" fn() -> i32;
type Reset = unsafe extern "C" fn(i32) -> i32;

#[derive(Clone, Copy)]
struct WindowStyle {
    hwnd: usize,
    added: u32,
}

#[derive(Default)]
pub(super) struct Hooks {
    wndproc: usize,
    poll: usize,
    reset: usize,
    style: Mutex<Option<WindowStyle>>,
    presentation: Option<presentation::ResetPresentation>,
    sizing: AtomicBool,
    resize_pending: AtomicBool,
    reset_failed: AtomicBool,
}

impl Hooks {
    unsafe fn configure(&self, hwnd: HWND) -> Result<(), String> {
        let style = unsafe { window_style(hwnd) }?;
        // Fullscreen transitions send WM_SIZE before updating the native mode
        // global. The actual window style is authoritative during that interval.
        if style & WS_POPUP.0 != 0 {
            return Ok(());
        }
        let first = {
            let mut saved = self.style.lock().unwrap_or_else(PoisonError::into_inner);
            if saved.is_some_and(|saved| saved.hwnd == hwnd.0 as usize) {
                false
            } else {
                *saved = Some(WindowStyle {
                    hwnd: hwnd.0 as usize,
                    added: WINDOW_CONTROLS & !style,
                });
                true
            }
        };
        let desired = style | WINDOW_CONTROLS;
        if desired != style {
            unsafe { set_window_style(hwnd, desired) }?;
        }
        if first {
            // The native WM_CREATE may have already deleted these commands.
            unsafe { GetSystemMenu(hwnd, true) };
        }
        if desired != style || first {
            unsafe {
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
                )
            }
            .map_err(|error| format!("无法更新工作台窗口边框：{error}"))?;
            unsafe { DrawMenuBar(hwnd) }
                .map_err(|error| format!("无法更新工作台窗口菜单：{error}"))?;
        }
        Ok(())
    }

    /// Called only after the hook group is disabled and all callbacks drain.
    pub(super) unsafe fn restore(&mut self, client: Client) -> Result<(), String> {
        if let Some(patch) = self.presentation.as_mut() {
            patch.restore()?;
        }
        self.presentation = None;
        let style = self.style.get_mut().unwrap_or_else(PoisonError::into_inner);
        let Some(saved) = *style else {
            return Ok(());
        };
        let hwnd = HWND(saved.hwnd as *mut c_void);
        if unsafe { IsWindow(Some(hwnd)) }.as_bool()
            && unsafe { client.read::<usize>(WINDOW) } == saved.hwnd
        {
            let style = unsafe { window_style(hwnd) }?;
            unsafe { set_window_style(hwnd, style & !saved.added) }?;
            // Trampolines have already been removed; call the unhooked target.
            let restrict: Restrict = unsafe { transmute(client.address(RESTRICT)) };
            unsafe {
                restrict(hwnd);
                SetWindowPos(
                    hwnd,
                    None,
                    0,
                    0,
                    0,
                    0,
                    SWP_FRAMECHANGED | SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER,
                )
            }
            .map_err(|error| format!("无法恢复游戏窗口边框：{error}"))?;
            unsafe { DrawMenuBar(hwnd) }
                .map_err(|error| format!("无法恢复游戏窗口菜单：{error}"))?;
        }
        *style = None;
        Ok(())
    }
}

pub(super) unsafe fn prepare(client: Client, hooks: &mut HookSet<State>) -> Result<Hooks, String> {
    for (address, expected) in [
        (
            RESTRICT,
            &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x08, 0x56, 0x57][..],
        ),
        (WNDPROC, &[0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x51][..]),
        // The first instruction contains the relocated device-global address.
        (
            POLL + 5,
            &[0x8b, 0x08, 0x8b, 0x51, 0x0c, 0x56, 0x50, 0x33, 0xf6][..],
        ),
        (
            RESET,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x0c, 0x0b, 0x00, 0x00][..],
        ),
    ] {
        if unsafe {
            std::slice::from_raw_parts(client.address(address) as *const u8, expected.len())
        } != expected
        {
            return Err(format!("不支持此游戏 DLL 的工作台窗口接口：{address:#x}"));
        }
    }
    let mut original = [0; 4];
    for (slot, (name, target, detour)) in original.iter_mut().zip([
        (
            "workbench window controls",
            RESTRICT,
            restrict as *mut c_void,
        ),
        ("workbench window messages", WNDPROC, wndproc as *mut c_void),
        ("workbench window resize", POLL, poll as *mut c_void),
        ("workbench device dimensions", RESET, reset as *mut c_void),
    ]) {
        *slot = unsafe { hooks.create(name, client.address(target) as _, detour) }? as usize;
    }
    let [_, wndproc, poll, reset] = original;
    Ok(Hooks {
        wndproc,
        poll,
        reset,
        presentation: Some(unsafe { presentation::ResetPresentation::install(client)? }),
        resize_pending: AtomicBool::new(true),
        ..Hooks::default()
    })
}

pub(super) unsafe fn activate(client: Client) {
    let hwnd = HWND(unsafe { client.read::<usize>(WINDOW) } as *mut c_void);
    if !hwnd.is_invalid()
        && let Err(error) = unsafe { PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0)) }
    {
        eprintln!("workbench: could not initialize window controls: {error}");
    }
}

unsafe fn window_style(hwnd: HWND) -> Result<u32, String> {
    unsafe { SetLastError(ERROR_SUCCESS) };
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) };
    if style == 0 && unsafe { GetLastError() } != ERROR_SUCCESS {
        return Err(format!(
            "无法读取工作台窗口样式：{}",
            windows::core::Error::from_thread()
        ));
    }
    Ok(style as u32)
}

unsafe fn set_window_style(hwnd: HWND, style: u32) -> Result<(), String> {
    unsafe { SetLastError(ERROR_SUCCESS) };
    if unsafe { SetWindowLongW(hwnd, GWL_STYLE, style as i32) } == 0
        && unsafe { GetLastError() } != ERROR_SUCCESS
    {
        return Err(format!(
            "无法设置工作台窗口样式：{}",
            windows::core::Error::from_thread()
        ));
    }
    Ok(())
}

unsafe fn client_size(client: Client) -> Option<(u32, u32)> {
    let hwnd = HWND(unsafe { client.read::<usize>(WINDOW) } as *mut c_void);
    let mut rect = RECT::default();
    if hwnd.is_invalid()
        || unsafe { IsIconic(hwnd) }.as_bool()
        || unsafe { GetClientRect(hwnd, &mut rect) }.is_err()
        || rect.right <= rect.left
        || rect.bottom <= rect.top
    {
        return None;
    }
    Some((
        (rect.right - rect.left) as u32,
        (rect.bottom - rect.top) as u32,
    ))
}

unsafe fn backbuffer_size(client: Client) -> Option<(u32, u32)> {
    let pointer = unsafe { client.read::<*mut c_void>(0x1e81_1a3c) };
    let device = unsafe { IDirect3DDevice9::from_raw_borrowed(&pointer) }?;
    let backbuffer = unsafe { device.GetBackBuffer(0, 0, D3DBACKBUFFER_TYPE_MONO) }.ok()?;
    let mut description = D3DSURFACE_DESC::default();
    unsafe { backbuffer.GetDesc(&mut description) }.ok()?;
    // The surface reference must drop before the game's Reset releases targets.
    Some((description.Width, description.Height))
}

unsafe extern "C" fn restrict(hwnd: HWND) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: Restrict =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + RESTRICT - 0x1000_0000) };
        return unsafe { original(hwnd) };
    };
    if let Err(error) = unsafe { state.window.configure(hwnd) } {
        eprintln!("workbench: {error}");
    }
    0
}

unsafe extern "system" fn wndproc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: WndProc =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + WNDPROC - 0x1000_0000) };
        return unsafe { original(hwnd, message, wparam, lparam) };
    };
    match message {
        WM_NULL => {
            if let Err(error) = unsafe { state.window.configure(hwnd) } {
                eprintln!("workbench: {error}");
            }
        }
        WM_CLOSE if wparam.0 != CLOSE_ALLOWED => {
            state.window.sizing.store(false, Ordering::Release);
            // Exit has priority over queued preview work in Control::send.
            let _ = state.control.send(Command::Exit);
            return LRESULT(0);
        }
        WM_SIZE => state.window.resize_pending.store(true, Ordering::Release),
        WM_ENTERSIZEMOVE => state.window.sizing.store(true, Ordering::Release),
        WM_EXITSIZEMOVE => {
            state.window.sizing.store(false, Ordering::Release);
            state.window.resize_pending.store(true, Ordering::Release);
        }
        // The workbench UI owns Alt+Enter and enqueues one fullscreen command.
        WM_SYSKEYDOWN if wparam.0 == 13 => return LRESULT(0),
        _ => {}
    }
    let original: WndProc = unsafe { transmute(state.window.wndproc) };
    unsafe { original(hwnd, message, wparam, lparam) }
}

unsafe extern "C" fn poll() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: Poll =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + POLL - 0x1000_0000) };
        return unsafe { original() };
    };
    if state.control.closing() {
        // 1158F870 invokes this on the same thread as dispatch and checks the
        // quit flag even when we return zero. Release resources without running
        // a new frame, including while minimized, sizing, or device-lost.
        let mut runtime = state.runtime.lock().unwrap_or_else(PoisonError::into_inner);
        if let Err(error) = unsafe { super::command(state, &mut runtime, Command::Exit) } {
            if runtime.snapshot.message.as_ref() != error {
                eprintln!("workbench: {error}");
                runtime.snapshot.message = error.into();
                state.control.publish(runtime.snapshot.clone());
            }
            std::thread::sleep(std::time::Duration::from_millis(8));
        }
        return 0;
    }
    let original: Poll = unsafe { transmute(state.window.poll) };
    if state.window.sizing.load(Ordering::Acquire)
        && state.window.resize_pending.load(Ordering::Acquire)
    {
        // 1158F870 skips the entire frame when this poll returns zero. Preserve
        // the last presented image throughout the drag, then reset once at its
        // final client size. The native outer loop has no wait of its own.
        std::thread::sleep(std::time::Duration::from_millis(8));
        return 0;
    }
    let ready = unsafe { original() };
    if ready == 0 {
        return 0;
    }
    // The first polls precede font initialization. Let those frames run while
    // preserving the queued resize for a later poll.
    if unsafe { state.client.read::<usize>(FONT_OBJECT) } == 0 {
        return ready;
    }
    let pending = state.window.resize_pending.swap(false, Ordering::AcqRel);
    let failed = state.window.reset_failed.load(Ordering::Acquire);
    if !pending && !failed {
        return ready;
    }
    let Some(size) = (unsafe { client_size(state.client) }) else {
        state.window.resize_pending.store(true, Ordering::Release);
        return 0;
    };
    if !failed && unsafe { backbuffer_size(state.client) } == Some(size) {
        return ready;
    }
    // Call the hooked native reset entry to keep one dimension/lifecycle path.
    let reset: Reset = unsafe { transmute(state.client.address(RESET)) };
    if unsafe { reset(1) } < 0 { 0 } else { ready }
}

unsafe extern "C" fn reset(mode: i32) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: Reset =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + RESET - 0x1000_0000) };
        return unsafe { original(mode) };
    };
    // Native polling can also request Reset (device loss or mode changes),
    // so guarding only the workbench's explicit resize call is insufficient.
    if unsafe { state.client.read::<usize>(FONT_OBJECT) } == 0
        || state.window.sizing.load(Ordering::Acquire)
    {
        state.window.resize_pending.store(true, Ordering::Release);
        return DEVICE_LOST;
    }
    // Clear before measuring so a later WM_SIZE remains pending during Reset.
    state.window.resize_pending.swap(false, Ordering::AcqRel);
    let Some((width, height)) = (unsafe { client_size(state.client) }) else {
        state.window.resize_pending.store(true, Ordering::Release);
        return DEVICE_LOST;
    };
    let client = state.client;
    let fullscreen = unsafe { client.read::<i32>(0x1e86_6c70) } != 0;
    let previous_width = unsafe { client.read::<u32>(0x119d_d738) };
    let previous_height = unsafe { client.read::<u32>(0x119d_d73c) };
    unsafe {
        put(client.address(0x119d_1e04), width);
        put(client.address(0x119d_d344), height);
        put(client.address(0x119d_d738), width);
        put(client.address(0x119d_d73c), height);
        if !fullscreen {
            put(client.address(0x119d_d740), width);
            put(client.address(0x119d_d744), height);
        }
    }
    let original: Reset = unsafe { transmute(state.window.reset) };
    let result = unsafe { original(1) };
    if fullscreen {
        unsafe {
            put(client.address(0x119d_d738), previous_width);
            put(client.address(0x119d_d73c), previous_height);
        }
    }
    let was_failed = state.window.reset_failed.swap(result < 0, Ordering::AcqRel);
    if result < 0 {
        state.window.resize_pending.store(true, Ordering::Release);
        if !was_failed {
            eprintln!(
                "workbench: native window resize failed ({result:#010x}); retrying when ready"
            );
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::UI::WindowsAndMessaging::{
            CreateWindowExW, DeleteMenu, DestroyWindow, GetMenuState, MF_BYCOMMAND, SC_CLOSE,
            SC_MAXIMIZE, SC_RESTORE, SC_SIZE, WINDOW_EX_STYLE, WINDOW_STYLE, WS_CAPTION,
            WS_DISABLED, WS_MINIMIZEBOX,
        },
        core::w,
    };

    struct TestWindow(HWND);

    impl TestWindow {
        fn new(style: WINDOW_STYLE) -> Self {
            Self(
                unsafe {
                    CreateWindowExW(
                        WINDOW_EX_STYLE::default(),
                        w!("STATIC"),
                        w!("MHF workbench window test"),
                        style,
                        0,
                        0,
                        480,
                        320,
                        None,
                        None,
                        None,
                        None,
                    )
                }
                .unwrap(),
            )
        }
    }

    impl Drop for TestWindow {
        fn drop(&mut self) {
            let _ = unsafe { DestroyWindow(self.0) };
        }
    }

    #[test]
    fn window_controls_restore_deleted_commands_and_preserve_existing_styles() {
        let window = TestWindow::new(WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_DISABLED);
        let hwnd = window.0;
        let before = unsafe { window_style(hwnd) }.unwrap();
        let commands = [SC_CLOSE, SC_SIZE, SC_MAXIMIZE, SC_RESTORE];
        let menu = unsafe { GetSystemMenu(hwnd, false) };
        for command in commands {
            unsafe { DeleteMenu(menu, command, MF_BYCOMMAND) }.unwrap();
            assert_eq!(
                unsafe { GetMenuState(menu, command, MF_BYCOMMAND) },
                u32::MAX
            );
        }

        let hooks = Hooks::default();
        unsafe { hooks.configure(hwnd) }.unwrap();
        unsafe { hooks.configure(hwnd) }.unwrap();
        assert_eq!(
            unsafe { window_style(hwnd) }.unwrap(),
            before | WINDOW_CONTROLS
        );
        assert_eq!(
            hooks.style.lock().unwrap().unwrap().added,
            WINDOW_CONTROLS & !before
        );
        let menu = unsafe { GetSystemMenu(hwnd, false) };
        for command in commands {
            assert_ne!(
                unsafe { GetMenuState(menu, command, MF_BYCOMMAND) },
                u32::MAX
            );
        }
    }

    #[test]
    fn fullscreen_popup_keeps_its_style_during_mode_transitions() {
        let window = TestWindow::new(WS_POPUP | WS_SYSMENU);
        let before = unsafe { window_style(window.0) }.unwrap();
        let hooks = Hooks::default();
        unsafe { hooks.configure(window.0) }.unwrap();
        assert_eq!(unsafe { window_style(window.0) }.unwrap(), before);
        assert!(hooks.style.lock().unwrap().is_none());
    }
}
