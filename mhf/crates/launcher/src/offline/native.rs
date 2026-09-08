use super::Session;
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    ffi::{CStr, c_char, c_void},
    mem::transmute,
    ptr,
    sync::{
        PoisonError,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA},
    },
    core::PCSTR,
};

static SLOT: HookSlot<State> = HookSlot::new();
static BASE: AtomicUsize = AtomicUsize::new(0);

// Only the three offline entrypoints. Debug command, movement and combat
// signatures belong to the optional debug hook group.
const SIGNATURES: &[(usize, &[u8])] = &[
    (
        0x008d25a0,
        &[0x55, 0x8b, 0xec, 0x51, 0x53, 0x56, 0x8b, 0x75],
    ),
    (
        0x01501c30,
        &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x18, 0x02, 0x00],
    ),
    (0x00817950, &[0x55, 0x8b, 0xec, 0x51, 0x0f, 0xbe, 0x05]),
];

pub(crate) struct State {
    // Release the DLL before the session buffers it could inspect in DllMain.
    _module: ModuleReference,
    base: usize,
    session: Session,
    bootstrap: usize,
    initialized: AtomicBool,
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
        .map_err(|error| format!("failed to retain the offline game module: {error}"))?;
        Ok(Self(retained.0 as usize))
    }
}

impl Drop for ModuleReference {
    fn drop(&mut self) {
        let _ = unsafe { FreeLibrary(HMODULE(self.0 as *mut c_void)) };
    }
}

impl State {
    fn address(&self, va: usize) -> usize {
        self.base + va - 0x1000_0000
    }
    unsafe fn read<T: Copy>(&self, va: usize) -> T {
        unsafe { get(self.address(va)) }
    }
    unsafe fn write<T>(&self, va: usize, value: T) {
        unsafe { put(self.address(va), value) }
    }
    unsafe fn call(&self, va: usize) {
        let function: unsafe extern "C" fn() = unsafe { transmute(self.address(va)) };
        unsafe { function() };
    }
}

unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}
unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

unsafe fn validate(base: usize) -> Result<(), String> {
    for &(rva, expected) in SIGNATURES {
        if unsafe { std::slice::from_raw_parts((base + rva) as *const u8, expected.len()) }
            != expected
        {
            return Err(format!("不支持此游戏 DLL 的离线接口：RVA {rva:#x}"));
        }
    }
    Ok(())
}

/// Stop all native callers, then remove debug hooks before this offline guard.
/// The guard retains the target DLL and prepared quest buffers through cleanup.
pub(crate) unsafe fn install(
    module: HMODULE,
    session: Session,
) -> Result<HookGuard<State>, String> {
    let base = module.0 as usize;
    unsafe { validate(base) }?;
    let retained = unsafe { ModuleReference::acquire(module) }?;
    let mut hooks = SLOT.prepare()?;
    BASE.store(base, Ordering::Relaxed);
    let bootstrap = unsafe {
        hooks.create(
            "offline bootstrap",
            (base + 0x008d25a0) as _,
            bootstrap as *mut c_void,
        )
    }?;
    unsafe {
        hooks.create(
            "local quest request",
            (base + 0x01501c30) as _,
            request_quest as *mut c_void,
        )?;
        hooks.create(
            "local quest delivery",
            (base + 0x00817950) as _,
            poll_quest as *mut c_void,
        )?;
        hooks.create_api(c"ws2_32.dll", c"connect", reject_connection as *mut c_void)?;
        hooks.create_api(
            c"ws2_32.dll",
            c"WSAConnect",
            reject_wsa_connection as *mut c_void,
        )?;
        *session
            .inner
            .quest_override
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = None;
        session.inner.started.store(false, Ordering::Release);
        hooks.install(State {
            _module: retained,
            base,
            session,
            bootstrap: bootstrap as usize,
            initialized: AtomicBool::new(false),
        })
    }
}

#[link(name = "ws2_32")]
unsafe extern "system" {
    fn WSASetLastError(error: i32);
}

unsafe extern "system" fn reject_connection(_: usize, _: *const c_void, _: i32) -> i32 {
    unsafe { WSASetLastError(10013) }; // WSAEACCES: this process is an offline session.
    -1
}
unsafe extern "system" fn reject_wsa_connection(
    socket: usize,
    name: *const c_void,
    len: i32,
    _: *const c_void,
    _: *mut c_void,
    _: *const c_void,
    _: *const c_void,
) -> i32 {
    unsafe { reject_connection(socket, name, len) }
}

unsafe extern "thiscall" fn request_quest(this: *mut c_void, kind: u8, name: *const c_char) -> u8 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "thiscall" fn(*mut c_void, u8, *const c_char) -> u8 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x01501c30) };
        return unsafe { original(this, kind, name) };
    };
    let requested = unsafe { CStr::from_ptr(name) }.to_bytes();
    let expected = format!("{:05}", state.session.quest_id());
    if kind != 0 || !requested.starts_with(expected.as_bytes()) {
        return 0;
    }
    unsafe { state.write(0x1e4528a4, 1_u32) };
    1
}

unsafe extern "C" fn poll_quest() -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x00817950) };
        return unsafe { original() };
    };
    unsafe {
        if state.read::<u8>(0x1e76afea) == 1 {
            let destination = state.read::<usize>(0x1e774b78) as *mut u8;
            if destination.is_null() {
                return 0;
            }
            let quest_override = state
                .session
                .inner
                .quest_override
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            let bytes = quest_override
                .as_deref()
                .unwrap_or(&state.session.inner.quest.bytes);
            ptr::copy_nonoverlapping(bytes.as_ptr(), destination, bytes.len());
            state.write(0x1e774b74, bytes.len() as u32);
            state.write(0x1e4528a4, 0_u32);
            state.write(0x1e76afea, 2_u8);
            return 0;
        }
        i32::from(state.read::<u8>(0x1e76afea))
    }
}

unsafe extern "C" fn bootstrap(task: *mut u8) -> i32 {
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(*mut u8) -> i32 =
            unsafe { transmute(BASE.load(Ordering::Relaxed) + 0x008d25a0) };
        return unsafe { original(task) };
    };
    unsafe {
        if *task.add(8) == 0 {
            let original: unsafe extern "C" fn(*mut u8) -> i32 = transmute(state.bootstrap);
            return original(task);
        }
        if !state.initialized.swap(true, Ordering::Relaxed) {
            let appearance: unsafe extern "C" fn(i16) = transmute(state.address(0x10834940));
            let create: unsafe extern "C" fn(i8) = transmute(state.address(0x10834bf0));
            appearance(0);
            create(-1);
            let save = state.read::<usize>(0x11a3ee2c);
            ptr::copy_nonoverlapping(c"Debug".as_ptr().cast(), (save + 88) as *mut u8, 6);
            prepare_scene(state);
            state.session.inner.started.store(true, Ordering::Release);
        }
        ptr::write_unaligned(task.cast::<u16>(), 0);
    }
    0
}

unsafe fn prepare_scene(state: &State) {
    unsafe {
        state.call(0x1089def0);
        state.call(0x107a1540);
        let scene = state.read::<usize>(0x1e7fff3c);
        put(scene + 9208, 0_u8);
        put(scene + 9210, 1_u8);
        put(scene + 9215, 0_u8);
        put(scene + 9672, 1_u8);
        state.write(0x1ed52870, 0_u8);
        state.write(0x1ed52951, 0_u16);
        state.write(0x1ed6bca0, 9_u8);
        state.write(0x1ed7d418, state.session.quest_id());
        state.write(0x1ed5291c, 0_u16);
        state.call(0x10a7fcf0); // Populate the party equipment from the current save.
        let record = state.address(0x1ee3df00) as *mut u8;
        ptr::write_bytes(record, 0, 0x390);
        put(record as usize, 0x30_u32);
        ptr::copy_nonoverlapping(
            state
                .session
                .inner
                .quest
                .bytes
                .as_ptr()
                .add(state.session.inner.quest.properties),
            record.add(16),
            320,
        );
        state.write(0x1e76af08, 1_u16);
        let task = state.address(0x1edb98c0) as *mut u8;
        ptr::write_bytes(task, 0, 32);
        put(task as usize, 12_u32);
        put(task as usize + 4, state.address(0x10899d90));
    }
}

/// The caller has already released tool-owned actor references on the game thread.
#[cfg(feature = "debug")]
pub(super) unsafe fn restart(session: &Session) -> Result<(), String> {
    let invocation = SLOT.enter();
    let state = invocation
        .state()
        .filter(|state| std::sync::Arc::ptr_eq(&state.session.inner, &session.inner))
        .ok_or("离线任务会话尚未运行")?;
    if !session.started() {
        return Err("离线猎人尚未初始化".into());
    }
    unsafe {
        state.call(0x10b738a0);
        ptr::write_bytes(state.address(0x1edb9820) as *mut u8, 0, 0x120);
        prepare_scene(state);
    }
    Ok(())
}

#[cfg(feature = "debug")]
pub(super) fn validate_session(session: &Session, module: HMODULE) -> Result<(), String> {
    let invocation = SLOT.enter();
    if invocation.state().is_some_and(|state| {
        state.base == module.0 as usize
            && std::sync::Arc::ptr_eq(&state.session.inner, &session.inner)
    }) {
        Ok(())
    } else {
        Err("离线会话未安装到此游戏 DLL".into())
    }
}

#[cfg(test)]
mod tests {
    use super::{SIGNATURES, validate};

    #[test]
    fn offline_validation_does_not_require_debug_command_or_monster_code() {
        let size = SIGNATURES
            .iter()
            .map(|(rva, bytes)| rva + bytes.len())
            .max()
            .unwrap();
        let mut image = vec![0; size];
        for &(rva, bytes) in SIGNATURES {
            image[rva..rva + bytes.len()].copy_from_slice(bytes);
        }
        for rva in [0x008fcee0, 0x008696d0, 0x00baee10, 0x008b7b60] {
            image[rva..rva + 8].fill(0xcc);
        }
        assert!(unsafe { validate(image.as_ptr() as usize) }.is_ok());
        image[0x00817950] ^= 0xff;
        assert!(unsafe { validate(image.as_ptr() as usize) }.is_err());
    }

    #[cfg(feature = "debug")]
    #[test]
    #[ignore = "set MHF_UTF8_TEST_CLIENT to the supported game DLL; loads its real DllMain"]
    fn supported_client_keeps_offline_and_debug_hook_lifetimes_separate() {
        use super::{ModuleReference, install};
        use crate::{debug, offline::Session};
        use std::sync::atomic::Ordering;
        use windows::{
            Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
            core::PCWSTR,
        };

        let path = std::env::var("MHF_UTF8_TEST_CLIENT").expect("MHF_UTF8_TEST_CLIENT");
        let wide: Vec<u16> = path.encode_utf16().chain(Some(0)).collect();
        let module =
            unsafe { LoadLibraryExW(PCWSTR(wide.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
                .expect("load supported game DLL and adjacent dependencies");
        let loaded = ModuleReference(module.0 as usize);
        let entry = |rva| unsafe {
            std::slice::from_raw_parts((module.0 as usize + rva) as *const u8, 16).to_vec()
        };
        let offline_rvas = [0x008d25a0, 0x01501c30, 0x00817950];
        let debug_rvas = [
            0x008fcee0, 0x0089e510, 0x008696d0, 0x00a5b800, 0x00baee10, 0x00b7b570, 0x008b6bf0,
            0x008b7b60, 0x00846ca0,
        ];
        let offline_originals = offline_rvas.map(entry);
        let debug_originals = debug_rvas.map(entry);
        let session = Session::test_map().unwrap();

        for _ in 0..2 {
            // Reusing a prepared Session starts a fresh original quest run.
            session.inner.started.store(true, Ordering::Release);
            session.prepare_monster(1, 461, [0.0; 3], 0).unwrap();
            let mut offline =
                unsafe { install(module, session.clone()) }.expect("install offline only");
            assert!(!session.started());
            assert!(!session.override_contains(0, 1));
            for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
                assert_eq!(
                    entry(rva),
                    *original,
                    "offline changed debug entry {rva:#x}"
                );
            }
            for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
                assert_ne!(
                    entry(rva),
                    *original,
                    "offline entry {rva:#x} was not hooked"
                );
            }

            let mut tools =
                unsafe { debug::install(module, session.clone(), debug::DebugControl::new()) }
                    .expect("add optional debug hooks to the running offline group");
            assert!(
                !session.started(),
                "install must not bootstrap the hunter or read the catalog"
            );
            for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
                assert_ne!(entry(rva), *original, "debug entry {rva:#x} was not hooked");
            }
            tools
                .uninstall()
                .expect("remove optional debug hooks first");
            for (rva, original) in debug_rvas.into_iter().zip(&debug_originals) {
                assert_eq!(
                    entry(rva),
                    *original,
                    "debug entry {rva:#x} was not restored"
                );
            }
            for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
                assert_ne!(
                    entry(rva),
                    *original,
                    "removing debug also removed offline entry {rva:#x}"
                );
            }

            offline
                .uninstall()
                .expect("remove offline hooks after debug");
            for (rva, original) in offline_rvas.into_iter().zip(&offline_originals) {
                assert_eq!(
                    entry(rva),
                    *original,
                    "offline entry {rva:#x} was not restored"
                );
            }
        }
        drop(loaded);
    }
}
