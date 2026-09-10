//! Preserve native printf numeric formatting and FILE ownership, while making
//! narrow string field widths count Unicode display columns. Precision retains
//! the C ABI's byte read limit and truncates at a valid UTF-8 boundary.

use std::{
    ffi::{CStr, CString, c_void},
    ptr,
    sync::atomic::{AtomicUsize, Ordering},
};

use super::{CodeHook, HOOK_STATE, MAX_TEXT_BYTES, signature};
use crate::provider::utf8::display_columns;

type Output = unsafe extern "C" fn(*mut c_void, *const u8, *mut c_void, *const u32) -> i32;
static OUTPUT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static SAFE_OUTPUT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
const SIGNATURE: &[(usize, u8)] =
    &signature([0x8B, 0xFF, 0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x80, 0x02, 0, 0]);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "UTF-8 printf string columns",
            rva: 0x015B268F,
            signature: SIGNATURE,
            detour: output_hook as *const () as *mut _,
            original: &OUTPUT_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 secure printf string columns",
            rva: 0x015BA92E,
            signature: SIGNATURE,
            detour: safe_output_hook as *const () as *mut _,
            original: &SAFE_OUTPUT_ORIGINAL,
        },
    ]
}

unsafe extern "C" fn output_hook(
    stream: *mut c_void,
    format: *const u8,
    locale: *mut c_void,
    arguments: *const u32,
) -> i32 {
    unsafe { output(&OUTPUT_ORIGINAL, stream, format, locale, arguments) }
}

unsafe extern "C" fn safe_output_hook(
    stream: *mut c_void,
    format: *const u8,
    locale: *mut c_void,
    arguments: *const u32,
) -> i32 {
    unsafe { output(&SAFE_OUTPUT_ORIGINAL, stream, format, locale, arguments) }
}

unsafe fn output(
    original: &AtomicUsize,
    stream: *mut c_void,
    format: *const u8,
    locale: *mut c_void,
    arguments: *const u32,
) -> i32 {
    let invocation = HOOK_STATE.enter();
    let original: Output = unsafe { std::mem::transmute(original.load(Ordering::Acquire)) };
    let rewritten = if invocation.state().is_some() && !format.is_null() && !arguments.is_null() {
        unsafe { CStr::from_ptr(format.cast()) }
            .to_str()
            .ok()
            .and_then(|format| unsafe { rewrite(format, arguments) })
    } else {
        None
    };
    match rewritten {
        Some(rewritten) => unsafe {
            original(
                stream,
                rewritten.format.as_ptr().cast(),
                locale,
                rewritten.arguments.as_ptr(),
            )
        },
        None => unsafe { original(stream, format, locale, arguments) },
    }
}

struct Rewritten {
    format: CString,
    arguments: Vec<u32>,
    // Keep every replaced string alive through the original formatter call.
    _strings: Vec<CString>,
}

#[derive(Clone, Copy)]
enum Amount {
    Value(i32),
    Argument,
}

struct Spec {
    start: usize,
    end: usize,
    width: Option<Amount>,
    precision: Option<Amount>,
    left: bool,
    narrow_string: bool,
    words: usize,
}

fn amount(bytes: &[u8], index: &mut usize) -> Option<Amount> {
    if bytes.get(*index) == Some(&b'*') {
        *index += 1;
        return Some(Amount::Argument);
    }
    let mut value = 0i32;
    while let Some(byte) = bytes.get(*index).filter(|b| b.is_ascii_digit()) {
        value = value
            .checked_mul(10)?
            .checked_add(i32::from(*byte - b'0'))?;
        *index += 1;
    }
    Some(Amount::Value(value))
}

fn specifications(format: &str) -> Option<Vec<Spec>> {
    let bytes = format.as_bytes();
    let mut result = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'%' {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        if bytes.get(index) == Some(&b'%') {
            index += 1;
            continue;
        }
        let mut left = false;
        while let Some(byte) = bytes.get(index).filter(|b| b"-+ #0".contains(b)) {
            left |= *byte == b'-';
            index += 1;
        }
        let width = if bytes
            .get(index)
            .is_some_and(|b| b.is_ascii_digit() || *b == b'*')
        {
            Some(amount(bytes, &mut index)?)
        } else {
            None
        };
        let precision = if bytes.get(index) == Some(&b'.') {
            index += 1;
            Some(amount(bytes, &mut index)?)
        } else {
            None
        };
        let length_start = index;
        for length in [
            b"I64".as_slice(),
            b"I32",
            b"ll",
            b"hh",
            b"h",
            b"l",
            b"L",
            b"F",
            b"N",
            b"w",
            b"I",
        ] {
            if bytes[index..].starts_with(length) {
                index += length.len();
                break;
            }
        }
        let length = &bytes[length_start..index];
        let specifier = *bytes.get(index)?;
        if !b"diuoxXaAeEfgGcCsSpnZ".contains(&specifier)
            || (length == b"I" && !b"diuoxX".contains(&specifier))
        {
            return None;
        }
        index += 1;
        let words = if b"aAeEfgG".contains(&specifier)
            || (b"diuoxXp".contains(&specifier) && [b"I64".as_slice(), b"ll"].contains(&length))
        {
            2
        } else {
            1
        };
        result.push(Spec {
            start,
            end: index,
            width,
            precision,
            left,
            narrow_string: (specifier == b's' && length != b"l" && length != b"w")
                || (specifier == b'S' && (length == b"h" || length == b"hh")),
            words,
        });
    }
    Some(result)
}

unsafe fn rewrite(format: &str, arguments: *const u32) -> Option<Rewritten> {
    // Numeric formats and ordinary %s calls need no Unicode layout work.
    if !format.bytes().any(|byte| matches!(byte, b's' | b'S'))
        || !format
            .bytes()
            .any(|byte| byte.is_ascii_digit() || matches!(byte, b'*' | b'.'))
    {
        return None;
    }
    let specs = specifications(format)?;
    if !specs
        .iter()
        .any(|s| s.narrow_string && (s.width.is_some() || s.precision.is_some()))
    {
        return None;
    }
    let mut output = String::with_capacity(format.len());
    let mut copied = Vec::new();
    let mut strings = Vec::new();
    let mut cursor = 0;
    let mut word = 0;
    for spec in specs {
        output.push_str(&format[cursor..spec.start]);
        let start_word = word;
        let mut resolve = |amount: Option<Amount>| {
            amount.map(|amount| match amount {
                Amount::Value(value) => value,
                Amount::Argument => {
                    let value = unsafe { ptr::read_unaligned(arguments.add(word)) } as i32;
                    word += 1;
                    value
                }
            })
        };
        let width = resolve(spec.width);
        let precision = resolve(spec.precision);
        let value_word = word;
        word += spec.words;
        if spec.narrow_string && (width.is_some() || precision.is_some()) {
            let width = width.unwrap_or(0);
            let left = spec.left || width < 0;
            let width = width.unsigned_abs() as usize;
            let precision = precision.filter(|p| *p >= 0).map(|p| p as usize);
            if width > MAX_TEXT_BYTES || precision.is_some_and(|p| p > MAX_TEXT_BYTES) {
                return None;
            }
            let source = unsafe { ptr::read_unaligned(arguments.add(value_word)) } as *const u8;
            let text = unsafe { string_argument(source, precision) }?;
            let text = CString::new(field(text, width, left)).ok()?;
            copied.push(text.as_ptr() as u32);
            strings.push(text);
            output.push_str("%s");
        } else {
            output.push_str(&format[spec.start..spec.end]);
            for index in start_word..word {
                copied.push(unsafe { ptr::read_unaligned(arguments.add(index)) });
            }
        }
        cursor = spec.end;
    }
    output.push_str(&format[cursor..]);
    Some(Rewritten {
        format: CString::new(output).ok()?,
        arguments: copied,
        _strings: strings,
    })
}

unsafe fn string_argument<'a>(source: *const u8, precision: Option<usize>) -> Option<&'a str> {
    if precision == Some(0) {
        return Some("");
    }
    if source.is_null() {
        return Some(&"(null)"[..precision.unwrap_or(6).min(6)]);
    }
    let Some(maximum) = precision else {
        return unsafe { CStr::from_ptr(source.cast()) }.to_str().ok();
    };
    // %.Ns may refer to exactly N non-NUL bytes. Conversely, an earlier NUL
    // may terminate an allocation shorter than N, so do not first create an
    // N-byte Rust slice and then search it.
    let mut length = 0;
    while length < maximum && unsafe { ptr::read(source.add(length)) } != 0 {
        length += 1;
    }
    let bytes = unsafe { std::slice::from_raw_parts(source, length) };
    match std::str::from_utf8(bytes) {
        Ok(text) => Some(text),
        Err(error) if error.error_len().is_none() => {
            std::str::from_utf8(&bytes[..error.valid_up_to()]).ok()
        }
        Err(_) => None,
    }
}

fn field(text: &str, width: usize, left: bool) -> String {
    let padding = " ".repeat(width.saturating_sub(display_columns(text)));
    if left {
        format!("{text}{padding}")
    } else {
        format!("{padding}{text}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn string_precision_keeps_byte_bounds_and_padding_counts_display_columns() {
        let prefix = unsafe { string_argument(c"中文A".as_ptr().cast(), Some(4)) }.unwrap();
        assert_eq!(prefix, "中");
        assert_eq!(field(prefix, 6, true), "中    ");
        assert_eq!(field("e\u{301}", 4, false), "   e\u{301}");
        assert_eq!(field("👩‍💻", 4, true), "👩‍💻  ");
    }

    #[test]
    fn rewritten_varargs_keep_numeric_words_stars_and_later_arguments() {
        let text = c"中文A";
        let args = [
            6u32,
            6,
            text.as_ptr() as u32,
            0x89ABCDEF,
            0x01234567,
            2,
            0,
            0x40000000,
        ];
        let result = unsafe { rewrite("%-*.*s:%I64x:%.*f:%%", args.as_ptr()) }.unwrap();
        assert_eq!(result.format.to_str().unwrap(), "%s:%I64x:%.*f:%%");
        assert_eq!(&result.arguments[1..], &args[3..]);
        assert_eq!(result._strings[0].to_str().unwrap(), "中文  ");
        assert!(unsafe { rewrite("%s %d", args.as_ptr()) }.is_none());
        for format in [
            "%jd/%4s", "%zd/%4s", "%td/%4s", "%F/%4s", "%Is/%4s", "%Ip/%4s",
        ] {
            assert!(specifications(format).is_none());
        }
        let args = [0x89ABCDEF, 0x01234567, text.as_ptr() as u32];
        let result = unsafe { rewrite("%I64p/%6s", args.as_ptr()) }.unwrap();
        assert_eq!(&result.arguments[..2], &args[..2]);
        assert_eq!(result._strings[0].to_str().unwrap(), " 中文A");
    }

    #[test]
    fn precision_does_not_read_past_an_unterminated_buffer_or_read_zero_bytes() {
        use windows::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_PROTECTION_FLAGS,
            PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualProtect,
        };
        let page = unsafe { VirtualAlloc(None, 8192, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) };
        assert!(!page.is_null());
        struct Pages(*mut std::ffi::c_void);
        impl Drop for Pages {
            fn drop(&mut self) {
                unsafe { VirtualFree(self.0, 0, MEM_RELEASE) }.unwrap();
            }
        }
        let _pages = Pages(page);
        let guard = unsafe { page.cast::<u8>().add(4096) };
        let mut old = PAGE_PROTECTION_FLAGS::default();
        unsafe { VirtualProtect(guard.cast(), 4096, PAGE_NOACCESS, &mut old) }.unwrap();
        let source = unsafe { guard.sub(2) };
        unsafe { ptr::copy_nonoverlapping(b"ab".as_ptr(), source, 2) };
        let result = unsafe { rewrite("%.2s", [source as u32].as_ptr()) }.unwrap();
        assert_eq!(result._strings[0].to_bytes(), b"ab");
        let result = unsafe { rewrite("%.0s", [guard as u32].as_ptr()) }.unwrap();
        assert!(result._strings[0].is_empty());
    }
}
