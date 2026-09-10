use crate::{
    MhfLaunchParams32, MhfLaunchProfile,
    abi::{
        GameMain, HostServices32, MhfGlobalData32, MhfHostData32, copy_c_string, function32, ptr32,
    },
};
use std::{
    ffi::{CStr, CString, c_char, c_void},
    sync::atomic::{AtomicPtr, Ordering},
};
use windows::{
    Win32::{
        Foundation::{ERROR_ALREADY_EXISTS, GetLastError, HANDLE, HGLOBAL, HINSTANCE, HMODULE},
        System::{
            LibraryLoader::{GetModuleHandleA, GetProcAddress, LoadLibraryA},
            Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalUnlock},
            Threading::{CreateMutexA, GetCurrentProcessId},
        },
        UI::Input::KeyboardAndMouse::GetKeyboardLayout,
    },
    core::{Error, Owned, PCSTR},
};

const MHFO_MAIN: &CStr = c"mhDLL_Main";

static HOST_MESSAGE: AtomicPtr<c_char> = AtomicPtr::new(std::ptr::null_mut());

pub(super) struct NativeGame {
    pub(super) data: Box<MhfHostData32>,
    module: Option<MhfoModule>,
    _host_message: CString,
    _instance_mutex: Owned<HANDLE>,
    _ready_mutex: Owned<HANDLE>,
    _global_alloc: Owned<HGLOBAL>,
}

impl NativeGame {
    pub(super) fn new(
        profile: &MhfLaunchProfile<'_>,
        game_dir: &str,
        params: MhfLaunchParams32,
    ) -> Result<Self, String> {
        let host_message_buffer = set_host_message(profile.host_message)?;

        let process_id = unsafe { GetCurrentProcessId() };
        let mutex_name_text = format!("{} {process_id}", profile.instance_mutex_prefix);
        let ready_name_text = format!("{} {process_id}", profile.ready_mutex_prefix);
        let mutex_name = CString::new(mutex_name_text.as_str())
            .map_err(|_| "instance mutex prefix must not contain NUL".to_owned())?;
        let ready_name = CString::new(ready_name_text.as_str())
            .map_err(|_| "ready mutex prefix must not contain NUL".to_owned())?;
        let instance_mutex = create_unique_mutex(&mutex_name)?;
        let ready_mutex = create_unique_mutex(&ready_name)?;

        // Keep the 0x2010-byte parameter block adjacent to the launcher globals
        // that its embedded pointers reference, matching mhf-iel's host layout.
        let mut data = Box::new(MhfHostData32::default());
        data.data_ptr = ptr32(&mut data.params);
        data.keyboard_layout = ptr32(unsafe { GetKeyboardLayout(0) }.0);
        data.host_services = HostServices32::new(
            data.host_request.as_mut_ptr(),
            data.host_response.as_mut_ptr(),
            function32(host_validate as *const ()),
            function32(host_message as *const ()),
        );
        data.params = MhfLaunchParams32 {
            module_instance: ptr32(module_handle()?.0),
            mhf_mutex_number: 0,
            instance_mutex: ptr32((*instance_mutex).0),
            master_ready_mutex: ptr32((*ready_mutex).0),
            host_callback_release: function32(guard_release as *const ()),
            host_callback_state: function32(guard_state as *const ()),
            host_callback_query: function32(guard_query as *const ()),
            host_services: ptr32(&mut data.host_services),
            ..params
        };
        fill_launcher_fields(&mut data.params, profile, game_dir, &mutex_name_text)?;
        copy_c_string(
            "ready mutex name",
            &mut data.ready_mutex_name,
            ready_name_text.as_bytes(),
        )?;

        let game_global_alloc = allocate_global()?;
        data.params.global_alloc = ptr32((*game_global_alloc).0);

        Ok(Self {
            data,
            module: None,
            _host_message: host_message_buffer,
            _instance_mutex: instance_mutex,
            _ready_mutex: ready_mutex,
            _global_alloc: game_global_alloc,
        })
    }

    pub(super) fn launch(&mut self, mods: &mut mhf_mod_host::ModHost) -> Result<bool, String> {
        let pointer = unsafe { GlobalLock(*self._global_alloc) };
        if pointer.is_null() {
            return Err(format!("GlobalLock failed: {}", Error::from_thread()));
        }
        let mut target = mhf_mod_api::LaunchTargetV1 {
            params: &mut self.data.params,
            global: pointer.cast::<MhfGlobalData32>(),
        };
        // Both buffers are exclusively borrowed and the global allocation stays
        // locked until the single startup callback returns.
        let result = unsafe { mods.launch(&mut target) };
        let unlock = unsafe { GlobalUnlock(*self._global_alloc) };
        let ready = result?;
        match unlock {
            Ok(()) => Ok(ready),
            Err(error) if error.code().is_ok() => Ok(ready),
            Err(error) => Err(format!("GlobalUnlock failed: {error}")),
        }
    }

    pub(super) fn load(&mut self, profile: &MhfLaunchProfile<'_>) -> Result<HMODULE, String> {
        let game = MhfoModule::load(profile.game_dll)?;
        let entry = game.main()?;
        let handle = game.handle();
        self.data.mhfo_module = handle;
        self.data.mhfo_main = Some(entry);
        self.module = Some(game);
        Ok(handle)
    }

    pub(super) unsafe fn run(&mut self) -> i32 {
        let entry = self.data.mhfo_main.expect("game loaded before entry");
        unsafe { entry(&mut self.data.params) }
    }

    pub(super) fn unload(&mut self) {
        drop(self.module.take());
    }
}

fn fill_launcher_fields(
    params: &mut MhfLaunchParams32,
    profile: &MhfLaunchProfile<'_>,
    game_dir: &str,
    mutex_name: &str,
) -> Result<(), String> {
    copy_c_string("game directory", &mut params.game_dir, game_dir.as_bytes())?;
    copy_c_string(
        "launcher directory",
        &mut params.launcher_dir,
        game_dir.as_bytes(),
    )?;
    copy_c_string("mutex name", &mut params.mutex_name, mutex_name.as_bytes())?;
    copy_c_string(
        "INI name",
        &mut params.ini_name,
        profile.ini_name.as_bytes(),
    )
}

fn set_host_message(message: &str) -> Result<CString, String> {
    let message =
        CString::new(message).map_err(|_| "host message must not contain NUL".to_owned())?;
    HOST_MESSAGE.store(message.as_ptr().cast_mut(), Ordering::Relaxed);
    Ok(message)
}

fn module_handle() -> Result<HINSTANCE, String> {
    let handle = unsafe { GetModuleHandleA(PCSTR::null()) }
        .map_err(|error| format!("GetModuleHandleA failed: {error}"))?;
    Ok(handle.into())
}

fn pcstr(value: &CStr) -> PCSTR {
    PCSTR(value.as_ptr().cast())
}

struct MhfoModule(Owned<HMODULE>);

impl MhfoModule {
    fn load(name: &str) -> Result<Self, String> {
        let name = CString::new(name).map_err(|_| "DLL name must not contain NUL".to_owned())?;
        let handle = unsafe { LoadLibraryA(pcstr(&name)) }
            .map_err(|error| format!("LoadLibraryA({}) failed: {error}", name.to_string_lossy()))?;
        Ok(Self(unsafe { Owned::new(handle) }))
    }

    fn handle(&self) -> HMODULE {
        *self.0
    }

    fn main(&self) -> Result<GameMain, String> {
        let address = unsafe { GetProcAddress(self.handle(), pcstr(MHFO_MAIN)) }
            .map(|address| address as *const () as *mut c_void)
            .ok_or_else(|| {
                let code = unsafe { GetLastError() }.0;
                format!("GetProcAddress(mhDLL_Main) failed with Win32 error {code}")
            })?;
        Ok(unsafe { std::mem::transmute::<*mut c_void, GameMain>(address) })
    }
}

fn create_unique_mutex(name: &CStr) -> Result<Owned<HANDLE>, String> {
    let handle = unsafe { CreateMutexA(None, false, pcstr(name)) }
        .map_err(|error| format!("CreateMutexA({}) failed: {error}", name.to_string_lossy()))?;
    let already_exists = unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
    let handle = unsafe { Owned::new(handle) };
    if already_exists {
        Err(format!("{} already exists", name.to_string_lossy()))
    } else {
        Ok(handle)
    }
}

fn allocate_global() -> Result<Owned<HGLOBAL>, String> {
    let handle = unsafe {
        GlobalAlloc(
            GMEM_MOVEABLE | GMEM_ZEROINIT,
            std::mem::size_of::<MhfGlobalData32>(),
        )
    }
    .map_err(|error| format!("GlobalAlloc failed: {error}"))?;
    Ok(unsafe { Owned::new(handle) })
}

extern "C" fn guard_release(_context: *mut c_void) -> u32 {
    0
}

extern "C" fn guard_state() -> i32 {
    // mhf-iel's gg_proc returns success so mhfo can pass its launcher check.
    1
}

extern "C" fn guard_query(_context: *const c_void) -> i32 {
    0
}

extern "C" fn host_validate() -> i32 {
    0
}

extern "C" fn host_message() -> *const c_char {
    HOST_MESSAGE.load(Ordering::Relaxed)
}
