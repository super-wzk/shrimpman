use crate::paths::Paths;
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    cell::Cell,
    ffi::{CStr, OsString, c_void},
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::{Path, PathBuf},
};
use windows_sys::Win32::{
    Foundation::{
        GENERIC_EXECUTE, GENERIC_READ, GetLastError, HANDLE, INVALID_HANDLE_VALUE, SetLastError,
    },
    Globalization::{CP_ACP, CP_OEMCP, MB_ERR_INVALID_CHARS, MultiByteToWideChar},
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{
        self, AreFileApisANSI, FILE_FLAG_DELETE_ON_CLOSE, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_FLAG_POSIX_SEMANTICS, FILE_GENERIC_EXECUTE, FILE_GENERIC_READ, FILE_READ_DATA,
        OPEN_EXISTING,
    },
};

type CreateFileA = unsafe extern "system" fn(
    *const u8,
    u32,
    u32,
    *const SECURITY_ATTRIBUTES,
    u32,
    u32,
    HANDLE,
) -> HANDLE;
type CreateFileW = unsafe extern "system" fn(
    *const u16,
    u32,
    u32,
    *const SECURITY_ATTRIBUTES,
    u32,
    u32,
    HANDLE,
) -> HANDLE;

pub(crate) struct State {
    paths: Paths,
    open_a: CreateFileA,
    open_w: CreateFileW,
}

static STATE: HookSlot<State> = HookSlot::new();
thread_local! {
    static OPENING: Cell<bool> = const { Cell::new(false) };
}

pub(crate) fn install(paths: Paths) -> Result<HookGuard<State>, String> {
    let mut hooks = STATE.prepare()?;
    // Both exports have the exact system ABI declared above. The slot pins
    // their trampolines through each callback, including fallback calls.
    unsafe {
        let open_a = hooks.create_api(
            c"kernel32.dll",
            c"CreateFileA",
            create_file_a as CreateFileA as *mut c_void,
        )?;
        let open_w = hooks.create_api(
            c"kernel32.dll",
            c"CreateFileW",
            create_file_w as CreateFileW as *mut c_void,
        )?;
        hooks.install(State {
            paths,
            open_a: std::mem::transmute::<*mut c_void, CreateFileA>(open_a),
            open_w: std::mem::transmute::<*mut c_void, CreateFileW>(open_w),
        })
    }
}

struct Opening(bool);

impl Opening {
    fn enter() -> Self {
        Self(OPENING.replace(true))
    }
}

impl Drop for Opening {
    fn drop(&mut self) {
        OPENING.set(self.0);
    }
}

// Created first and dropped last: allocations, TLS and invocation draining
// must not overwrite the CreateFile result's thread-local error code.
struct LastError(u32);

impl Drop for LastError {
    fn drop(&mut self) {
        unsafe { SetLastError(self.0) };
    }
}

#[derive(Clone, Copy)]
struct Request {
    access: u32,
    share: u32,
    security: *const SECURITY_ATTRIBUTES,
    disposition: u32,
    flags: u32,
    template: HANDLE,
}

impl Request {
    fn is_read(self) -> bool {
        let read_access = GENERIC_READ | GENERIC_EXECUTE | FILE_GENERIC_READ | FILE_GENERIC_EXECUTE;
        self.disposition == OPEN_EXISTING
            && self.access & (GENERIC_READ | FILE_READ_DATA) != 0
            && self.access & !read_access == 0
            && self.flags
                & (FILE_FLAG_DELETE_ON_CLOSE
                    | FILE_FLAG_OPEN_REPARSE_POINT
                    | FILE_FLAG_POSIX_SEMANTICS)
                == 0
    }

    unsafe fn open(
        self,
        path: impl FnOnce() -> Option<PathBuf>,
        original: impl FnOnce(Option<&State>) -> HANDLE,
    ) -> HANDLE {
        let entry_error = unsafe { GetLastError() };
        let mut error = LastError(entry_error);
        let invocation = STATE.enter();
        let opening = Opening::enter();
        if let Some(state) = invocation.state().filter(|_| !opening.0 && self.is_read())
            && let Some(path) = path()
            && let Some(handle) = unsafe { self.replacement(state, &path, &mut error) }
        {
            return handle;
        }
        unsafe { SetLastError(entry_error) };
        let handle = original(invocation.state());
        error.0 = unsafe { GetLastError() };
        handle
    }

    unsafe fn replacement(
        self,
        state: &State,
        path: &Path,
        error: &mut LastError,
    ) -> Option<HANDLE> {
        let replacement = state.paths.replacement(path)?;
        if !replacement.is_file() {
            return None;
        }
        let wide: Vec<_> = replacement.as_os_str().encode_wide().chain([0]).collect();
        unsafe { SetLastError(error.0) };
        let handle = unsafe {
            (state.open_w)(
                wide.as_ptr(),
                self.access,
                self.share,
                self.security,
                self.disposition,
                self.flags,
                self.template,
            )
        };
        // The outer guard restores this after all path buffers and hook/TLS
        // guards have dropped, including after a redirected open succeeds.
        error.0 = unsafe { GetLastError() };
        (handle != INVALID_HANDLE_VALUE).then_some(handle)
    }
}

unsafe extern "system" fn create_file_a(
    filename: *const u8,
    access: u32,
    share: u32,
    security: *const SECURITY_ATTRIBUTES,
    disposition: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    let request = Request {
        access,
        share,
        security,
        disposition,
        flags,
        template,
    };
    unsafe {
        request.open(
            || ansi_path(filename),
            |state| {
                let original =
                    state.map_or(FileSystem::CreateFileA as CreateFileA, |state| state.open_a);
                original(
                    filename,
                    access,
                    share,
                    security,
                    disposition,
                    flags,
                    template,
                )
            },
        )
    }
}

unsafe extern "system" fn create_file_w(
    filename: *const u16,
    access: u32,
    share: u32,
    security: *const SECURITY_ATTRIBUTES,
    disposition: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    let request = Request {
        access,
        share,
        security,
        disposition,
        flags,
        template,
    };
    unsafe {
        request.open(
            || wide_path(filename),
            |state| {
                let original =
                    state.map_or(FileSystem::CreateFileW as CreateFileW, |state| state.open_w);
                original(
                    filename,
                    access,
                    share,
                    security,
                    disposition,
                    flags,
                    template,
                )
            },
        )
    }
}

unsafe fn wide_path(pointer: *const u16) -> Option<PathBuf> {
    if pointer.is_null() {
        return None;
    }
    let mut len = 0;
    while unsafe { *pointer.add(len) } != 0 {
        len += 1;
    }
    Some(OsString::from_wide(unsafe { std::slice::from_raw_parts(pointer, len) }).into())
}

unsafe fn ansi_path(pointer: *const u8) -> Option<PathBuf> {
    if pointer.is_null() {
        return None;
    }
    let bytes = unsafe { CStr::from_ptr(pointer.cast()) }.to_bytes();
    let len = i32::try_from(bytes.len()).ok()?;
    let code_page = if unsafe { AreFileApisANSI() } != 0 {
        CP_ACP
    } else {
        CP_OEMCP
    };
    let size = unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            pointer,
            len,
            std::ptr::null_mut(),
            0,
        )
    };
    if size == 0 {
        return None;
    }
    let mut wide = vec![0; size as usize];
    if unsafe {
        MultiByteToWideChar(
            code_page,
            MB_ERR_INVALID_CHARS,
            pointer,
            len,
            wide.as_mut_ptr(),
            size,
        )
    } != size
    {
        return None;
    }
    Some(OsString::from_wide(&wide).into())
}

#[cfg(test)]
mod tests;
