//! The native editor stores UTF-8 in its existing byte buffers. IMM ownership
//! and candidate UI are shared with egui through `mhf-overlay`.

mod editor;

use std::{
    ffi::c_void,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use mhf_hooks::{HookGuard, HookSlot};
use mhf_overlay::{HostIme, HostImeTarget, InputCaptureState};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE, HWND, LPARAM, WPARAM},
        System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA},
    },
    core::PCSTR,
};

use editor::Editor;

const EDITOR_RENDER_RVA: usize = 0x014D_42F0;
const MESSAGE_RVA: usize = 0x014D_3960;
const EDITOR_RVA: usize = 0x0EDB_A1BC;

type NativeMessage = unsafe extern "C" fn(HWND, u32, WPARAM, LPARAM) -> i32;
static MESSAGE_TARGET: AtomicUsize = AtomicUsize::new(0);
static HOOK_STATE: HookSlot<HookState> = HookSlot::new();

pub(super) struct GameIme {
    editor: Editor,
    module: ModuleReference,
}

impl GameIme {
    /// Called after LoadLibrary has initialized the supported game DLL, before
    /// mhDLL_Main. Its input callbacks must stop before module release.
    pub(super) unsafe fn new(module: HMODULE) -> Result<Arc<Self>, String> {
        let retained = unsafe { ModuleReference::acquire(module) }?;
        let base = module.0 as usize;
        unsafe { validate_layout(base) }?;
        Ok(Arc::new(Self {
            editor: unsafe { Editor::new(base) },
            module: retained,
        }))
    }

    pub(super) unsafe fn install(
        self: &Arc<Self>,
        capture: InputCaptureState,
    ) -> Result<HookGuard<HookState>, String> {
        let base = self.module.0;
        let mut hooks = HOOK_STATE.prepare()?;
        let message = unsafe {
            hooks.create(
                "native IME commands",
                (base + MESSAGE_RVA) as *mut c_void,
                message_hook as *mut c_void,
            )
        }?;
        MESSAGE_TARGET.store(base + MESSAGE_RVA, Ordering::Release);
        unsafe {
            hooks.install(HookState {
                adapter: Arc::clone(self),
                message: std::mem::transmute::<*mut c_void, NativeMessage>(message),
                capture,
            })
        }
    }
}

impl HostIme for GameIme {
    fn target(&self) -> Option<HostImeTarget> {
        let id = self.editor.target_id()?;
        let cursor_rect = self.editor.cursor_rect(id)?;
        Some(HostImeTarget { id, cursor_rect })
    }

    fn event(&self, id: usize, event: &egui::ImeEvent) {
        self.editor.event(id, event);
    }
}

pub(super) struct HookState {
    // Retain native memory if hook cleanup fails while the broker is removed.
    adapter: Arc<GameIme>,
    message: NativeMessage,
    capture: InputCaptureState,
}

unsafe extern "C" fn message_hook(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        let target = MESSAGE_TARGET.load(Ordering::Acquire);
        let original = unsafe { std::mem::transmute::<usize, NativeMessage>(target) };
        return unsafe { original(hwnd, message, wparam, lparam) };
    };
    // Native soft-keyboard commands can change conversion mode or composition.
    // They belong to the game editor and must not alter an active overlay editor.
    if (0x7E8..=0x7ED).contains(&message) && state.capture.captures_keyboard() {
        return 1;
    }
    if unsafe {
        state
            .adapter
            .editor
            .composition_command(hwnd, message, lparam)
    } {
        return 0;
    }
    unsafe { (state.message)(hwnd, message, wparam, lparam) }
}

struct ModuleReference(usize);

impl ModuleReference {
    unsafe fn acquire(module: HMODULE) -> Result<Self, String> {
        let mut retained = HMODULE::default();
        unsafe {
            GetModuleHandleExA(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCSTR(module.0.cast()),
                &mut retained,
            )
        }
        .map_err(|error| format!("failed to retain native IME module: {error}"))?;
        Ok(Self(retained.0 as usize))
    }
}

impl Drop for ModuleReference {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(HMODULE(self.0 as *mut _)) };
    }
}

unsafe fn validate_layout(base: usize) -> Result<(), String> {
    let read_u16 = |offset| unsafe { ((base + offset) as *const u16).read_unaligned() };
    let read_u32 = |offset| unsafe { ((base + offset) as *const u32).read_unaligned() };
    if read_u16(0) != 0x5A4D {
        return Err("native IME module is not a PE image".to_owned());
    }
    let pe = read_u32(0x3C) as usize;
    if pe > 0x1000 || read_u32(pe) != 0x4550 || read_u16(pe + 24) != 0x10B {
        return Err("native IME requires the supported 32-bit game DLL".to_owned());
    }
    if (read_u32(pe + 24 + 56) as usize) < EDITOR_RVA + 4 {
        return Err("native IME game image is too small for its editor layout".to_owned());
    }
    for (name, rva, bytes) in [
        (
            "editor renderer",
            EDITOR_RENDER_RVA,
            &[0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x6C, 0x01, 0x00, 0x00][..],
        ),
        (
            "IME message handler",
            MESSAGE_RVA,
            &[0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x14, 0xA1][..],
        ),
    ] {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, bytes.len()) } != bytes {
            return Err(format!("unsupported native {name} at RVA {rva:#x}"));
        }
    }
    if read_u32(EDITOR_RENDER_RVA + 23) as usize != base + EDITOR_RVA
        || read_u32(MESSAGE_RVA + 7) as usize != base + 0x0E86_6CA4
    {
        return Err("native IME globals do not match the supported editor layout".to_owned());
    }
    Ok(())
}
