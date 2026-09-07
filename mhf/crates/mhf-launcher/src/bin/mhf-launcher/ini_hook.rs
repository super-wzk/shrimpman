use super::config::Store;
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    ffi::{CStr, c_void},
    sync::{Mutex, MutexGuard, PoisonError},
};
use windows_sys::{
    Win32::System::WindowsProgramming,
    core::{BOOL, PCSTR, PSTR},
};

type GetPrivateProfileIntA = unsafe extern "system" fn(PCSTR, PCSTR, i32, PCSTR) -> u32;
type GetPrivateProfileStringA =
    unsafe extern "system" fn(PCSTR, PCSTR, PCSTR, PSTR, u32, PCSTR) -> u32;
type WritePrivateProfileStringA = unsafe extern "system" fn(PCSTR, PCSTR, PCSTR, PCSTR) -> BOOL;

pub(crate) struct HookState {
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

static STATE: HookSlot<HookState> = HookSlot::new();

pub(crate) fn install(ini_name: &str, store: Store) -> Result<HookGuard<HookState>, String> {
    if !ini_name.is_ascii() || ini_name.as_bytes().contains(&0) {
        return Err("INI file name must contain non-NUL ASCII bytes only".to_owned());
    }

    let mut hooks = STATE.prepare()?;
    let get_int = unsafe {
        hooks.create_api(
            c"kernel32.dll",
            c"GetPrivateProfileIntA",
            get_private_profile_int as GetPrivateProfileIntA as *mut c_void,
        )?
    };
    let get_string = unsafe {
        hooks.create_api(
            c"kernel32.dll",
            c"GetPrivateProfileStringA",
            get_private_profile_string as GetPrivateProfileStringA as *mut c_void,
        )?
    };
    let write_string = unsafe {
        hooks.create_api(
            c"kernel32.dll",
            c"WritePrivateProfileStringA",
            write_private_profile_string as WritePrivateProfileStringA as *mut c_void,
        )?
    };

    let state = HookState {
        file_name: ini_name.as_bytes().to_vec(),
        store: Mutex::new(store),
        get_int: unsafe { std::mem::transmute::<*mut c_void, GetPrivateProfileIntA>(get_int) },
        get_string: unsafe {
            std::mem::transmute::<*mut c_void, GetPrivateProfileStringA>(get_string)
        },
        write_string: unsafe {
            std::mem::transmute::<*mut c_void, WritePrivateProfileStringA>(write_string)
        },
    };
    // Every detour holds its invocation through the original API call.
    unsafe { hooks.install(state) }
}

unsafe extern "system" fn get_private_profile_int(
    app_name: PCSTR,
    key_name: PCSTR,
    default: i32,
    file_name: PCSTR,
) -> u32 {
    let invocation = STATE.enter();
    let state = invocation.state();
    let original = state.map_or(
        WindowsProgramming::GetPrivateProfileIntA as GetPrivateProfileIntA,
        |state| state.get_int,
    );
    let Some(state) = state.filter(|state| is_target_file(state, file_name)) else {
        return unsafe { original(app_name, key_name, default, file_name) };
    };

    let Some(section) = utf8_pointer_to_string(app_name) else {
        return default as u32;
    };
    let Some(key) = utf8_pointer_to_string(key_name) else {
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
    let invocation = STATE.enter();
    let state = invocation.state();
    let original = state.map_or(
        WindowsProgramming::GetPrivateProfileStringA as GetPrivateProfileStringA,
        |state| state.get_string,
    );
    let Some(state) = state.filter(|state| is_target_file(state, file_name)) else {
        return unsafe { original(app_name, key_name, default, output, size, file_name) };
    };

    if app_name.is_null() {
        let bytes = {
            let store = state.store();
            let names = store.section_names();
            profile_list_bytes(names.iter().map(String::as_str)).unwrap_or_default()
        };
        return unsafe { copy_profile_list(output, size, &bytes) };
    }

    let Some(section) = utf8_pointer_to_string(app_name) else {
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

    let value = utf8_pointer_to_string(key_name).and_then(|key| {
        let store = state.store();
        store.value(&section, &key)
    });
    let bytes = value
        .map(String::into_bytes)
        .unwrap_or_else(|| default_bytes(default));
    unsafe { copy_profile_string(output, size, &bytes) }
}

unsafe extern "system" fn write_private_profile_string(
    app_name: PCSTR,
    key_name: PCSTR,
    value: PCSTR,
    file_name: PCSTR,
) -> BOOL {
    let invocation = STATE.enter();
    let state = invocation.state();
    let original = state.map_or(
        WindowsProgramming::WritePrivateProfileStringA as WritePrivateProfileStringA,
        |state| state.write_string,
    );
    let Some(state) = state.filter(|state| is_target_file(state, file_name)) else {
        return unsafe { original(app_name, key_name, value, file_name) };
    };

    if app_name.is_null() && key_name.is_null() && value.is_null() {
        return 1;
    }

    let mut store = state.store();
    let Some(section) = utf8_pointer_to_string(app_name) else {
        return 0;
    };
    let key = utf8_pointer_to_string(key_name);
    let decoded_value = utf8_pointer_to_string(value);
    if (!key_name.is_null() && key.is_none()) || (!value.is_null() && decoded_value.is_none()) {
        return 0;
    }
    let value = decoded_value;
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

fn utf8_pointer_to_string(pointer: PCSTR) -> Option<String> {
    pointer_bytes(pointer).and_then(|bytes| String::from_utf8(bytes).ok())
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

fn profile_list_bytes<'a>(values: impl IntoIterator<Item = &'a str>) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    for value in values {
        if value.contains('\0') {
            return None;
        }
        bytes.extend_from_slice(value.as_bytes());
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
    let count = utf8_prefix(bytes, size as usize - 1);
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

    let limit = size as usize - 2;
    let count = utf8_prefix(bytes, limit);
    unsafe {
        std::ptr::write_bytes(output, 0, size as usize);
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), output, count);
    }
    // Win32 signals a truncated multi-string with nSize - 2, even when a whole
    // UTF-8 prefix leaves additional unused bytes before the double terminator.
    limit as u32
}

fn utf8_prefix(bytes: &[u8], limit: usize) -> usize {
    let mut end = bytes.len().min(limit);
    while end > 0 && end < bytes.len() && bytes[end] & 0xC0 == 0x80 {
        end -= 1;
    }
    end
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

    #[test]
    fn ini_hooks_restore_win32_and_can_be_installed_again() {
        use std::{
            ffi::CString,
            fs,
            time::{SystemTime, UNIX_EPOCH},
        };

        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let directory = std::env::temp_dir().join(format!("shrimpman-ini-hooks-{suffix}"));
        fs::create_dir(&directory).unwrap();
        let config_path = directory.join("mhf.toml");
        let source = r#"
[sign.http]
base_url = "http://127.0.0.1:53001"
[hook_test]
value = "42"
"#;
        fs::write(&config_path, source).unwrap();
        let ini_name = format!("shrimpman-virtual-{suffix}.ini");
        let ini_file = CString::new(ini_name.as_str()).unwrap();
        let section = c"hook_test".as_ptr().cast();
        let key = c"value".as_ptr().cast();
        let file = ini_file.as_ptr().cast();
        let read =
            || unsafe { WindowsProgramming::GetPrivateProfileIntA(section, key, 1234, file) };

        let (_, store) = super::super::config::load(config_path.clone()).unwrap();
        let mut hooks = install(&ini_name, store).unwrap();
        assert_eq!(read(), 42);
        assert_eq!(
            unsafe {
                WindowsProgramming::GetPrivateProfileIntA(
                    section,
                    key,
                    1234,
                    c"shrimpman-unrelated.ini".as_ptr().cast(),
                )
            },
            1234
        );
        let mut output = [0; 16];
        assert_eq!(
            unsafe {
                WindowsProgramming::GetPrivateProfileStringA(
                    section,
                    key,
                    c"missing".as_ptr().cast(),
                    output.as_mut_ptr(),
                    output.len() as u32,
                    file,
                )
            },
            2
        );
        assert_eq!(&output[..3], b"42\0");
        assert_ne!(
            unsafe {
                WindowsProgramming::WritePrivateProfileStringA(
                    section,
                    key,
                    c"84".as_ptr().cast(),
                    file,
                )
            },
            0
        );
        let unicode_key = c"中文".as_ptr().cast();
        assert_ne!(
            unsafe {
                WindowsProgramming::WritePrivateProfileStringA(
                    section,
                    unicode_key,
                    c"啊🙂".as_ptr().cast(),
                    file,
                )
            },
            0
        );
        let mut unicode = [0; 8];
        assert_eq!(
            unsafe {
                WindowsProgramming::GetPrivateProfileStringA(
                    section,
                    unicode_key,
                    c"".as_ptr().cast(),
                    unicode.as_mut_ptr(),
                    unicode.len() as u32,
                    file,
                )
            },
            7
        );
        assert_eq!(&unicode, "啊🙂\0".as_bytes());
        let mut short = [0; 5];
        assert_eq!(
            unsafe { copy_profile_string(short.as_mut_ptr(), 5, "啊🙂".as_bytes()) },
            3
        );
        assert_eq!(&short[..4], "啊\0".as_bytes());
        hooks.uninstall().unwrap();
        assert_eq!(read(), 1234);
        assert_eq!(
            unsafe { get_private_profile_int(section, key, 1234, file) },
            1234
        );

        let (_, store) = super::super::config::load(config_path).unwrap();
        let hooks = install(&ini_name, store).unwrap();
        assert_eq!(read(), 84, "reinstall must read the persisted TOML value");
        drop(hooks);
        assert_eq!(read(), 1234);
        fs::remove_dir_all(directory).unwrap();
    }
}
