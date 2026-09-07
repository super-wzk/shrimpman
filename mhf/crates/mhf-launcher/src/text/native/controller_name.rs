//! DirectInput's ANSI device name enters UTF-8 only when writing keyconfig XML.
//! The original 260-byte name remains available for native controller matching.
//! UTF-8 XML restores that ANSI field on load, including with no device attached.

use super::{CodeHook, HOOK_STATE, Registers, read_z, signature};
use std::{ffi::CString, ptr, sync::atomic::AtomicUsize};
use windows::{
    Win32::Globalization::{
        CP_ACP, GetACP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar, WC_NO_BEST_FIT_CHARS,
        WideCharToMultiByte,
    },
    core::{BOOL, PCSTR},
};

const CALL_RVA: usize = 0x008D76EE;
const PRINT_RVA: usize = 0x015AC59C;
const NAME_CAPACITY: usize = 260;
const NAME_ARGUMENT: usize = 14;
type Print = unsafe extern "C" fn(*mut u8, usize, *const u8, ...) -> i32;

const LOAD_CALLS: [usize; 2] = [0x008D658D, 0x008D677E];

pub(super) unsafe fn validate(base: usize, size: usize) -> Result<(), String> {
    // Loader hooks read arguments through EBP and can take its early epilogue.
    for &(rva, bytes) in &[
        (
            0x008D6420,
            &[0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xA8, 0x05, 0, 0][..],
        ),
        (
            0x008D6552,
            &[
                0x5F, 0x5E, 0x5B, 0x8B, 0x4D, 0xF8, 0x33, 0xCD, 0xE8, 0xD6, 0x50, 0xCD, 0, 0x8B,
                0xE5, 0x5D, 0xC3,
            ][..],
        ),
    ] {
        if rva + bytes.len() > size
            || unsafe { std::slice::from_raw_parts((base + rva) as *const u8, bytes.len()) }
                != bytes
        {
            return Err(format!(
                "unsupported controller XML loader at RVA {rva:#010X}"
            ));
        }
    }
    Ok(())
}

macro_rules! call_hook {
    ($original:ident, $detour:ident, $operation:literal) => {
        static $original: AtomicUsize = AtomicUsize::new(0);
        #[unsafe(naked)]
        unsafe extern "C" fn $detour() {
            core::arch::naked_asm!(
                "pushfd", "pushad", "mov eax, esp",
                "sub esp, 528", "and esp, -16", "fxsave [esp]", "mov [esp + 512], eax",
                "push {operation}", "push eax", "call {dispatch}", "add esp, 8",
                "fxrstor [esp]", "mov esp, [esp + 512]", "test eax, eax", "jz 2f",
                "popad", "popfd", "jmp dword ptr [esp - 24]",
                "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                operation = const $operation,
                dispatch = sym dispatch,
                original = sym $original,
            );
        }
    }
}

call_hook!(ORIGINAL, detour, 0);
call_hook!(LOAD_OLD_ORIGINAL, load_old_detour, 1);
call_hook!(LOAD_ORIGINAL, load_detour, 2);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "UTF-8 keyconfig controller name",
            rva: CALL_RVA,
            signature: {
                const SIGNATURE: &[(usize, u8)] = &signature([
                    0xE8, 0xA9, 0x4E, 0xCD, 0x00, 0x8D, 0x85, 0xFC, 0xFB, 0xFF, 0xFF,
                ]);
                SIGNATURE
            },
            detour: detour as *const () as *mut _,
            original: &ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 legacy keyconfig controller ingress",
            rva: LOAD_CALLS[0],
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0xE8, 0x59, 0x64, 0xCD, 0x00, 0x83, 0xC4, 0x0C]);
                SIGNATURE
            },
            detour: load_old_detour as *const () as *mut _,
            original: &LOAD_OLD_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 keyconfig controller ingress",
            rva: LOAD_CALLS[1],
            signature: {
                const SIGNATURE: &[(usize, u8)] =
                    &signature([0xE8, 0x68, 0x62, 0xCD, 0x00, 0x83, 0xC4, 0x0C]);
                SIGNATURE
            },
            detour: load_detour as *const () as *mut _,
            original: &LOAD_ORIGINAL,
        },
    ]
}

unsafe extern "C" fn dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let registers = unsafe { &mut *registers };
    if operation != 0 {
        return unsafe { load_name(state.module_base, registers, operation as usize - 1) };
    }
    // This interrupts CALL, so no return address has been pushed. The original
    // caller leaves all 18 words on its stack and later removes exactly 0x48.
    let arguments =
        unsafe { ptr::read_unaligned((registers.esp as usize + 4) as *const [u32; 18]) };
    let print: Print = unsafe { std::mem::transmute(state.module_base + PRINT_RVA) };
    registers.eax = unsafe { print_controller(print, &arguments, CP_ACP) } as u32;
    // POPAD ignores its ESP slot; the shim uses it to resume after CALL.
    registers.esp = (state.module_base + CALL_RVA + 5) as u32;
    1
}

#[unsafe(naked)]
unsafe extern "C" fn reject_file() {
    // Remove this strcpy_s call's arguments before the original early-return
    // epilogue restores EDI/ESI/EBX and checks its stack cookie. EAX is zero.
    core::arch::naked_asm!("add esp, 12", "jmp ecx");
}

unsafe fn load_name(base: usize, registers: &mut Registers, index: usize) -> u32 {
    // 108D6050 passes (temporary config, raw file buffer, file length). The
    // hand-written parser does not transcode XML attributes itself.
    let file = unsafe { ptr::read_unaligned((registers.ebp as usize + 12) as *const u32) };
    let length = unsafe { ptr::read_unaligned((registers.ebp as usize + 16) as *const u32) };
    if file == 0 || length == 0 || length > 0x80000 {
        return 0;
    }
    let file = unsafe { std::slice::from_raw_parts(file as *const u8, length as usize) };
    if !xml_is_utf8(file) {
        return 0;
    }
    let arguments = unsafe { ptr::read_unaligned((registers.esp as usize + 4) as *const [u32; 3]) };
    let result = unsafe { read_z(arguments[2] as *const u8, 1028) }
        .ok_or("unterminated controller name")
        .and_then(|source| encode_ansi(source, CP_ACP))
        .and_then(|name| {
            if name.len() >= arguments[1] as usize {
                return Err("controller name exceeds the native ANSI buffer");
            }
            unsafe {
                ptr::copy_nonoverlapping(name.as_ptr(), arguments[0] as *mut u8, name.len());
                ptr::write((arguments[0] as *mut u8).add(name.len()), 0);
            }
            Ok(())
        });
    registers.eax = 0;
    registers.esp = match result {
        Ok(()) => (base + LOAD_CALLS[index] + 5) as u32,
        Err(error) => {
            eprintln!(
                "cannot load keyconfig controller name; keeping current configuration: {error}"
            );
            // No global config has been committed at either name-copy site.
            registers.ecx = (base + 0x008D6552) as u32;
            reject_file as *const () as u32
        }
    };
    1
}

fn xml_is_utf8(file: &[u8]) -> bool {
    if file.starts_with(b"\xEF\xBB\xBF") {
        return true;
    }
    let Some(declaration) = file.trim_ascii_start().strip_prefix(b"<?xml") else {
        return false;
    };
    if !declaration.first().is_some_and(u8::is_ascii_whitespace) {
        return false;
    }
    let Some(end) = declaration.windows(2).position(|part| part == b"?>") else {
        return false;
    };
    let declaration = &declaration[..end];
    let Some(start) = declaration.windows(8).position(|part| part == b"encoding") else {
        return false;
    };
    if start == 0 || !declaration[start - 1].is_ascii_whitespace() {
        return false;
    }
    let Some(value) = declaration[start + 8..]
        .trim_ascii_start()
        .strip_prefix(b"=")
    else {
        return false;
    };
    let value = value.trim_ascii_start();
    let Some((&quote @ (b'\'' | b'"'), value)) = value.split_first() else {
        return false;
    };
    let Some(end) = value.iter().position(|&byte| byte == quote) else {
        return false;
    };
    value[..end].eq_ignore_ascii_case(b"UTF-8")
}

fn encode_ansi(source: &[u8], code_page: u32) -> Result<Vec<u8>, &'static str> {
    let text = std::str::from_utf8(source).map_err(|_| "controller name is not valid UTF-8")?;
    let code_page = if code_page == CP_ACP {
        unsafe { GetACP() }
    } else {
        code_page
    };
    if code_page == 65001 || text.is_empty() {
        return Ok(source.to_vec());
    }
    let wide: Vec<u16> = text.encode_utf16().collect();
    let mut replaced = BOOL(0);
    let required = unsafe {
        WideCharToMultiByte(
            code_page,
            WC_NO_BEST_FIT_CHARS,
            &wide,
            None,
            PCSTR::null(),
            Some(&mut replaced),
        )
    };
    if required <= 0 || replaced.as_bool() {
        return Err("controller name cannot be represented in the Windows ANSI code page");
    }
    let mut bytes = vec![0u8; required as usize];
    let written = unsafe {
        WideCharToMultiByte(
            code_page,
            WC_NO_BEST_FIT_CHARS,
            &wide,
            Some(&mut bytes),
            PCSTR::null(),
            Some(&mut replaced),
        )
    };
    if written != required || replaced.as_bool() {
        return Err("controller name cannot be represented in the Windows ANSI code page");
    }
    if ansi_name(&bytes, code_page).to_bytes() != source {
        return Err("controller name does not round-trip through the Windows ANSI code page");
    }
    Ok(bytes)
}

unsafe fn print_controller(print: Print, arguments: &[u32; 18], code_page: u32) -> i32 {
    let source =
        unsafe { read_z(arguments[NAME_ARGUMENT] as *const u8, NAME_CAPACITY) }.unwrap_or_default();
    let name = ansi_name(source, code_page);
    // GUID: Data1, Data2, Data3 and eight separately promoted Data4 bytes;
    // then name, axes, dead_zone and operate_type. Only name changes ownership.
    unsafe {
        print(
            arguments[0] as *mut u8,
            arguments[1] as usize,
            arguments[2] as *const u8,
            arguments[3],
            arguments[4],
            arguments[5],
            arguments[6],
            arguments[7],
            arguments[8],
            arguments[9],
            arguments[10],
            arguments[11],
            arguments[12],
            arguments[13],
            name.as_ptr(),
            arguments[15],
            arguments[16],
            arguments[17],
        )
    }
}

fn ansi_name(source: &[u8], code_page: u32) -> CString {
    let mut wide = [0u16; NAME_CAPACITY];
    // DirectInputA follows the Windows ANSI code page, which need not be 932.
    // Flags 0 replace malformed ANSI sequences; the serialized XML stays UTF-8.
    let written = if source.is_empty() {
        0
    } else {
        unsafe {
            MultiByteToWideChar(
                code_page,
                MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
                source,
                Some(&mut wide),
            )
        }
    };
    let text = if written > 0 {
        String::from_utf16_lossy(&wide[..written as usize])
    } else {
        String::from_utf8_lossy(source).into_owned()
    };
    // read_z excludes NUL, and neither conversion introduces embedded NULs.
    CString::new(text).unwrap_or_default()
}

#[cfg(test)]
pub(in crate::text) unsafe fn verify_controller_name_crt(base: usize) -> Result<(), String> {
    let print: Print = unsafe { std::mem::transmute(base + PRINT_RVA) };
    unsafe { verify_formatter(print) }
}

#[cfg(test)]
unsafe fn verify_formatter(print: Print) -> Result<(), String> {
    use std::ffi::CStr;

    const FORMAT: &CStr = c"\t<controller guid=\"%08X-%04X-%04X-%02X%02X-%02X%02X%02X%02X%02X%02X\" name=\"%s\" axes=\"0x%08X\" dead_zone=\"%d\" operate_type=\"%d\"";
    let mut destination = [0xabu8; 1026];
    // CP932 half-width katakana expands from one byte to three UTF-8 bytes.
    // The maximum 259-byte name plus all other fields still fits Buffer[1024].
    let mut name = [0xA6u8; NAME_CAPACITY];
    name[NAME_CAPACITY - 1] = 0;
    let saved = name;
    let arguments = [
        unsafe { destination.as_mut_ptr().add(1) } as u32,
        1024,
        FORMAT.as_ptr() as u32,
        0x12345678,
        0x1234,
        0xABCD,
        0x12,
        0x34,
        0x56,
        0x78,
        0x9A,
        0xBC,
        0xDE,
        0xF0,
        name.as_ptr() as u32,
        0xFEDCBA98,
        i32::MIN as u32,
        i32::MIN as u32,
    ];
    let written = unsafe { print_controller(print, &arguments, 932) };
    let actual = CStr::from_bytes_until_nul(&destination[1..1025])
        .map_err(|error| format!("unterminated controller XML: {error}"))?
        .to_str()
        .map_err(|error| format!("controller XML is not UTF-8: {error}"))?;
    let expected = format!(
        "\t<controller guid=\"12345678-1234-ABCD-1234-56789ABCDEF0\" name=\"{}\" axes=\"0xFEDCBA98\" dead_zone=\"-2147483648\" operate_type=\"-2147483648\"",
        "ｦ".repeat(NAME_CAPACITY - 1)
    );
    if actual != expected || written as usize != expected.len() {
        return Err("controller serializer changed an argument or truncated the name".to_owned());
    }
    if name != saved || destination[0] != 0xAB || destination[1025] != 0xAB {
        return Err("controller serializer changed source bytes or exceeded capacity".to_owned());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    unsafe extern "C" {
        fn sprintf_s(destination: *mut u8, capacity: usize, format: *const u8, ...) -> i32;
    }

    #[test]
    fn os_names_follow_the_source_code_page() {
        for (page, bytes, expected) in [
            (
                932,
                b"\x83\x52\x83\x93\x83\x67\x83\x8d\x81\x5b\x83\x89".as_slice(),
                "コントローラ",
            ),
            (936, b"\xca\xd6\xb1\xfa".as_slice(), "手柄"),
            (1252, b"Contr\xf4leur".as_slice(), "Contrôleur"),
            (65001, "手柄😀".as_bytes(), "手柄😀"),
            (932, b"".as_slice(), ""),
        ] {
            assert_eq!(ansi_name(bytes, page).to_str().unwrap(), expected);
        }
        assert!(ansi_name(b"\x81", 932).to_str().is_ok());
    }

    #[test]
    fn controller_xml_preserves_arguments_and_fits_the_native_buffer() {
        unsafe { verify_formatter(sprintf_s) }.unwrap();
    }

    #[test]
    fn xml_encoding_is_explicit_and_legacy_files_remain_unchanged() {
        for xml in [
            b"<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\r\n".as_slice(),
            b"<?xml version='1.0' encoding = 'utf-8'?>".as_slice(),
            b"\xEF\xBB\xBF<keyconfig>".as_slice(),
        ] {
            assert!(xml_is_utf8(xml));
        }
        for xml in [
            b"<?xml version=\"1.0\" encoding=\"Shift_JIS\" ?>".as_slice(),
            b"<controller name=\"UTF-8\"/>".as_slice(),
            b"<?xml version=\"1.0\"?>".as_slice(),
            b"<?xml version='1.0' fake_encoding='UTF-8'?>".as_slice(),
        ] {
            assert!(!xml_is_utf8(xml));
        }
    }

    #[test]
    fn xml_names_round_trip_without_best_fit_or_replacement() {
        for (page, name) in [
            (932, "コントローラ"),
            (936, "手柄"),
            (1252, "Contrôleur"),
            (65001, "手柄😀"),
        ] {
            let ansi = encode_ansi(name.as_bytes(), page).unwrap();
            assert_eq!(ansi_name(&ansi, page).to_str().unwrap(), name);
        }
        for name in ["😀", "𠮷", "∞"] {
            assert!(encode_ansi(name.as_bytes(), 1252).is_err());
        }
        assert!(encode_ansi(b"\xFF", 1252).is_err());
    }

    #[test]
    fn invalid_xml_name_rejects_the_file_without_changing_the_match_buffer() {
        let file = b"<?xml version='1.0' encoding='UTF-8'?>";
        let source = b"\xFF\0";
        let mut destination = [0xABu8; NAME_CAPACITY];
        let frame = [0, 0, 0, file.as_ptr() as u32, file.len() as u32];
        let arguments = [
            destination.as_mut_ptr() as u32,
            NAME_CAPACITY as u32,
            source.as_ptr() as u32,
        ];
        let mut registers = Registers {
            edi: 0,
            esi: 0,
            ebp: frame.as_ptr() as u32,
            esp: (arguments.as_ptr() as u32).wrapping_sub(4),
            ebx: 0,
            edx: 0,
            ecx: 0,
            eax: 0,
            flags: 0,
        };
        assert_eq!(unsafe { load_name(0x10000000, &mut registers, 1) }, 1);
        assert_eq!(registers.eax, 0);
        assert_eq!(registers.ecx, 0x108D6552);
        assert_eq!(registers.esp, reject_file as *const () as u32);
        assert_eq!(destination, [0xAB; NAME_CAPACITY]);
    }

    #[unsafe(naked)]
    unsafe extern "C" fn rejection_epilogue() {
        core::arch::naked_asm!(
            "pop edi",
            "pop esi",
            "pop ebx",
            "mov esp, ebp",
            "pop ebp",
            "ret"
        );
    }

    #[unsafe(naked)]
    unsafe extern "C" fn exercise_rejection() -> u32 {
        core::arch::naked_asm!(
            "push ebp", "mov ebp, esp", "push ebx", "push esi", "push edi",
            "push 0x1111", "push 260", "push 0x2222",
            "lea ecx, [{epilogue}]", "mov eax, 0x12345678", "jmp {reject}",
            epilogue = sym rejection_epilogue, reject = sym reject_file,
        );
    }

    #[test]
    fn rejected_file_removes_only_the_three_pending_copy_arguments() {
        assert_eq!(unsafe { exercise_rejection() }, 0x12345678);
    }
}
