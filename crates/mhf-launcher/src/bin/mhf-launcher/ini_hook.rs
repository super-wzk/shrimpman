use super::config::Store;
use minhook::MinHook;
use std::{
    ffi::{CStr, c_void},
    ptr::{null, null_mut},
    sync::{Mutex, MutexGuard, OnceLock, PoisonError},
};
use windows_sys::{
    Win32::Globalization::{CP_ACP, MultiByteToWideChar, WideCharToMultiByte},
    core::{BOOL, PCSTR, PSTR},
};

type GetPrivateProfileIntA = unsafe extern "system" fn(PCSTR, PCSTR, i32, PCSTR) -> u32;
type GetPrivateProfileStringA =
    unsafe extern "system" fn(PCSTR, PCSTR, PCSTR, PSTR, u32, PCSTR) -> u32;
type WritePrivateProfileStringA = unsafe extern "system" fn(PCSTR, PCSTR, PCSTR, PCSTR) -> BOOL;

struct HookState {
    file_name: Vec<u8>,
    store: Mutex<Store>,
    get_int: GetPrivateProfileIntA,
    get_string: GetPrivateProfileStringA,
    write_string: WritePrivateProfileStringA,
}

impl HookState {
    fn store(&self) -> MutexGuard<'_, Store> {
        self.store.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

static STATE: OnceLock<HookState> = OnceLock::new();

fn hook_state() -> &'static HookState {
    STATE
        .get()
        .expect("hook state must exist before hooks are enabled")
}

pub(crate) fn install(ini_name: &str, store: Store) -> Result<(), String> {
    if STATE.get().is_some() {
        return Err("TOML-backed INI hooks are already installed".to_owned());
    }
    if !ini_name.is_ascii() || ini_name.as_bytes().contains(&0) {
        return Err("INI file name must contain non-NUL ASCII bytes only".to_owned());
    }

    let get_int = unsafe {
        MinHook::create_hook_api(
            "kernel32.dll",
            "GetPrivateProfileIntA",
            get_private_profile_int as GetPrivateProfileIntA as *mut c_void,
        )
        .map_err(|status| format!("failed to hook GetPrivateProfileIntA: {status:?}"))?
    };
    let get_string = unsafe {
        MinHook::create_hook_api(
            "kernel32.dll",
            "GetPrivateProfileStringA",
            get_private_profile_string as GetPrivateProfileStringA as *mut c_void,
        )
        .map_err(|status| format!("failed to hook GetPrivateProfileStringA: {status:?}"))?
    };
    let write_string = unsafe {
        MinHook::create_hook_api(
            "kernel32.dll",
            "WritePrivateProfileStringA",
            write_private_profile_string as WritePrivateProfileStringA as *mut c_void,
        )
        .map_err(|status| format!("failed to hook WritePrivateProfileStringA: {status:?}"))?
    };

    STATE
        .set(HookState {
            file_name: ini_name.as_bytes().to_vec(),
            store: Mutex::new(store),
            get_int: unsafe { std::mem::transmute::<*mut c_void, GetPrivateProfileIntA>(get_int) },
            get_string: unsafe {
                std::mem::transmute::<*mut c_void, GetPrivateProfileStringA>(get_string)
            },
            write_string: unsafe {
                std::mem::transmute::<*mut c_void, WritePrivateProfileStringA>(write_string)
            },
        })
        .map_err(|_| "TOML-backed INI hooks are already installed".to_owned())?;

    unsafe { MinHook::enable_all_hooks() }
        .map_err(|status| format!("failed to enable TOML-backed INI hooks: {status:?}"))
}

unsafe extern "system" fn get_private_profile_int(
    app_name: PCSTR,
    key_name: PCSTR,
    default: i32,
    file_name: PCSTR,
) -> u32 {
    let state = hook_state();
    if !is_target_file(state, file_name) {
        return unsafe { (state.get_int)(app_name, key_name, default, file_name) };
    }

    let Some(section) = ansi_pointer_to_string(app_name) else {
        return default as u32;
    };
    let Some(key) = ansi_pointer_to_string(key_name) else {
        return default as u32;
    };
    let store = state.store();
    store
        .value(&section, &key)
        .as_deref()
        .map_or(default as u32, parse_profile_integer)
}

unsafe extern "system" fn get_private_profile_string(
    app_name: PCSTR,
    key_name: PCSTR,
    default: PCSTR,
    output: PSTR,
    size: u32,
    file_name: PCSTR,
) -> u32 {
    let state = hook_state();
    if !is_target_file(state, file_name) {
        return unsafe { (state.get_string)(app_name, key_name, default, output, size, file_name) };
    }

    if app_name.is_null() {
        let bytes = {
            let store = state.store();
            let names = store.section_names();
            profile_list_bytes(names.iter().map(String::as_str)).unwrap_or_default()
        };
        return unsafe { copy_profile_list(output, size, &bytes) };
    }

    let Some(section) = ansi_pointer_to_string(app_name) else {
        return unsafe { copy_profile_string(output, size, &default_bytes(default)) };
    };
    if key_name.is_null() {
        let bytes = {
            let store = state.store();
            let names = store.key_names(&section);
            profile_list_bytes(names.iter().map(String::as_str)).unwrap_or_default()
        };
        return unsafe { copy_profile_list(output, size, &bytes) };
    }

    let value = ansi_pointer_to_string(key_name).and_then(|key| {
        let store = state.store();
        store.value(&section, &key)
    });
    let bytes = value
        .as_deref()
        .and_then(string_to_ansi)
        .unwrap_or_else(|| default_bytes(default));
    unsafe { copy_profile_string(output, size, &bytes) }
}

unsafe extern "system" fn write_private_profile_string(
    app_name: PCSTR,
    key_name: PCSTR,
    value: PCSTR,
    file_name: PCSTR,
) -> BOOL {
    let state = hook_state();
    if !is_target_file(state, file_name) {
        return unsafe { (state.write_string)(app_name, key_name, value, file_name) };
    }

    if app_name.is_null() && key_name.is_null() && value.is_null() {
        return 1;
    }

    let mut store = state.store();
    let Some(section) = ansi_pointer_to_string(app_name) else {
        return 0;
    };
    let key = ansi_pointer_to_string(key_name);
    let value = ansi_pointer_to_string(value);
    let result = match (key, value) {
        (Some(key), Some(value)) => store.set_value(section, key, value),
        (Some(key), None) => store.remove_key(&section, &key),
        (None, None) => store.remove_section(&section),
        (None, Some(_)) => return 0,
    };
    BOOL::from(result.is_ok())
}

fn is_target_file(state: &HookState, file_name: PCSTR) -> bool {
    let Some(path) = pointer_bytes(file_name) else {
        return false;
    };
    let name = path
        .rsplit(|byte| matches!(byte, b'/' | b'\\'))
        .next()
        .unwrap_or(path.as_slice());
    name.eq_ignore_ascii_case(&state.file_name)
}

fn ansi_pointer_to_string(pointer: PCSTR) -> Option<String> {
    pointer_bytes(pointer).and_then(|bytes| ansi_to_string(&bytes))
}

fn pointer_bytes(pointer: PCSTR) -> Option<Vec<u8>> {
    if pointer.is_null() {
        None
    } else {
        Some(
            unsafe { CStr::from_ptr(pointer.cast()) }
                .to_bytes()
                .to_vec(),
        )
    }
}

fn ansi_to_string(bytes: &[u8]) -> Option<String> {
    if bytes.is_empty() {
        return Some(String::new());
    }
    let length = i32::try_from(bytes.len()).ok()?;
    let required = unsafe { MultiByteToWideChar(CP_ACP, 0, bytes.as_ptr(), length, null_mut(), 0) };
    if required <= 0 {
        return None;
    }
    let mut wide = vec![0; required as usize];
    let written = unsafe {
        MultiByteToWideChar(
            CP_ACP,
            0,
            bytes.as_ptr(),
            length,
            wide.as_mut_ptr(),
            required,
        )
    };
    if written != required {
        return None;
    }
    String::from_utf16(&wide).ok()
}

fn string_to_ansi(value: &str) -> Option<Vec<u8>> {
    if value.is_empty() {
        return Some(Vec::new());
    }
    let wide: Vec<u16> = value.encode_utf16().collect();
    let length = i32::try_from(wide.len()).ok()?;
    let required = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            length,
            null_mut(),
            0,
            null(),
            null_mut(),
        )
    };
    if required <= 0 {
        return None;
    }
    let mut bytes = vec![0; required as usize];
    let written = unsafe {
        WideCharToMultiByte(
            CP_ACP,
            0,
            wide.as_ptr(),
            length,
            bytes.as_mut_ptr(),
            required,
            null(),
            null_mut(),
        )
    };
    (written == required).then_some(bytes)
}

fn profile_list_bytes<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend(string_to_ansi(value)?);
        bytes.push(0);
    }
    Some(bytes)
}

fn default_bytes(default: PCSTR) -> Vec<u8> {
    let mut bytes = pointer_bytes(default).unwrap_or_default();
    while bytes.last() == Some(&b' ') {
        bytes.pop();
    }
    bytes
}

unsafe fn copy_profile_string(output: PSTR, size: u32, bytes: &[u8]) -> u32 {
    if output.is_null() || size == 0 {
        return 0;
    }
    let count = bytes.len().min(size as usize - 1);
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count);
        *output.add(count) = 0;
    }
    count as u32
}

unsafe fn copy_profile_list(output: PSTR, size: u32, bytes: &[u8]) -> u32 {
    if output.is_null() || size == 0 {
        return 0;
    }
    if bytes.is_empty() {
        unsafe {
            *output = 0;
            if size > 1 {
                *output.add(1) = 0;
            }
        }
        return 0;
    }
    if bytes.len() < size as usize {
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, bytes.len());
            *output.add(bytes.len()) = 0;
        }
        return bytes.len() as u32;
    }
    if size == 1 {
        unsafe { *output = 0 };
        return 0;
    }

    let count = size as usize - 2;
    unsafe {
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count);
        *output.add(count) = 0;
        *output.add(count + 1) = 0;
    }
    count as u32
}

fn parse_profile_integer(value: &str) -> u32 {
    let value = value.trim();
    if let Some(hex) = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        u32::from_str_radix(hex, 16).unwrap_or(0)
    } else {
        value.parse::<i32>().map_or(0, |value| value as u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_integer_matches_win32_conventions() {
        assert_eq!(parse_profile_integer(" 42 "), 42);
        assert_eq!(parse_profile_integer("0x2a"), 42);
        assert_eq!(parse_profile_integer("-1"), u32::MAX);
        assert_eq!(parse_profile_integer("invalid"), 0);
    }
}
