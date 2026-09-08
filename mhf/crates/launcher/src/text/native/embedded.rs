//! Text constructors that copy embedded CP932 words instead of loading a string
//! pointer. Their destination sizes and register contracts come from the callers.

use std::{cell::Cell, ffi::c_void, ptr, sync::atomic::AtomicUsize};

use windows::Win32::System::SystemInformation::GetLocalTime;
use windows_sys::Win32::{
    Foundation::{GetLastError, HANDLE, INVALID_HANDLE_VALUE, SetLastError},
    Security::SECURITY_ATTRIBUTES,
    Storage::FileSystem::{CreateFileA, CreateFileW},
};

use super::{CodeHook, HOOK_STATE, Registers, copy_z, fullwidth, read_z, signature};
use crate::text::utf8::{byte_offset, display_columns, truncate};

const NAME_CAPACITY: usize = 260;
const SUBSTITUTION_CAPACITY: usize = 1024;
const MAIL_CAPACITY: usize = 0x7ff;
const NAME: &str = "アイルー";

thread_local! {
    // Keep the game's static CRT FILE object, converting only this fopen's OS
    // path boundary. Other native CRT opens retain their existing behavior.
    static SHORT_MAIL_OPEN: Cell<bool> = const { Cell::new(false) };
}

macro_rules! inline_hook {
    ($original:ident, $detour:ident, $operation:literal, $pop:literal, $dispatch:ident) => {
        static $original: AtomicUsize = AtomicUsize::new(0);
        #[unsafe(naked)]
        unsafe extern "C" fn $detour() {
            core::arch::naked_asm!(
                "pushfd", "pushad", "mov eax, esp",
                "sub esp, 528", "and esp, -16", "fxsave [esp]", "mov [esp + 512], eax",
                "push {operation}", "push eax", "call {dispatch}", "add esp, 8",
                "fxrstor [esp]", "mov esp, [esp + 512]", "test eax, eax", "jz 2f",
                "popad", "popfd", "lea esp, [esp + {pop}]",
                "jmp dword ptr [esp - {continuation_offset}]",
                "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                operation = const $operation, pop = const $pop,
                continuation_offset = const 24 + $pop,
                dispatch = sym $dispatch, original = sym $original,
            );
        }
    };
}

macro_rules! function_hook {
    ($original:ident, $detour:ident, $operation:literal) => {
        static $original: AtomicUsize = AtomicUsize::new(0);
        #[unsafe(naked)]
        unsafe extern "C" fn $detour() {
            core::arch::naked_asm!(
                "pushfd", "pushad", "mov eax, esp", "push {operation}", "push eax",
                "call {dispatch}", "add esp, 8", "test eax, eax", "jz 2f",
                "popad", "popfd", "ret",
                "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                operation = const $operation, dispatch = sym function_dispatch,
                original = sym $original,
            );
        }
    };
}

inline_hook!(ACTOR_INIT_ORIGINAL, actor_init, 0, 0, inline_dispatch);
inline_hook!(ACTOR_RESET_ORIGINAL, actor_reset, 1, 0, inline_dispatch);
inline_hook!(PERCENT_ORIGINAL, percent, 2, 0, inline_dispatch);
inline_hook!(COUNTDOWN_ORIGINAL, countdown, 3, 0, inline_dispatch);
inline_hook!(
    COUNTDOWN_WIDTH_ORIGINAL,
    countdown_width,
    4,
    0,
    inline_dispatch
);
inline_hook!(MAIL_OPEN_ORIGINAL, mail_open, 5, 0, inline_dispatch);
inline_hook!(CRT_FILE_ORIGINAL, crt_file, 6, 0, inline_dispatch);
inline_hook!(CRT_RETRY_ORIGINAL, crt_retry, 7, 28, inline_dispatch);
function_hook!(PLAYER_NAME_ORIGINAL, player_name, 0);
function_hook!(SUBSTITUTION_ORIGINAL, substitution, 1);
function_hook!(MAIL_PATH_ORIGINAL, mail_path, 2);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "UTF-8 default companion name",
            rva: 0x0073_E686,
            signature: &[
                (0, 0xa1),
                (5, 0x89),
                (6, 0x82),
                (7, 0x7c),
                (8, 0x0b),
                (9, 0),
                (10, 0),
                (11, 0x8b),
                (12, 0x0d),
            ],
            detour: actor_init as *mut _,
            original: &ACTOR_INIT_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 reset companion name",
            rva: 0x0073_EA60,
            signature: &[
                (0, 0x8b),
                (1, 0x0d),
                (6, 0x89),
                (7, 0x88),
                (8, 0x7c),
                (9, 0x0b),
                (10, 0),
                (11, 0),
            ],
            detour: actor_reset as *mut _,
            original: &ACTOR_RESET_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 embedded percent suffix",
            rva: 0x0158_7E4F,
            signature: &[
                (0, 0x66),
                (1, 0x8b),
                (2, 0x0d),
                (7, 0x8a),
                (8, 0x15),
                (13, 0x66),
                (14, 0x89),
                (15, 0x08),
            ],
            detour: percent as *mut _,
            original: &PERCENT_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 countdown digits",
            rva: 0x0158_7FE5,
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0x0f, 0xbf, 0xc8, 0xb8, 0x89, 0x88, 0x88, 0x88, 0xf7, 0xe9]);
                SIGNATURE
            },
            detour: countdown as *mut _,
            original: &COUNTDOWN_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 countdown centering",
            rva: 0x0158_806A,
            signature: {
                const SIGNATURE: &[(usize, u8)] = &signature([
                    0x8d, 0x71, 0x01, 0x8d, 0x49, 0x00, 0x8a, 0x11, 0x41, 0x84, 0xd2, 0x75, 0xf9,
                ]);
                SIGNATURE
            },
            detour: countdown_width as *mut _,
            original: &COUNTDOWN_WIDTH_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 short-mail CRT scope",
            rva: 0x005C_5B9C,
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0xe8, 0x5f, 0x5e, 0xfe, 0, 0x8b, 0xf0, 0x83, 0xc4, 0x08]);
                SIGNATURE
            },
            detour: mail_open as *mut _,
            original: &MAIL_OPEN_ORIGINAL,
        },
        CodeHook {
            name: "Unicode short-mail CreateFile target",
            rva: 0x015C_1517,
            signature: &[
                (0, 0x8b),
                (1, 0x3d),
                (6, 0x6a),
                (7, 0),
                (8, 0xff),
                (9, 0x75),
                (10, 0xf0),
            ],
            detour: crt_file as *mut _,
            original: &CRT_FILE_ORIGINAL,
        },
        CodeHook {
            name: "Unicode short-mail CreateFile retry",
            rva: 0x015C_19C1,
            signature: &[
                (0, 0xff),
                (1, 0x15),
                (6, 0x3b),
                (7, 0xc3),
                (8, 0x75),
                (9, 0x34),
            ],
            detour: crt_retry as *mut _,
            original: &CRT_RETRY_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 escaped player name",
            rva: 0x0097_4C30,
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0x56, 0x57, 0x8b, 0xf0, 0xe8, 0x27, 0xf7, 0xf6, 0xff]);
                SIGNATURE
            },
            detour: player_name as *mut _,
            original: &PLAYER_NAME_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 player-name substitution",
            rva: 0x0097_4D80,
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0x55, 0x8b, 0xec, 0x81, 0xec, 0x08, 0x01, 0, 0]);
                SIGNATURE
            },
            detour: substitution as *mut _,
            original: &SUBSTITUTION_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 short-mail filename",
            rva: 0x014D_8B70,
            signature: {
                const SIGNATURE: &[(usize, u8)] = &signature([0x55, 0x8b, 0xec, 0x83, 0xec, 0x28]);
                SIGNATURE
            },
            detour: mail_path as *mut _,
            original: &MAIL_PATH_ORIGINAL,
        },
    ]
}

unsafe extern "C" fn inline_dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let registers = unsafe { &mut *registers };
    let base = state.module_base;
    let mut last_error = None;
    let continuation = match operation {
        0 => {
            // Constructors at 1065C280/CA70/D0F0 prove an 18-byte name field.
            unsafe {
                write_text((registers.edx as usize + 2940) as *mut u8, 18, NAME);
            }
            registers.eax &= !0xff; // The original returned the copied NUL in AL.
            0x0073_E6A8
        }
        1 => {
            unsafe {
                write_text((registers.eax as usize + 2940) as *mut u8, 18, NAME);
            }
            0x0073_EA84
        }
        2 => {
            let start = registers.ebp as usize - 0x90;
            let destination = registers.eax as usize;
            if let Some(used) = destination.checked_sub(start).filter(|used| *used < 128) {
                unsafe {
                    write_text(destination as *mut u8, 128 - used, "％");
                }
            }
            0x0158_7E62
        }
        3 => {
            // Format spans [EBP-0Ch, EBP-4), immediately before the stack cookie.
            let destination = (registers.ebp as usize - 12) as *mut u8;
            let text = countdown_text(registers.eax as u16 as i16);
            unsafe {
                write_text(destination, 8, &text);
            }
            registers.ecx = destination as u32;
            registers.eax = unsafe { read_u32(base + 0x0E3C_BD64) };
            0x0158_8050
        }
        4 => {
            let text = unsafe { read_text(registers.ecx as *const u8, 8) }.unwrap_or_default();
            registers.esi = registers.ecx.wrapping_add(1);
            registers.ecx = display_columns(text) as u32;
            0x0158_8079
        }
        5 => {
            let filename = unsafe { inline_argument(registers, 0) } as *const u8;
            let mode = unsafe { inline_argument(registers, 1) } as *const u8;
            let open: unsafe extern "C" fn(*const u8, *const u8) -> *mut c_void =
                unsafe { std::mem::transmute(base + 0x015A_BA00) };
            registers.eax = with_short_mail_open(|| unsafe { open(filename, mode) }) as u32;
            last_error = Some(unsafe { GetLastError() });
            0x005C_5BA1
        }
        6 if SHORT_MAIL_OPEN.get() => {
            // The static CRT calls this register twice on its ordinary open path.
            registers.edi = create_file as *const () as u32;
            0x015C_151D
        }
        7 if SHORT_MAIL_OPEN.get() => {
            let arguments = std::array::from_fn::<_, 7, _>(|index| unsafe {
                inline_argument(registers, index)
            });
            registers.eax = unsafe {
                create_file(
                    arguments[0] as *const u8,
                    arguments[1],
                    arguments[2],
                    arguments[3] as *const SECURITY_ATTRIBUTES,
                    arguments[4],
                    arguments[5],
                    arguments[6] as HANDLE,
                )
            } as u32;
            last_error = Some(unsafe { GetLastError() });
            0x015C_19C7
        }
        _ => return 0,
    };
    registers.esp = (base + continuation) as u32;
    drop(invocation);
    if let Some(error) = last_error {
        unsafe {
            SetLastError(error);
        }
    }
    1
}

unsafe extern "C" fn function_dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let registers = unsafe { &mut *registers };
    let base = state.module_base;
    match operation {
        0 => {
            let (name, _, _) = unsafe { displayed_name(base) };
            unsafe {
                write_text(registers.eax as *mut u8, NAME_CAPACITY, &name);
            }
        }
        1 => {
            let source = unsafe { read_text(registers.ecx as *const u8, super::MAX_TEXT_BYTES) }
                .unwrap_or_default()
                .to_owned();
            let (name, original_characters, masked) = unsafe { displayed_name(base) };
            let name = if masked {
                &name[..byte_offset(&name, original_characters)]
            } else {
                &name
            };
            let output = substitute_name(&source, name);
            unsafe {
                write_text(registers.edx as *mut u8, SUBSTITUTION_CAPACITY, &output);
            }
            registers.eax = 0;
        }
        2 => {
            unsafe {
                short_mail_filename(base, registers.esi as *mut u8);
            }
            registers.eax = registers.esi;
        }
        _ => return 0,
    }
    1
}

unsafe fn displayed_name(base: usize) -> (String, usize, bool) {
    let get_name: unsafe extern "C" fn() -> *const u8 =
        unsafe { std::mem::transmute(base + 0x008E_4360) };
    let source = unsafe { read_text(get_name(), NAME_CAPACITY) }.unwrap_or_default();
    let original_characters = source.chars().count();
    let escaped = source.replace('%', "％");
    let mut index = unsafe { ptr::read((base + 0x01A3_EF92) as *const u8) } as usize;
    let override_state = unsafe { read_u32(base + 0x0E87_9D90) } as usize;
    if unsafe { read_u32(base + 0x0E86_6CB0) } as i32 >= 2
        || (override_state != 0 && unsafe { read_u32(override_state + 4) } != 0)
    {
        index = 1;
    }
    let masked = unsafe { ptr::read((base + 0x018E_1234 + index) as *const u8) } & 1 != 0;
    if masked {
        let resources = unsafe { read_u32(base + 0x0E77_DCCC) } as usize;
        if resources != 0 {
            let table = unsafe { read_u32(resources + 145 * 4) } as usize;
            if table != 0 {
                // The native table index uses old half/fullwidth byte units.
                let replacement =
                    unsafe { read_u32(table + 1620 + display_columns(&escaped) * 4) } as *const u8;
                if let Some(text) = unsafe { read_text(replacement, NAME_CAPACITY) } {
                    return (text.to_owned(), original_characters, true);
                }
            }
        }
        return (String::new(), original_characters, true);
    }
    (escaped, original_characters, masked)
}

fn substitute_name(source: &str, name: &str) -> String {
    let mut output = String::new();
    for (index, part) in source.split("||").enumerate() {
        for value in (index != 0).then_some(name).into_iter().chain([part]) {
            let prefix = truncate(value, SUBSTITUTION_CAPACITY - 1 - output.len());
            output.push_str(prefix);
            if prefix.len() != value.len() {
                return output;
            }
        }
    }
    output
}

fn countdown_text(ticks: i16) -> String {
    let seconds = (ticks.max(0) / 60).to_string();
    fullwidth::convert(&seconds, 8, true).into_owned()
}

unsafe fn short_mail_filename(base: usize, destination: *mut u8) {
    let params = unsafe { read_u32(base + 0x0E86_6C5C) } as usize;
    if params == 0 || destination.is_null() {
        return;
    }
    let game_dir = unsafe { read_text((params + 20) as *const u8, 0x400) };
    let launcher_dir = unsafe { read_text((params + 1044) as *const u8, 0x400) };
    let Some((game_dir, launcher_dir)) = game_dir.zip(launcher_dir) else {
        unsafe {
            ptr::write(destination, 0);
        }
        return;
    };
    // std's Windows filesystem boundary uses UTF-16, unlike the former A calls.
    let _ = std::env::set_current_dir(game_dir);
    let _ = std::fs::create_dir(format!("{}ショートメール", directory_prefix(game_dir)));
    let now = unsafe { GetLocalTime() };
    let filename = mail_filename(
        launcher_dir,
        [
            now.wYear,
            now.wMonth,
            now.wDay,
            now.wHour,
            now.wMinute,
            now.wSecond,
            now.wMilliseconds,
        ],
    );
    let filename = if filename.len() < MAIL_CAPACITY {
        &filename
    } else {
        ""
    };
    unsafe {
        write_text(destination, MAIL_CAPACITY, filename);
    }
}

fn directory_prefix(directory: &str) -> String {
    if directory.ends_with(['\\', '/']) {
        directory.to_owned()
    } else {
        format!("{directory}\\")
    }
}

fn mail_filename(directory: &str, time: [u16; 7]) -> String {
    let [year, month, day, hour, minute, second, millisecond] = time;
    format!(
        "{}ショートメール\\mhfmail_{year:04}{month:02}{day:02}_{hour:02}{minute:02}{second:02}_{millisecond:03}.txt",
        directory_prefix(directory)
    )
}

fn with_short_mail_open<T>(open: impl FnOnce() -> T) -> T {
    struct Restore(bool);
    impl Drop for Restore {
        fn drop(&mut self) {
            SHORT_MAIL_OPEN.set(self.0);
        }
    }
    let _restore = Restore(SHORT_MAIL_OPEN.replace(true));
    open()
}

unsafe extern "system" fn create_file(
    filename: *const u8,
    access: u32,
    share: u32,
    security: *const SECURITY_ATTRIBUTES,
    disposition: u32,
    flags: u32,
    template: HANDLE,
) -> HANDLE {
    // Preserve LastError across allocation and hook-lease cleanup: the game's
    // CRT queries it immediately when CreateFile returns INVALID_HANDLE_VALUE.
    let (result, error) = {
        let invocation = HOOK_STATE.enter();
        let result = if invocation.state().is_some() && SHORT_MAIL_OPEN.get() {
            if let Some(text) = unsafe { read_text(filename, MAIL_CAPACITY) } {
                let wide = text.encode_utf16().chain([0]).collect::<Vec<_>>();
                let result = unsafe {
                    CreateFileW(
                        wide.as_ptr(),
                        access,
                        share,
                        security,
                        disposition,
                        flags,
                        template,
                    )
                };
                let error = unsafe { GetLastError() };
                drop(wide);
                unsafe {
                    SetLastError(error);
                }
                result
            } else {
                unsafe {
                    SetLastError(1113);
                } // ERROR_NO_UNICODE_TRANSLATION
                INVALID_HANDLE_VALUE
            }
        } else {
            unsafe {
                CreateFileA(
                    filename,
                    access,
                    share,
                    security,
                    disposition,
                    flags,
                    template,
                )
            }
        };
        (result, unsafe { GetLastError() })
    };
    unsafe {
        SetLastError(error);
    }
    result
}

unsafe fn inline_argument(registers: &Registers, index: usize) -> u32 {
    unsafe { read_u32(registers.esp as usize + 4 + index * 4) }
}

unsafe fn read_u32(address: usize) -> u32 {
    unsafe { ptr::read_unaligned(address as *const u32) }
}

unsafe fn read_text<'a>(source: *const u8, capacity: usize) -> Option<&'a str> {
    unsafe { read_z(source, capacity) }.and_then(|text| std::str::from_utf8(text).ok())
}

unsafe fn write_text(destination: *mut u8, capacity: usize, text: &str) {
    unsafe {
        copy_z(
            destination,
            capacity,
            truncate(text, capacity.saturating_sub(1)).as_bytes(),
        );
    }
}

/// Called by the existing real-DLL smoke test while the text hooks are installed.
#[cfg(test)]
pub(in crate::text) unsafe fn verify_short_mail_crt(base: usize) -> Result<(), String> {
    use std::{
        ffi::CString,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    type Open = unsafe extern "C" fn(*const i8, *const i8) -> *mut c_void;
    type Print = unsafe extern "C" fn(*mut c_void, *const i8, ...) -> i32;
    type Close = unsafe extern "C" fn(*mut c_void) -> i32;

    struct Directory(Option<PathBuf>);
    impl Drop for Directory {
        fn drop(&mut self) {
            if let Some(path) = &self.0 {
                let _ = std::fs::remove_dir_all(path);
            }
        }
    }
    struct Stream {
        pointer: *mut c_void,
        close: Close,
    }
    impl Drop for Stream {
        fn drop(&mut self) {
            if !self.pointer.is_null() {
                unsafe {
                    (self.close)(self.pointer);
                }
            }
        }
    }

    if SHORT_MAIL_OPEN.get() {
        return Err("short-mail scope was already active".to_owned());
    }
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| error.to_string())?
        .as_nanos();
    let root = std::env::temp_dir().join(format!("mhf-short-mail-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root)
        .map_err(|error| format!("create isolated CRT test directory: {error}"))?;
    let mut directory = Directory(Some(root.clone()));
    let nested = root.join("短邮件😀");
    std::fs::create_dir(&nested)
        .map_err(|error| format!("create Unicode CRT test directory: {error}"))?;
    let path = nested.join("往返é-𠮷😀.txt");
    let filename = CString::new(
        path.to_str()
            .ok_or_else(|| "temporary path is not Unicode".to_owned())?,
    )
    .map_err(|error| error.to_string())?;
    let payload = c"短邮件 UTF-8：é / 你好 / 𠮷 / 😀\n";

    // The supported DLL links fprintf, not fwrite. This is the same static CRT
    // open/print/close chain used by its real short-mail save operation.
    let open: Open = unsafe { std::mem::transmute(base + 0x015A_BA00) };
    let print: Print = unsafe { std::mem::transmute(base + 0x015A_C5BA) };
    let close: Close = unsafe { std::mem::transmute(base + 0x015A_C03C) };
    let pointer = with_short_mail_open(|| unsafe { open(filename.as_ptr(), c"wb".as_ptr()) });
    if pointer.is_null() {
        return Err(format!(
            "native fopen failed for Unicode path (Win32 error {})",
            unsafe { GetLastError() }
        ));
    }
    let mut stream = Stream { pointer, close };
    if SHORT_MAIL_OPEN.get() {
        return Err("short-mail scope remained active after fopen".to_owned());
    }
    let written = unsafe { print(stream.pointer, c"%s".as_ptr(), payload.as_ptr()) };
    let pointer = std::mem::replace(&mut stream.pointer, ptr::null_mut());
    let closed = unsafe { close(pointer) };
    if written < 0 || written as usize != payload.to_bytes().len() || closed != 0 {
        return Err(format!(
            "native CRT write/close failed: written={written}, close={closed}"
        ));
    }
    let actual = std::fs::read(&path)
        .map_err(|error| format!("read back native CRT Unicode file: {error}"))?;
    if actual != payload.to_bytes() {
        return Err("native CRT changed the UTF-8 file contents".to_owned());
    }
    std::fs::remove_dir_all(&root)
        .map_err(|error| format!("remove isolated CRT test directory: {error}"))?;
    directory.0 = None;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_utf8_literals_fit_the_verified_native_fields() {
        let mut name = [0xabu8; 20];
        unsafe {
            write_text(name.as_mut_ptr().add(1), 18, NAME);
        }
        assert_eq!(&name[1..14], c"アイルー".to_bytes_with_nul());
        assert_eq!(name[0], 0xab);
        assert_eq!(name[19], 0xab);
        let mut percent = [0xabu8; 6];
        unsafe {
            write_text(percent.as_mut_ptr().add(1), 4, "％");
        }
        assert_eq!(&percent[1..5], c"％".to_bytes_with_nul());
        assert_eq!(percent[0], 0xab);
        assert_eq!(percent[5], 0xab);
        unsafe {
            write_text(percent.as_mut_ptr().add(1), 3, "％");
        }
        assert_eq!(percent[1], 0);
    }

    #[test]
    fn countdown_uses_fullwidth_digits_and_display_columns_within_eight_bytes() {
        for (ticks, expected, columns) in [
            (0, "０", 2),
            (9 * 60, "９", 2),
            (30 * 60, "３０", 4),
            (99 * 60, "９９", 4),
            (100 * 60, "100", 3),
        ] {
            let text = countdown_text(ticks);
            assert_eq!(text, expected);
            assert!(text.len() < 8);
            assert_eq!(display_columns(&text), columns);
        }
    }

    #[test]
    fn name_substitution_keeps_percent_expansion_and_complete_unicode() {
        assert_eq!(
            substitute_name("你好||，||！", "é％😀"),
            "你好é％😀，é％😀！"
        );
        let text = substitute_name(&"中".repeat(600), "x");
        assert_eq!(text.len(), 1023);
        assert!(text.chars().all(|character| character == '中'));
        let mask = "●●●";
        assert_eq!(&mask[..byte_offset(mask, 2)], "●●");
    }

    #[test]
    fn short_mail_path_is_utf8_and_retains_native_filename_fields() {
        assert_eq!(
            mail_filename("Z:\\游戏", [2026, 9, 7, 1, 2, 3, 4]),
            "Z:\\游戏\\ショートメール\\mhfmail_20260907_010203_004.txt"
        );
        assert_eq!(directory_prefix("C:\\mhf\\"), "C:\\mhf\\");
    }

    #[test]
    fn short_mail_unicode_scope_is_nested_and_thread_local() {
        assert!(!SHORT_MAIL_OPEN.get());
        with_short_mail_open(|| {
            assert!(SHORT_MAIL_OPEN.get());
            with_short_mail_open(|| assert!(SHORT_MAIL_OPEN.get()));
            assert!(SHORT_MAIL_OPEN.get());
            assert!(!std::thread::spawn(|| SHORT_MAIL_OPEN.get()).join().unwrap());
        });
        assert!(!SHORT_MAIL_OPEN.get());
    }

    unsafe extern "C" fn record_inline_call(registers: *mut Registers, _: u32) -> u32 {
        let registers = unsafe { &mut *registers };
        let output = unsafe { inline_argument(registers, 0) } as *mut u32;
        for index in 1..7 {
            unsafe {
                output
                    .add(index - 1)
                    .write(inline_argument(registers, index));
            }
        }
        unsafe {
            output.add(6).write(registers.ebx);
            output.add(7).write(registers.esi);
            output.add(8).write(registers.edi);
        }
        registers.eax = 0x89ab_cdef;
        registers.esp = checked_continuation as *const () as u32;
        unsafe {
            core::arch::asm!("xorps xmm0, xmm0", "fninit", out("xmm0") _, options(nostack));
        }
        1
    }

    inline_hook!(CHECKED_ORIGINAL, checked_inline, 0, 28, record_inline_call);

    #[unsafe(naked)]
    unsafe extern "C" fn checked_continuation() {
        core::arch::naked_asm!(
            "jnc 2f",
            "cmp eax, 0x89abcdef",
            "jne 2f",
            "cmp ebx, 0x11223344",
            "jne 2f",
            "cmp esi, 0x55667788",
            "jne 2f",
            "cmp edi, 0x99aabbcc",
            "jne 2f",
            "movd edx, xmm0",
            "cmp edx, 0x12345678",
            "jne 2f",
            "sub esp, 4",
            "fstp dword ptr [esp]",
            "mov edx, [esp]",
            "add esp, 4",
            "cmp edx, 0x3f800000",
            "jne 2f",
            "lea edx, [ebp - 12]",
            "cmp esp, edx",
            "jne 2f",
            "mov eax, 1",
            "jmp 3f",
            "2:",
            "xor eax, eax",
            "3:",
            "lea esp, [ebp - 12]",
            "pop edi",
            "pop esi",
            "pop ebx",
            "pop ebp",
            "ret",
        );
    }

    #[unsafe(naked)]
    unsafe extern "C" fn invoke_inline(_target: usize, _output: *mut u32) -> u32 {
        core::arch::naked_asm!(
            "push ebp",
            "mov ebp, esp",
            "push ebx",
            "push esi",
            "push edi",
            "mov ebx, 0x11223344",
            "mov esi, 0x55667788",
            "mov edi, 0x99aabbcc",
            "mov eax, 0x12345678",
            "movd xmm0, eax",
            "fld1",
            "push 66",
            "push 55",
            "push 44",
            "push 33",
            "push 22",
            "push 11",
            "push dword ptr [ebp + 12]",
            "stc",
            "jmp dword ptr [ebp + 8]",
        );
    }

    #[test]
    fn inline_bridge_preserves_registers_flags_fp_and_stdcall_cleanup() {
        let mut output = [0u32; 9];
        let result =
            unsafe { invoke_inline(checked_inline as *const () as usize, output.as_mut_ptr()) };
        assert_eq!(result, 1);
        assert_eq!(
            output,
            [
                11,
                22,
                33,
                44,
                55,
                66,
                0x1122_3344,
                0x5566_7788,
                0x99aa_bbcc
            ]
        );
    }
}
