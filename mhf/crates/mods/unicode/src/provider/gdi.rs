//! UTF-8 adapters for the font module's GDI entrypoints.

use mhf_font::TextRenderer;
use windows_sys::{
    Win32::{
        Foundation::{RECT, SIZE},
        Graphics::Gdi::{ETO_OPTIONS, ETO_PDY, ExtTextOutW, GetTextExtentPoint32W, HDC},
    },
    core::{BOOL, PCSTR},
};

pub const fn renderer() -> TextRenderer {
    TextRenderer { measure, draw }
}

unsafe extern "system" fn measure(hdc: HDC, string: PCSTR, count: i32, size: *mut SIZE) -> BOOL {
    let Some(text) = (unsafe { utf8_text(string, count.try_into().ok()) }) else {
        return 0;
    };
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    unsafe { GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, size) }
}

unsafe extern "system" fn draw(
    hdc: HDC,
    x: i32,
    y: i32,
    options: ETO_OPTIONS,
    rect: *const RECT,
    string: PCSTR,
    count: u32,
    spacing: *const i32,
) -> BOOL {
    let Some(text) = (unsafe { utf8_text(string, Some(count as usize)) }) else {
        return 0;
    };
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    let axes = if options & ETO_PDY != 0 { 2 } else { 1 };
    let advances = if spacing.is_null() {
        None
    } else {
        let source = unsafe { std::slice::from_raw_parts(spacing, text.len() * axes) };
        Some(utf16_advances(text, source, axes))
    };
    unsafe {
        ExtTextOutW(
            hdc,
            x,
            y,
            options,
            rect,
            utf16.as_ptr(),
            utf16.len() as u32,
            advances
                .as_ref()
                .map_or(std::ptr::null(), |values| values.as_ptr()),
        )
    }
}

unsafe fn utf8_text<'a>(string: PCSTR, count: Option<usize>) -> Option<&'a str> {
    if string.is_null() {
        return None;
    }
    std::str::from_utf8(unsafe { std::slice::from_raw_parts(string, count?) }).ok()
}

fn utf16_advances(text: &str, advances: &[i32], axes: usize) -> Vec<i32> {
    let mut output = Vec::with_capacity(text.encode_utf16().count() * axes);
    for (offset, character) in text.char_indices() {
        for _ in 1..character.len_utf16() {
            output.extend(std::iter::repeat_n(0, axes));
        }
        for axis in 0..axes {
            let value = (offset..offset + character.len_utf8()).fold(0i32, |sum, byte| {
                sum.wrapping_add(advances[byte * axes + axis])
            });
            output.push(value);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::utf16_advances;

    #[test]
    fn preserves_byte_advances_across_utf8_and_surrogate_pairs() {
        assert_eq!(
            utf16_advances("A中🙂", &[1, 2, 3, 4, 5, 6, 7, 8], 1),
            [1, 9, 0, 26]
        );
        assert_eq!(utf16_advances("é", &[2, 10, 3, 20], 2), [5, 30]);
    }
}
