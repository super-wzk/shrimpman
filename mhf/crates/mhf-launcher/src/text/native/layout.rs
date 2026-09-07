use super::{MAX_TEXT_BYTES, Registers, copy_character, fullwidth, read_z, strings, tokens};
use crate::text::utf8::{display_columns, truncate};
use std::{ffi::c_void, ptr};
use unicode_segmentation::UnicodeSegmentation;

pub(super) unsafe fn expand(base: usize, source: &[u8]) -> Vec<u8> {
    let mut result = Vec::with_capacity(source.len());
    let mut literal = false;
    let mut offset = 0;
    while offset < source.len() {
        let token = tokens::parse(&source[offset..], literal);
        if token.bytes == 0 {
            break;
        }
        match token.kind {
            6 => {
                let value = unsafe { number(base, token.argument(&source[offset..])) };
                result.extend_from_slice(value.as_bytes());
            }
            16 => {
                let value = unsafe { variable(base, token.argument(&source[offset..])) };
                // The original expands variables directly into a draw buffer,
                // so their braces and tildes were literal, not nested markup.
                result.extend_from_slice(&strings::escape(&value, MAX_TEXT_BYTES, false));
            }
            11 => {
                literal = !literal;
                result.extend_from_slice(&source[offset..offset + token.bytes]);
            }
            _ => result.extend_from_slice(&source[offset..offset + token.bytes]),
        }
        offset += token.bytes;
        if result.len() >= MAX_TEXT_BYTES {
            break;
        }
    }
    result
}

unsafe fn number(base: usize, index: u32) -> String {
    let value = match index {
        0 => unsafe { ptr::read_volatile((base + 0x0E7FE078) as *const i32) }.wrapping_add(1),
        1 => unsafe { ptr::read_volatile((base + 0x0E7FE0A8) as *const i32) },
        _ => return String::new(),
    };
    let text = value.to_string();
    if unsafe { fullwidth::enabled(base) } {
        text.chars().map(fullwidth::character).collect()
    } else {
        text
    }
}

pub(super) unsafe fn variable(base: usize, index: u32) -> Vec<u8> {
    let mut output = [0u8; 1024];
    unsafe {
        call_variable(
            base + 0x00885ED0,
            index,
            output.as_mut_ptr(),
            output.len() as u32,
        );
    }
    let length = output
        .iter()
        .position(|&byte| byte == 0)
        .unwrap_or(output.len() - 1);
    output[..length].to_vec()
}

#[unsafe(naked)]
unsafe extern "C" fn call_variable(
    _target: usize,
    _index: u32,
    _destination: *mut u8,
    _capacity: u32,
) -> u32 {
    core::arch::naked_asm!(
        "push edi",
        "mov edx, [esp + 8]",
        "mov eax, [esp + 12]",
        "mov edi, [esp + 16]",
        "push dword ptr [esp + 20]",
        "call edx",
        "add esp, 4",
        "pop edi",
        "ret",
    );
}

pub(super) fn measure(source: &[u8], narrow: u32, wide: u32) -> u32 {
    let mut maximum = 0u32;
    let mut line = 0u32;
    let mut literal = false;
    let mut offset = 0;
    while offset < source.len() {
        let token = tokens::parse(&source[offset..], literal);
        if token.bytes == 0 {
            break;
        }
        match token.kind {
            2 | 3 => {
                line = line.saturating_add(match token.columns {
                    0 => 0,
                    1 => narrow,
                    _ => wide,
                })
            }
            4 => {
                maximum = maximum.max(line);
                line = 0;
            }
            5 | 12 | 13 => line = line.saturating_add(narrow),
            7 | 9 | 14 | 15 => line = line.saturating_add(wide),
            11 => literal = !literal,
            _ => {}
        }
        offset += token.bytes;
    }
    maximum.max(line)
}

/// These consumers have four bytes for a character, not a general string.
pub(super) unsafe fn display_token(base: usize, registers: &Registers) -> u32 {
    let source_pointer = unsafe { registers.argument(0) } as *const u8;
    let source = unsafe { read_z(source_pointer, MAX_TEXT_BYTES) }.unwrap_or_default();
    let variable_index = unsafe { registers.argument(1) } as usize;
    let next = unsafe { registers.argument(2) } as *mut u32;
    let next_variable = unsafe { registers.argument(3) } as *mut u32;
    let destination = registers.edi as *mut u8;
    let mut offset = 0;
    let mut literal = false;
    loop {
        let token = tokens::parse(&source[offset..], literal);
        let mut advance = token.bytes;
        let mut next_index = 0;
        let kind = match token.kind {
            0 => {
                unsafe { copy_character(destination, &[], 0) };
                4
            }
            1 | 8 | 10 => {
                offset += advance;
                continue;
            }
            11 => {
                literal = !literal;
                offset += advance;
                continue;
            }
            2..=4 => {
                unsafe { copy_character(destination, &source[offset..], advance) };
                u32::from(token.columns > 1)
            }
            5 | 12 | 13 => {
                let byte = match token.kind {
                    5 => b'~',
                    12 => b'{',
                    _ => b'}',
                };
                unsafe { copy_character(destination, &[byte], 1) };
                0
            }
            6 | 7 | 9 | 14 | 15 => {
                unsafe {
                    copy_character(destination, &[token.argument(&source[offset..]) as u8], 1)
                };
                if token.kind == 6 { 3 } else { 2 }
            }
            16 => {
                let value = unsafe { variable(base, token.argument(&source[offset..])) };
                let text = String::from_utf8_lossy(&value);
                let mut chars = text.graphemes(true);
                let character = chars.nth(variable_index);
                if let Some(character) = character {
                    unsafe { copy_character(destination, character.as_bytes(), character.len()) };
                    if chars.next().is_some() {
                        advance = 0;
                        next_index = variable_index + 1;
                    }
                    u32::from(display_columns(character) > 1)
                } else {
                    unsafe { copy_character(destination, &[], 0) };
                    0
                }
            }
            _ => {
                offset += advance;
                continue;
            }
        };
        if !next.is_null() {
            unsafe { ptr::write_unaligned(next, source_pointer.add(offset + advance) as u32) }
        }
        if !next_variable.is_null() {
            unsafe { ptr::write_unaligned(next_variable, next_index as u32) }
        }
        return kind;
    }
}

pub(super) unsafe fn variable_character(
    base: usize,
    index: u32,
    character_index: usize,
    destination: *mut u8,
) -> u32 {
    let value = unsafe { variable(base, index) };
    let text = String::from_utf8_lossy(&value);
    if let Some(character) = text.graphemes(true).nth(character_index) {
        unsafe { copy_character(destination, character.as_bytes(), character.len()) };
    } else {
        unsafe { copy_character(destination, &[], 0) }
    }
    text.graphemes(true).count() as u32
}

#[unsafe(naked)]
pub(super) unsafe extern "C" fn call_render(
    _target: usize,
    _source: *const u8,
    _limit: u32,
) -> u32 {
    core::arch::naked_asm!(
        "mov edx, [esp + 4]",
        "mov ecx, [esp + 8]",
        "push dword ptr [esp + 12]",
        "call edx",
        "add esp, 4",
        "ret",
    );
}

pub(super) unsafe fn context(base: usize) -> usize {
    unsafe { ptr::read_volatile((base + 0x0E3CBD64) as *const u32) as usize }
}

pub(super) unsafe fn formatted(base: usize, registers: &Registers, wrap: bool) -> u32 {
    // Both native entrypoints skip formatting and all draw-state changes here.
    if unsafe { ptr::read_volatile((base + 0x0ECB1704) as *const u32) } != 0 {
        return if wrap {
            (unsafe { registers.argument(0) }) as u16 as u32
        } else {
            0
        };
    }
    type Format = unsafe extern "C" fn(*mut u8, usize, *const u8, *const c_void) -> i32;
    let formatter = unsafe { std::mem::transmute::<usize, Format>(base + 0x015AD820) };
    let mut output = [0u8; 1024];
    let format = unsafe { registers.argument(1) } as *const u8;
    if format.is_null() {
        return 0;
    }
    unsafe {
        formatter(
            output.as_mut_ptr(),
            output.len(),
            format,
            (registers.esp as usize + 16) as *const c_void,
        );
    }
    output[1023] = 0;
    let source = unsafe { read_z(output.as_ptr(), output.len()) }.unwrap_or_default();
    let source = String::from_utf8_lossy(source);
    let context = unsafe { context(base) };
    if context == 0 {
        return 0;
    }
    let initial_x = unsafe { ptr::read_unaligned((context + 12) as *const u16) };
    let maximum = unsafe { registers.argument(0) } as i16;
    let mut remaining = maximum;
    let mut line = String::new();
    for character in source.graphemes(true) {
        if character == "\n" || character == "\r\n" {
            unsafe {
                queue(base, line.as_bytes());
                advance_line(context, initial_x);
            }
            line.clear();
            continue;
        }
        if maximum == 0 || (!wrap && remaining == 0) {
            break;
        }
        // The queue owns its copied bytes; emitting before the native 1024-byte
        // formatting buffer fills keeps multi-byte scalars intact.
        if line.len() + character.len() >= 1024 {
            break;
        }
        line.push_str(character);
        if remaining > 0 {
            remaining -= 1
        }
        if wrap && remaining == 0 {
            unsafe {
                queue(base, line.as_bytes());
                advance_line(context, initial_x);
            }
            line.clear();
            remaining = maximum;
        }
    }
    unsafe { queue(base, line.as_bytes()) }
}

pub(super) unsafe fn queue(base: usize, source: &[u8]) -> u32 {
    type Queue = unsafe extern "C" fn(*const u8) -> u32;
    let queue = unsafe { std::mem::transmute::<usize, Queue>(base + 0x014DEA50) };
    let mut source = source.to_vec();
    source.push(0);
    unsafe { queue(source.as_ptr()) }
}

unsafe fn advance_line(context: usize, x: u16) {
    let height = unsafe { ptr::read((context + 25) as *const u8) } as u16;
    let spacing = if unsafe { ptr::read((context + 26) as *const u8) } != 0 {
        (unsafe { ptr::read((context + 27) as *const u8) }) as u16
    } else {
        0
    };
    let y = unsafe { ptr::read_unaligned((context + 14) as *const u16) };
    unsafe {
        ptr::write_unaligned((context + 12) as *mut u16, x);
        ptr::write_unaligned(
            (context + 14) as *mut u16,
            y.wrapping_add(height).wrapping_add(spacing),
        );
    }
}

pub(super) fn render_input(source: &[u8]) -> Vec<u8> {
    let source = String::from_utf8_lossy(source);
    let mut source = truncate(&source, 1023).as_bytes().to_vec();
    source.push(0);
    source
}

#[cfg(test)]
mod tests {
    use super::{measure, render_input};

    #[test]
    fn measurements_ignore_controls_and_keep_unicode_width() {
        assert_eq!(measure("~C83A中~C00\n😀e\u{301}".as_bytes(), 1, 2), 3);
        assert_eq!(measure("ｶ中{I1}".as_bytes(), 10, 20), 50);
        assert_eq!(measure(b"~%~C83~%", 1, 2), 4);
    }

    #[test]
    fn renderer_bound_reserves_nul_and_a_whole_last_character() {
        let text = format!("{}😀", "a".repeat(1021));
        let bounded = render_input(text.as_bytes());
        assert_eq!(bounded.len(), 1022);
        assert_eq!(bounded.last(), Some(&0));
        assert!(std::str::from_utf8(&bounded).is_ok());
    }

    #[test]
    fn suppressed_formatting_does_not_read_format_or_render_context() {
        use super::{Registers, formatted};
        use std::ptr;
        use windows::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAlloc, VirtualFree,
        };
        // Reserve the image range, but commit only the suppression flag's page.
        let module = unsafe { VirtualAlloc(None, 0x0ECB2000, MEM_RESERVE, PAGE_READWRITE) };
        assert!(!module.is_null());
        struct Image(*mut std::ffi::c_void);
        impl Drop for Image {
            fn drop(&mut self) {
                unsafe { VirtualFree(self.0, 0, MEM_RELEASE) }.unwrap();
            }
        }
        let _image = Image(module);
        let flag = unsafe { module.cast::<u8>().add(0x0ECB1704) }.cast::<u32>();
        assert!(
            !unsafe { VirtualAlloc(Some(flag.cast()), 4, MEM_COMMIT, PAGE_READWRITE) }.is_null()
        );
        unsafe { ptr::write(flag, 1) };
        let stack = [0u32, 0, 0xABCD1234, 1];
        let registers = Registers {
            edi: 0,
            esi: 0,
            ebp: 0,
            esp: stack.as_ptr() as u32,
            ebx: 0,
            edx: 0,
            ecx: 0,
            eax: 0,
            flags: 0,
        };
        assert_eq!(unsafe { formatted(module as usize, &registers, false) }, 0);
        assert_eq!(
            unsafe { formatted(module as usize, &registers, true) },
            0x1234
        );
    }
}
