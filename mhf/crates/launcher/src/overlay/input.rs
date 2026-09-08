//! MHF reads immediate mouse/keyboard state through DirectInput 8A. See the
//! `GetDeviceState` call at mhfo-hd.dll RVA 0x014D1304 and cMOUSE at 0x014D23E0.
//! Resolve runtime vtables instead of depending on a particular game DLL build.

mod polling;

use std::{
    ffi::c_void,
    mem,
    panic::{AssertUnwindSafe, catch_unwind},
    ptr,
    sync::{
        Mutex, PoisonError,
        atomic::{AtomicPtr, Ordering},
    },
};

use mhf_hooks::{HookGuard, HookSlot};
use mhf_overlay::InputCaptureState;
use windows::{
    Win32::{
        Devices::HumanInterfaceDevice::{
            DI8DEVTYPE_KEYBOARD, DI8DEVTYPE_MOUSE, DIDEVICEINSTANCEA, DIRECTINPUT_VERSION,
            DirectInput8Create, GUID_SysKeyboard, GUID_SysMouse, IDirectInput8A,
            IDirectInputDevice8A,
        },
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{
            GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA, GetModuleHandleW,
        },
    },
    core::{HRESULT, Interface, PCSTR},
};

use polling::PolledInput;

type GetDeviceState = unsafe extern "system" fn(*mut c_void, u32, *mut c_void) -> HRESULT;
static TARGETS: [AtomicPtr<c_void>; 2] = [const { AtomicPtr::new(ptr::null_mut()) }; 2];
static HOOK_STATE: HookSlot<HookState> = HookSlot::new();

pub(super) struct HookState {
    original: [GetDeviceState; 2],
    capture: InputCaptureState,
    mouse: Mutex<PolledInput<8>>,
    keyboard: Mutex<PolledInput<256>>,
    _modules: Vec<ModuleReference>,
}

/// Called before mhDLL_Main; its device calls stop before this guard is removed.
pub(super) unsafe fn install(capture: InputCaptureState) -> Result<HookGuard<HookState>, String> {
    // On preparation failure, remove hooks before releasing their target DLLs.
    let mut modules = Vec::new();
    let mut hooks = HOOK_STATE.prepare()?;
    let instance = unsafe { GetModuleHandleW(None) }
        .map_err(|error| format!("failed to find the host module: {error}"))?;
    let mut raw = ptr::null_mut();
    unsafe {
        DirectInput8Create(
            instance.into(),
            DIRECTINPUT_VERSION,
            &IDirectInput8A::IID,
            &mut raw,
            None,
        )
    }
    .map_err(|error| format!("DirectInput8Create failed: {error}"))?;
    if raw.is_null() {
        return Err("DirectInput8Create returned no interface".to_owned());
    }
    let direct_input = unsafe { IDirectInput8A::from_raw(raw) };
    let mut devices = Vec::new();
    for guid in [GUID_SysMouse, GUID_SysKeyboard] {
        let mut device = None;
        unsafe { direct_input.CreateDevice(&guid, &mut device, None) }
            .map_err(|error| format!("DirectInput CreateDevice failed: {error}"))?;
        devices.push(device.ok_or("DirectInput CreateDevice returned no device")?);
    }
    let mouse = devices[0].vtable().GetDeviceState;
    let keyboard = devices[1].vtable().GetDeviceState;
    let targets = [mouse as *mut c_void, keyboard as *mut c_void];
    modules.push(unsafe { ModuleReference::acquire(targets[0]) }?);
    let first = unsafe {
        hooks.create(
            "DirectInput GetDeviceState",
            targets[0],
            get_device_state_hook::<0> as *mut c_void,
        )
    }?;
    let second = if targets[1] == targets[0] {
        first
    } else {
        modules.push(unsafe { ModuleReference::acquire(targets[1]) }?);
        unsafe {
            hooks.create(
                "DirectInput keyboard GetDeviceState",
                targets[1],
                get_device_state_hook::<1> as *mut c_void,
            )
        }?
    };
    for (slot, target) in TARGETS.iter().zip(targets) {
        slot.store(target, Ordering::Release);
    }
    unsafe {
        hooks.install(HookState {
            original: [
                mem::transmute::<*mut c_void, GetDeviceState>(first),
                mem::transmute::<*mut c_void, GetDeviceState>(second),
            ],
            capture,
            mouse: Mutex::new(PolledInput::default()),
            keyboard: Mutex::new(PolledInput::default()),
            _modules: modules,
        })
    }
}

unsafe extern "system" fn get_device_state_hook<const SLOT: usize>(
    device: *mut c_void,
    size: u32,
    data: *mut c_void,
) -> HRESULT {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let original = match state {
        Some(state) => state.original[SLOT],
        None => {
            let target = TARGETS[SLOT].load(Ordering::Acquire);
            if target.is_null() {
                return HRESULT(0x8000_4005_u32.cast_signed()); // E_FAIL
            }
            unsafe { mem::transmute::<*mut c_void, GetDeviceState>(target) }
        }
    };
    let result = unsafe { original(device, size, data) };
    if result.is_ok() && !data.is_null() && matches!(size, 16 | 20 | 256) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            if let Some(state) = state {
                unsafe { state.filter(device, size, data) };
            }
        }));
    }
    result
}

impl HookState {
    unsafe fn filter(&self, raw: *mut c_void, size: u32, data: *mut c_void) {
        let Some(device) = (unsafe { IDirectInputDevice8A::from_raw_borrowed(&raw) }) else {
            return;
        };
        let mut info = DIDEVICEINSTANCEA {
            dwSize: mem::size_of::<DIDEVICEINSTANCEA>() as u32,
            ..Default::default()
        };
        if unsafe { device.GetDeviceInfo(&mut info) }.is_err() {
            return;
        }
        // Check the device type as well as the format size: gamepads can share
        // the same implementation of GetDeviceState and must remain untouched.
        let data = unsafe { std::slice::from_raw_parts_mut(data.cast::<u8>(), size as usize) };
        match info.dwDevType & 0xff {
            DI8DEVTYPE_MOUSE if matches!(size, 16 | 20) => {
                let capture = self.capture.captures_pointer();
                self.mouse
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .mouse(raw as usize, data, capture, |button| {
                        self.capture.pointer_button_owner(button)
                    });
            }
            DI8DEVTYPE_KEYBOARD if size == 256 => {
                let capture = self.capture.captures_keyboard();
                self.keyboard
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .keyboard(raw as usize, data, capture, |scan_code| {
                        self.capture.scan_code_owner(scan_code)
                    });
            }
            _ => {}
        }
    }
}

// Keep the DLL containing each vtable target loaded until callbacks are drained.
// A module reference is process-wide and may be released on the cleanup thread.
struct ModuleReference(usize);

impl ModuleReference {
    unsafe fn acquire(address: *mut c_void) -> Result<Self, String> {
        let mut module = HMODULE::default();
        unsafe {
            GetModuleHandleExA(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCSTR(address.cast()),
                &mut module,
            )
        }
        .map_err(|error| format!("failed to retain the DirectInput runtime: {error}"))?;
        Ok(Self(module.0 as usize))
    }
}

impl Drop for ModuleReference {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(HMODULE(self.0 as *mut c_void)) };
    }
}
