//! Replacements for the client's independently compiled multibyte helpers.
//! The x86 shims preserve the original register and caller-cleaned stack ABI.

use super::{CodeHook, HOOK_STATE};
use crate::text::utf8::{char_columns, decode_first, display_columns, truncate};
use std::{
    ptr, str,
    sync::atomic::{AtomicUsize, Ordering},
};
use unicode_segmentation::UnicodeSegmentation;

mod controller_name;
mod crt;
mod embedded;
mod filter;
mod formatting;
mod fullwidth;
mod inline;
mod layout;
mod padding;
mod paths;
mod script;
mod strings;
mod tokens;

const MAX_TEXT_BYTES: usize = 1024 * 1024;

pub(super) unsafe fn validate(base: usize, size: usize) -> Result<(), String> {
    unsafe { fullwidth::validate(base, size) }?;
    unsafe { padding::validate(base, size) }?;
    unsafe { controller_name::validate(base, size) }
}

#[cfg(test)]
pub(super) use controller_name::verify_controller_name_crt;
#[cfg(test)]
pub(super) use embedded::verify_short_mail_crt;

/// PUSHAD's ESP points at the flags saved immediately before PUSHAD.
#[repr(C)]
struct Registers {
    edi: u32,
    esi: u32,
    ebp: u32,
    esp: u32,
    ebx: u32,
    edx: u32,
    ecx: u32,
    eax: u32,
    flags: u32,
}

impl Registers {
    unsafe fn argument(&self, index: usize) -> u32 {
        unsafe { ptr::read_unaligned((self.esp as usize + 8 + index * 4) as *const u32) }
    }
}

const fn signature<const N: usize>(bytes: [u8; N]) -> [(usize, u8); N] {
    let mut result = [(0, 0); N];
    let mut index = 0;
    while index < N {
        result[index] = (index, bytes[index]);
        index += 1;
    }
    result
}

macro_rules! define_hooks {
    ($(($original:ident, $detour:ident, $id:literal, $name:literal, $rva:literal, [$($byte:literal),+])),+ $(,)?) => {
        $(
            static $original: AtomicUsize = AtomicUsize::new(0);
            #[unsafe(naked)]
            unsafe extern "C" fn $detour() {
                core::arch::naked_asm!(
                    "pushfd", "pushad", "mov eax, esp", "push {id}", "push eax",
                    "call {dispatch}", "add esp, 8", "test eax, eax", "jz 2f",
                    "popad", "popfd", "ret",
                    "2:", "popad", "popfd", "jmp dword ptr [{original}]",
                    id = const $id,
                    dispatch = sym dispatch,
                    original = sym $original,
                );
            }
        )+
        pub(super) fn code_hooks() -> Vec<CodeHook> {
            let mut hooks = vec![$(CodeHook {
                name: $name, rva: $rva, signature: {
                    const SIGNATURE: &[(usize, u8)] = &signature([$($byte),+]);
                    SIGNATURE
                },
                detour: $detour as *const () as *mut _, original: &$original,
            }),+];
            hooks.extend(inline::code_hooks());
            hooks.extend(crt::code_hooks());
            hooks.extend(embedded::code_hooks());
            hooks.extend(padding::code_hooks());
            hooks.extend(formatting::code_hooks());
            hooks.extend(controller_name::code_hooks());
            hooks
        }
    }
}

define_hooks!(
    (
        SCREENSHOT_TARGET,
        screenshot_detour,
        35,
        "UTF-8 screenshot filename",
        0x015928D0,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x18, 0x08, 0x00, 0x00]
    ),
    (
        TOKEN_TARGET,
        token_detour,
        0,
        "UTF-8 markup tokenizer",
        0x00885800,
        [0x53, 0x8A, 0x19, 0x0F, 0xBE, 0xD3, 0x56, 0x83]
    ),
    (
        LITERAL_TARGET,
        literal_detour,
        1,
        "UTF-8 literal tokenizer",
        0x00885C20,
        [0x8A, 0x0A, 0x84, 0xC9, 0x74, 0x7E, 0x80, 0xF9]
    ),
    (
        ESCAPE_TARGET,
        escape_detour,
        2,
        "UTF-8 markup escaping",
        0x00885D50,
        [0x55, 0x8B, 0xEC, 0x51, 0x56, 0x8B, 0xF0, 0x8B]
    ),
    (
        COLOR_TARGET,
        color_detour,
        3,
        "UTF-8 color removal",
        0x00885E50,
        [0x55, 0x8B, 0xEC, 0x56, 0x8B, 0xF0, 0x8B, 0x45]
    ),
    (
        UNFORMAT_TARGET,
        unformat_detour,
        4,
        "UTF-8 markup removal",
        0x00986020,
        [0x53, 0x8A, 0x1A, 0x56, 0x8B, 0xF0, 0x84, 0xDB]
    ),
    (
        SUBSTITUTE_TARGET,
        substitute_detour,
        5,
        "UTF-8 template substitution",
        0x0071A3E0,
        [0x55, 0x8B, 0xEC, 0x56, 0x57, 0x8B, 0x7D, 0x08]
    ),
    (
        LABEL_TARGET,
        label_detour,
        6,
        "UTF-8 short label truncation",
        0x00990730,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x3C]
    ),
    (
        PERCENT_TARGET,
        percent_detour,
        7,
        "UTF-8 percent escaping",
        0x00B71270,
        [0x55, 0x8B, 0xEC, 0x51, 0x56, 0x8B, 0xF0, 0x8B]
    ),
    (
        NEWLINES_TARGET,
        newlines_detour,
        8,
        "UTF-8 line count",
        0x00B72930,
        [0x55, 0x8B, 0xEC, 0x51, 0x56, 0xC7, 0x45, 0xFC]
    ),
    (
        NEXT_TARGET,
        next_detour,
        9,
        "UTF-8 next display character",
        0x00B73090,
        [0x55, 0x8B, 0xEC, 0x51, 0x8B, 0x4D, 0x08, 0x8A]
    ),
    (
        WIDE_TARGET,
        wide_detour,
        10,
        "UTF-8 wide character boundary",
        0x00B74830,
        [0x55, 0x8B, 0xEC, 0x51, 0x8B, 0x55, 0x08, 0x53]
    ),
    (
        COUNT_TARGET,
        count_detour,
        11,
        "UTF-8 scalar count",
        0x00974D00,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08, 0x53, 0x56]
    ),
    (
        VALID_TARGET,
        valid_detour,
        12,
        "UTF-8 string validation",
        0x0156E890,
        [0x33, 0xC0, 0x85, 0xC9, 0x0F, 0x84, 0x9E, 0x00]
    ),
    (
        PRINTABLE_TARGET,
        printable_detour,
        13,
        "UTF-8 printable string validation",
        0x0156E940,
        [0x55, 0x8B, 0xEC, 0x33, 0xC0, 0x57, 0x8B, 0x7D]
    ),
    (
        PREFIX_TARGET,
        prefix_detour,
        14,
        "UTF-8 truncation boundary",
        0x0156E650,
        [0x55, 0x8B, 0xEC, 0x8A, 0x0F, 0x33, 0xC0, 0x53]
    ),
    (
        SHORTEN_TARGET,
        shorten_detour,
        15,
        "UTF-8 suffix truncation",
        0x0156E520,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08, 0x8B, 0x55]
    ),
    (
        RESTRICT_TARGET,
        restrict_detour,
        16,
        "UTF-8 field restrictions",
        0x0156E250,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x14, 0x53, 0x8B]
    ),
    (
        SPACES_TARGET,
        spaces_detour,
        17,
        "UTF-8 leading spaces",
        0x0156E4C0,
        [0x55, 0x8B, 0xEC, 0x51, 0x53, 0x56, 0x8B, 0x75]
    ),
    (
        FULLWIDTH_TARGET,
        fullwidth_detour,
        18,
        "UTF-8 fullwidth conversion",
        0x014DF610,
        [0x55, 0x8B, 0xEC, 0x8B, 0x15]
    ),
    (
        RENDER_TARGET,
        render_detour,
        19,
        "UTF-8 expanded markup rendering",
        0x00B71370,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x60, 0x04, 0x00]
    ),
    (
        COLUMNS_TARGET,
        columns_detour,
        20,
        "UTF-8 markup columns",
        0x00B72180,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x18, 0x04, 0x00]
    ),
    (
        PIXELS_TARGET,
        pixels_detour,
        21,
        "UTF-8 markup pixel width",
        0x00B72680,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x1C, 0x04, 0x00]
    ),
    (
        CENTER_TARGET,
        center_detour,
        22,
        "UTF-8 text centering",
        0x015A1630,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x9C, 0x04, 0x00]
    ),
    (
        MULTILINE_TARGET,
        multiline_detour,
        23,
        "UTF-8 maximum line width",
        0x00B723B0,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0xA0, 0x04, 0x00]
    ),
    (
        VARIABLE_WIDTH_TARGET,
        variable_width_detour,
        24,
        "UTF-8 variable width",
        0x00886110,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x84, 0x00, 0x00]
    ),
    (
        VARIABLE_CHAR_TARGET,
        variable_char_detour,
        25,
        "UTF-8 variable character",
        0x008861C0,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x88, 0x00, 0x00]
    ),
    (
        DISPLAY_TOKEN_TARGET,
        display_token_detour,
        26,
        "UTF-8 next markup character",
        0x00B729A0,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x0C, 0x53, 0x8B]
    ),
    (
        PLAIN_TARGET,
        plain_detour,
        27,
        "UTF-8 expanded plain text",
        0x00B72010,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x08, 0x8B, 0x45]
    ),
    (
        FORMAT_TARGET,
        format_detour,
        28,
        "UTF-8 formatted text limit",
        0x00B73110,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x20, 0x08, 0x00]
    ),
    (
        FORMAT_WRAP_TARGET,
        format_wrap_detour,
        29,
        "UTF-8 formatted text wrapping",
        0x00B73390,
        [0x55, 0x8B, 0xEC, 0x81, 0xEC, 0x20, 0x08, 0x00]
    ),
    (
        FILTER_TARGET,
        filter_detour,
        30,
        "UTF-8 native word filtering",
        0x0156E080,
        [0x55, 0x8B, 0xEC, 0x83, 0xEC, 0x10, 0x56, 0x8B]
    ),
    (
        SCRIPT_TARGET,
        script_detour,
        31,
        "UTF-8 scripted text",
        0x0048DE10,
        [0x55, 0x8B, 0xEC, 0x53, 0x56, 0x8B, 0xF0, 0x0F]
    ),
    (
        ASCII_TARGET,
        ascii_detour,
        32,
        "UTF-8 ASCII field test",
        0x003D1640,
        [0x55, 0x8B, 0xEC, 0x51, 0x8A, 0x02, 0x53, 0x56]
    ),
    (
        CODE_TARGET,
        code_detour,
        33,
        "UTF-8 six-character code validation",
        0x005C3620,
        [0x55, 0x8B, 0xEC, 0x51, 0x53, 0x57]
    ),
    (
        TOOLTIP_TARGET,
        tooltip_detour,
        34,
        "UTF-8 tooltip dimensions",
        0x007E4270,
        [0x55, 0x8B, 0xEC, 0x51, 0x57, 0x8B, 0x7D, 0x08]
    ),
);

unsafe extern "C" fn dispatch(registers: *mut Registers, operation: u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    // The invocation retains both the DLL and its hook state until every native
    // read and callback below has completed.
    let registers = unsafe { &mut *registers };
    let result = unsafe {
        match operation {
            0 | 1 => {
                let source = if operation == 0 {
                    registers.ecx
                } else {
                    registers.edx
                };
                let source = read_z(source as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let token = tokens::parse(source, operation == 1);
                if registers.eax != 0 {
                    ptr::write_unaligned(registers.eax as *mut u32, token.bytes as u32)
                }
                token.kind
            }
            2 | 7 => {
                let source =
                    read_z(registers.argument(0) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let capacity = if operation == 7 {
                    128
                } else {
                    registers.eax as usize
                };
                let destination = if operation == 7 {
                    registers.eax
                } else {
                    registers.ecx
                };
                let escaped = strings::escape(source, capacity.min(MAX_TEXT_BYTES), operation == 7);
                copy_z(destination as *mut u8, capacity, &escaped);
                0
            }
            3 | 4 => {
                let source = if operation == 3 {
                    registers.argument(0)
                } else {
                    registers.edx
                };
                let source = read_z(source as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let result = strings::unformat(source, operation == 3);
                copy_z(registers.eax as *mut u8, source.len() + 1, &result);
                0
            }
            5 => {
                let source = read_z(registers.eax as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let template =
                    read_z(registers.argument(0) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let mut output = Vec::new();
                if let Some(index) = template.iter().rposition(|&byte| byte == b'$') {
                    output.extend_from_slice(&template[..index]);
                    for &byte in source {
                        output.push(byte);
                        if byte == b'~' {
                            output.push(b'~');
                        }
                    }
                    output.extend_from_slice(&template[index + 1..]);
                } else {
                    output.extend_from_slice(template)
                }
                let text = String::from_utf8_lossy(&output);
                let destination = state.module_base + 0x0EE089E0;
                copy_z(destination as *mut u8, 256, truncate(&text, 255).as_bytes());
                destination as u32
            }
            6 => {
                let source =
                    read_z(registers.argument(1) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let source = String::from_utf8_lossy(source);
                let (prefix, suffix) = strings::shortened(&source, 21, "…");
                let result = [prefix.as_bytes(), suffix.as_bytes()].concat();
                let destination = registers.argument(0);
                copy_z(destination as *mut u8, 21, &result);
                destination
            }
            8 => {
                let source = read_z(registers.ecx as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                source.iter().filter(|&&byte| byte == b'\n').count() as u16 as u32
            }
            9 => {
                let source = registers.argument(0) as *const u8;
                let source_bytes = read_z(source, MAX_TEXT_BYTES).unwrap_or_default();
                let next = registers.argument(1) as *mut u32;
                let token = tokens::parse(source_bytes, true);
                // Both callers allocate exactly four bytes, and inspect the
                // first byte plus the returned kind. Four-byte scalars occupy
                // that full buffer; no terminator is written past its end.
                copy_character(registers.edx as *mut u8, source_bytes, token.bytes);
                if !next.is_null() {
                    ptr::write_unaligned(next, source.add(token.bytes) as u32)
                }
                if token.kind == 0 {
                    4
                } else {
                    u32::from(token.columns > 1)
                }
            }
            10 => {
                let source = read_z(registers.eax as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let index = registers.argument(0) as usize;
                str::from_utf8(source)
                    .ok()
                    .filter(|text| index < text.len() && text.is_char_boundary(index))
                    .and_then(|text| text[index..].chars().next())
                    .is_some_and(|c| char_columns(c) > 1) as u32
            }
            11 => {
                let source = read_z(registers.edx as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                String::from_utf8_lossy(source).graphemes(true).count() as u8 as u32
            }
            12 | 13 => {
                let source = if operation == 12 {
                    registers.edi
                } else {
                    registers.argument(0)
                };
                let capacity = registers.ecx as usize;
                read_z(source as *const u8, capacity)
                    .and_then(|bytes| str::from_utf8(bytes).ok())
                    .is_some_and(|text| {
                        operation == 12 || text.chars().all(|c| c == '\n' || !c.is_control())
                    }) as u32
            }
            14 => {
                let source = read_z(registers.edi as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let maximum = registers.argument(0) as i32;
                if maximum <= 0 || maximum as usize >= source.len() {
                    0
                } else {
                    str::from_utf8(source)
                        .map_or(0, |text| truncate(text, maximum as usize).len() as u32)
                }
            }
            15 => {
                let capacity = registers.argument(1) as i32;
                let destination = registers.argument(0) as *mut u8;
                if capacity <= 0 || destination.is_null() {
                    22
                } else {
                    let source =
                        read_z(registers.eax as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                    let suffix = read_z(registers.argument(2) as *const u8, MAX_TEXT_BYTES)
                        .unwrap_or_default();
                    let source = String::from_utf8_lossy(source);
                    let Ok(suffix) = str::from_utf8(suffix) else {
                        registers.eax = 42;
                        return 1;
                    };
                    let (prefix, suffix) = strings::shortened(&source, capacity as usize, suffix);
                    copy_z(
                        destination,
                        capacity as usize,
                        &[prefix.as_bytes(), suffix.as_bytes()].concat(),
                    );
                    0
                }
            }
            16 => {
                let destination = registers.argument(0) as *mut u8;
                let source = read_z(destination, MAX_TEXT_BYTES)
                    .unwrap_or_default()
                    .to_vec();
                let flags = registers.argument(1) as u8;
                let mut offset = 0;
                let mut valid = true;
                while offset < source.len() {
                    let decoded = decode_first(&source[offset..]);
                    let (character, count) = decoded.unwrap_or(('\u{FFFD}', 1));
                    if decoded.is_none() || !strings::allowed(character, flags) {
                        ptr::write_bytes(destination.add(offset), b' ', count);
                        valid = false;
                    }
                    offset += count;
                }
                valid as u32
            }
            17 => {
                let source =
                    read_z(registers.argument(0) as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let mut offset = 0;
                while let Some((' ' | '\u{3000}', count)) = decode_first(&source[offset..]) {
                    offset += count;
                }
                offset as u32
            }
            18 => {
                let return_address =
                    ptr::read_unaligned((registers.esp as usize + 4) as *const u32);
                let Some(capacity) =
                    fullwidth::capacity((return_address as usize).wrapping_sub(state.module_base))
                else {
                    return 1;
                };
                let source = read_z(registers.argument(0) as *const u8, MAX_TEXT_BYTES)
                    .and_then(|bytes| str::from_utf8(bytes).ok())
                    .unwrap_or_default();
                let converted =
                    fullwidth::convert(source, capacity, fullwidth::enabled(state.module_base));
                copy_z(registers.eax as *mut u8, capacity, converted.as_bytes());
                registers.eax.wrapping_add(converted.len() as u32)
            }
            19..=23 | 27 => {
                let source_pointer = if matches!(operation, 23 | 27) {
                    registers.argument(0)
                } else {
                    registers.ecx
                };
                let source =
                    read_z(source_pointer as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                let expanded = layout::expand(state.module_base, source);
                match operation {
                    19 => {
                        let input = layout::render_input(&expanded);
                        layout::call_render(
                            RENDER_TARGET.load(Ordering::Acquire),
                            input.as_ptr(),
                            registers.argument(0),
                        )
                    }
                    20 | 23 => layout::measure(&expanded, 1, 2),
                    21 => {
                        let context = layout::context(state.module_base);
                        if context == 0 {
                            0
                        } else {
                            let wide = u32::from(ptr::read((context + 24) as *const u8));
                            let narrow = if ptr::read_unaligned((context + 96) as *const u32) != 0 {
                                wide / 2
                            } else {
                                2 * wide / 3
                            };
                            layout::measure(&expanded, narrow, wide)
                        }
                    }
                    22 => {
                        let available = registers.argument(0) as u16 as u32;
                        let font = registers.argument(1) as u16 as u32;
                        let origin = registers.argument(2);
                        let context = layout::context(state.module_base);
                        let narrow = if context == 0
                            || ptr::read_unaligned((context + 96) as *const u32) != 0
                        {
                            font / 2
                        } else {
                            2 * font / 3
                        };
                        let width = layout::measure(&expanded, narrow, font);
                        if font == 0 || width == 0 {
                            origin
                        } else {
                            origin.wrapping_add((available / 2).wrapping_sub(width / 2))
                        }
                    }
                    27 => {
                        let plain = strings::unformat(&expanded, false);
                        let plain = String::from_utf8_lossy(&plain);
                        let destination = registers.argument(1);
                        copy_z(
                            destination as *mut u8,
                            1024,
                            truncate(&plain, 1023).as_bytes(),
                        );
                        destination
                    }
                    _ => unreachable!(),
                }
            }
            24 => {
                let value = layout::variable(state.module_base, registers.argument(0));
                let narrow = registers.argument(1) as u16 as u32;
                let wide = registers.argument(2) as u16 as u32;
                String::from_utf8_lossy(&value)
                    .graphemes(true)
                    .map(|c| match display_columns(c) {
                        0 => 0,
                        1 => narrow,
                        _ => wide,
                    })
                    .sum()
            }
            25 => layout::variable_character(
                state.module_base,
                registers.argument(0),
                registers.argument(1) as usize,
                registers.argument(2) as *mut u8,
            ),
            26 => layout::display_token(state.module_base, registers),
            28 | 29 => layout::formatted(state.module_base, registers, operation == 29),
            30 => filter::apply(
                state.module_base,
                registers.argument(0) as *mut u8,
                registers.argument(1) as *const u8,
            ),
            31 => script::render(
                state.module_base,
                registers.eax as *const u8,
                f32::from_bits(registers.argument(0)),
                f32::from_bits(registers.argument(1)),
            ),
            32 => {
                let source = read_z(registers.edx as *const u8, MAX_TEXT_BYTES).unwrap_or_default();
                source.iter().take(14).any(|byte| !byte.is_ascii()) as u32
            }
            33 => {
                let pointer = registers.edi as *mut u8;
                let source = read_z(pointer, 8).unwrap_or_default();
                if source.is_empty() {
                    0
                } else if source.iter().any(|&byte| !(0x21..=0x7F).contains(&byte)) {
                    (-1i32) as u32
                } else {
                    let uppercase = source
                        .iter()
                        .map(u8::to_ascii_uppercase)
                        .collect::<Vec<_>>();
                    copy_z(pointer, 8, &uppercase);
                    if uppercase.len() != 6 {
                        (-2i32) as u32
                    } else {
                        let expected = std::slice::from_raw_parts(
                            (state.module_base + 0x0ECD82A0) as *const u8,
                            6,
                        );
                        if uppercase == expected {
                            (-3i32) as u32
                        } else {
                            1
                        }
                    }
                }
            }
            34 => {
                type Tooltip = unsafe extern "C" fn(*const u8, u32) -> u32;
                let original =
                    std::mem::transmute::<usize, Tooltip>(TOOLTIP_TARGET.load(Ordering::Acquire));
                let source_pointer = registers.argument(0) as *const u8;
                let source = read_z(source_pointer, MAX_TEXT_BYTES).unwrap_or_default();
                let source_text = String::from_utf8_lossy(source);
                let mut measure = vec![b' '; display_columns(&source_text).min(i16::MAX as usize)];
                measure.push(0);
                let result = original(measure.as_ptr(), registers.argument(1));
                if !source.is_empty() {
                    ptr::write_unaligned(
                        (state.module_base + 0x0E76BB08) as *mut u32,
                        source_pointer as u32,
                    );
                    ptr::write_unaligned(
                        (state.module_base + 0x0E76BAF8) as *mut u16,
                        source.len().min(u16::MAX as usize) as u16,
                    );
                }
                result
            }
            35 => paths::screenshot(state.module_base, registers.esi as *mut u8),
            _ => return 0,
        }
    };
    registers.eax = result;
    1
}

unsafe fn read_z<'a>(source: *const u8, capacity: usize) -> Option<&'a [u8]> {
    if source.is_null() || capacity == 0 || capacity > MAX_TEXT_BYTES {
        return None;
    }
    for length in 0..capacity {
        if unsafe { ptr::read(source.add(length)) } == 0 {
            return Some(unsafe { std::slice::from_raw_parts(source, length) });
        }
    }
    None
}

unsafe fn copy_z(destination: *mut u8, capacity: usize, source: &[u8]) {
    if destination.is_null() || capacity == 0 {
        return;
    }
    let count = source.len().min(capacity - 1);
    unsafe {
        ptr::copy(source.as_ptr(), destination, count);
        ptr::write(destination.add(count), 0);
    }
}

unsafe fn copy_character(destination: *mut u8, source: &[u8], count: usize) {
    if destination.is_null() {
        return;
    }
    let count = count.min(source.len()).min(4);
    unsafe {
        ptr::copy(source.as_ptr(), destination, count);
    }
    if count < 4 {
        unsafe { ptr::write(destination.add(count), 0) }
    }
}
