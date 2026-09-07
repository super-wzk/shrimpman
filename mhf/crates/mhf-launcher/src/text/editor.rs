use std::{ffi::c_void, ptr, sync::atomic::AtomicUsize};

use unicode_segmentation::UnicodeSegmentation;
use windows::Win32::{
    Foundation::HGLOBAL,
    System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    },
};

use super::{CodeHook, HOOK_STATE, utf8};

const ACTIVE_EDITOR: usize = 0x0EDB_A1BC;
const RENDERER: usize = 0x0E3C_BD64;
const COMPOSING: usize = 0x0E81_1A20;
const COMPOSITION: usize = 0x0E39_2CB0;
const ATTRIBUTES: usize = 0x0E38_E680;
const COMPOSITION_CURSOR: usize = 0x0E39_28A4;
const CURSOR_BLINK: usize = 0x0E81_1A28;
const CLIPBOARD: usize = 0x0E39_28A8;
const TEXT_CAPACITY: usize = 256;
const COMPOSITION_CAPACITY: usize = 512;

static END_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static MASK_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static RENDER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static CLIPBOARD_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "UTF-8 editor end navigation",
            rva: 0x014D_35F0,
            signature: &[
                (0, 0x55),
                (1, 0x8b),
                (2, 0xec),
                (3, 0x8b),
                (4, 0x15),
                (9, 0x83),
                (10, 0xec),
                (11, 0x08),
            ],
            detour: end_hook as *mut c_void,
            original: &END_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 password masking",
            rva: 0x014D_4240,
            signature: &[
                (0, 0x55),
                (1, 0x8b),
                (2, 0xec),
                (3, 0x81),
                (4, 0xec),
                (5, 0x0c),
                (6, 0x01),
                (7, 0),
                (8, 0),
            ],
            detour: mask_hook as *mut c_void,
            original: &MASK_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 editor layout and composition drawing",
            rva: 0x014D_42F0,
            signature: &[
                (0, 0x55),
                (1, 0x8b),
                (2, 0xec),
                (3, 0x81),
                (4, 0xec),
                (5, 0x6c),
                (6, 0x01),
                (7, 0),
                (8, 0),
            ],
            detour: render_hook as *mut c_void,
            original: &RENDER_ORIGINAL,
        },
        CodeHook {
            name: "Unicode clipboard text",
            rva: 0x014D_4AF0,
            signature: &[
                (0, 0x55),
                (1, 0x8b),
                (2, 0xec),
                (3, 0x51),
                (4, 0x53),
                (5, 0x57),
                (6, 0x33),
                (7, 0xdb),
                (8, 0x53),
            ],
            detour: clipboard_hook as *mut c_void,
            original: &CLIPBOARD_ORIGINAL,
        },
    ]
}

unsafe extern "C" fn end_hook() -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let editor = unsafe { read::<u32>(state.module_base + ACTIVE_EDITOR) } as usize;
    if editor == 0 {
        return 0;
    }
    let Some(text) = (unsafe { read_text(editor + 43, TEXT_CAPACITY) }) else {
        return 0;
    };
    if usize::from(unsafe { read::<u16>(editor + 16) }) != text.len() {
        unsafe {
            write(editor + 16, text.len() as u16);
            write(editor + 15, 0u8);
            write(editor + 300, 23u32);
        }
    }
    0
}

unsafe extern "thiscall" fn mask_hook(destination: *mut u8) -> usize {
    let invocation = HOOK_STATE.enter();
    let Some(_state) = invocation.state() else {
        return 0;
    };
    if destination.is_null() {
        return 0;
    }
    let Some(text) = (unsafe { read_text(destination as usize, TEXT_CAPACITY) }) else {
        return 0;
    };
    let length = text.chars().count();
    unsafe {
        ptr::write_bytes(destination, b'*', length);
        destination.add(length).write(0);
    }
    destination as usize
}

struct Run {
    text: String,
    column: usize,
    style: u32,
}

struct Layout {
    runs: Vec<Run>,
    scroll: usize,
    cursor: usize,
}

/// The editor stores byte cursors and a display-column scroll origin. Construct
/// complete grapheme runs, so clipping and highlighting use the same boundaries
/// as the font atlas and never separate joined emoji or combining marks.
#[allow(clippy::too_many_arguments)]
fn layout(
    text: &str,
    cursor: usize,
    composition: &str,
    composition_cursor: usize,
    attributes: &[u8],
    masked: bool,
    scroll: usize,
    width: usize,
) -> Layout {
    let cursor = utf8::floor_boundary(text, cursor);
    let display = |text: &str| {
        if masked {
            "*".repeat(text.chars().count())
        } else {
            text.to_owned()
        }
    };
    let prefix = display(&text[..cursor]);
    let suffix = display(&text[cursor..]);
    let caret = utf8::display_columns(&prefix)
        + utf8::display_columns(utf8::truncate(composition, composition_cursor));
    let width = width.max(1);
    let margin = 2.min(width / 2);
    let mut scroll = scroll;
    if caret < scroll.saturating_add(margin) {
        scroll = caret.saturating_sub(margin);
    } else if caret >= scroll.saturating_add(width.saturating_sub(margin)) {
        scroll = caret.saturating_sub(width.saturating_sub(margin).saturating_sub(1));
    }
    let full = format!("{prefix}{composition}{suffix}");
    let composition_start = prefix.len();
    let composition_end = composition_start + composition.len();
    let mut runs: Vec<Run> = Vec::new();
    let mut visible_end = None;
    for (offset, grapheme) in full.grapheme_indices(true) {
        let end = offset + grapheme.len();
        let column = utf8::display_columns(&full[..offset]);
        let end_column = utf8::display_columns(&full[..end]);
        if column < scroll || end_column > scroll.saturating_add(width) {
            visible_end = None;
            continue;
        }
        // Combining marks and joined emoji must disappear with a clipped base.
        // Keeping a zero-width suffix alone would attach it to unrelated text
        // at the left edge of the editor.
        if end_column <= column && visible_end != Some(column) {
            continue;
        }
        visible_end = Some(end_column);
        let style = if composition_start < composition_end
            && offset < composition_end
            && end > composition_start
        {
            let start = offset
                .saturating_sub(composition_start)
                .min(attributes.len());
            let end = (end.min(composition_end) - composition_start).min(attributes.len());
            if attributes[start..end]
                .iter()
                .any(|attribute| matches!(*attribute, 1 | 3))
            {
                3
            } else {
                2
            }
        } else {
            0
        };
        if let Some(run) = runs.last_mut()
            && run.style == style
            && run.column + utf8::display_columns(&run.text) == column - scroll
        {
            run.text.push_str(grapheme);
        } else {
            runs.push(Run {
                text: grapheme.to_owned(),
                column: column - scroll,
                style,
            });
        }
    }
    Layout {
        runs,
        scroll,
        cursor: caret.saturating_sub(scroll),
    }
}

// The old routine uses EDI only as an unused vararg for its literal caret glyph.
// Its two coordinates and caller-clean stack match this C signature exactly.
unsafe extern "C" fn render_hook(x: f32, y: f32) -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let base = state.module_base;
    let editor = unsafe { read::<u32>(base + ACTIVE_EDITOR) } as usize;
    // 114D431A loads the renderer directly from this global; its first field is
    // renderer data, not another pointer despite the decompiler's expression.
    let renderer = unsafe { read::<u32>(base + RENDERER) } as usize;
    if editor == 0 || renderer == 0 || !x.is_finite() || !y.is_finite() {
        return 0;
    }
    let Some(text) = (unsafe { read_text(editor + 43, TEXT_CAPACITY) }) else {
        return 0;
    };
    let cursor = usize::from(unsafe { read::<u16>(editor + 16) });
    let extra = usize::from(unsafe { read::<u16>(editor + 18) });
    let width = unsafe { read::<f32>(editor + 304) };
    let height = unsafe { read::<u32>(editor + 308) };
    if !width.is_finite() || width <= 0.0 || height == 0 {
        return 0;
    }
    let columns = unsafe { read::<u32>(editor + 312) }.clamp(1, 512) as usize;
    let scroll = unsafe { read::<i32>(editor + 28) }.max(0) as usize;
    let masked = unsafe { read::<u8>(editor + 324) } != 0;
    let composition = if unsafe { read::<u32>(base + COMPOSING) } != 0 {
        unsafe { read_text(base + COMPOSITION, COMPOSITION_CAPACITY) }.unwrap_or_default()
    } else {
        String::new()
    };
    let composition_cursor = unsafe { read::<u32>(base + COMPOSITION_CURSOR) } as usize;
    let attributes =
        unsafe { std::slice::from_raw_parts((base + ATTRIBUTES) as *const u8, composition.len()) };
    let layout = layout(
        &text,
        cursor.saturating_add(extra),
        &composition,
        composition_cursor,
        attributes,
        masked,
        scroll,
        columns,
    );
    unsafe {
        write(editor + 28, layout.scroll as i32);
    }
    let queue: unsafe extern "C" fn(*const u8) -> i32 =
        unsafe { std::mem::transmute(base + 0x014D_EA50) };
    let default_color = unsafe { read::<u32>(base + 0x015F_98F8) } | 0xff00_0000;
    let mut result = 0;
    for run in layout.runs {
        let mut bytes = run.text.into_bytes();
        bytes.push(0);
        unsafe {
            set_draw_state(
                base,
                renderer,
                x + run.column as f32 * width * 0.5,
                y,
                width as u8,
                height as u8,
                run.style,
                if run.style == 0 {
                    default_color
                } else {
                    u32::MAX
                },
            );
            result = queue(bytes.as_ptr());
        }
    }
    if unsafe { read::<u32>(base + CURSOR_BLINK) } != 0 && layout.cursor <= columns {
        unsafe {
            set_draw_state(
                base,
                renderer,
                x + layout.cursor as f32 * width * 0.5,
                y - 3.0,
                2,
                (height as u8).wrapping_add(6),
                0,
                default_color,
            );
            result = queue(c"■".as_ptr().cast());
        }
    }
    result
}

#[allow(clippy::too_many_arguments)]
unsafe fn set_draw_state(
    base: usize,
    renderer: usize,
    x: f32,
    y: f32,
    width: u8,
    height: u8,
    style: u32,
    color: u32,
) {
    let scale = unsafe { read::<f32>(renderer + 16) };
    let bucket = (scale * unsafe { read::<f32>(base + 0x019B_5F00) } * 7.0) as i32;
    unsafe {
        write(renderer + 12, x as i32 as i16);
        write(renderer + 14, y as i32 as i16);
        write(renderer + 24, width);
        write(renderer + 25, height);
        write(renderer + 100, bucket.clamp(0, 6));
        write(renderer + 136, style);
        write(renderer + 133_072, color);
    }
}

unsafe extern "C" fn clipboard_hook() -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let text = clipboard_text();
    let destination = (state.module_base + CLIPBOARD) as *mut u8;
    unsafe {
        ptr::write_bytes(destination, 0, COMPOSITION_CAPACITY);
        if let Some(text) = &text {
            ptr::copy_nonoverlapping(text.as_ptr(), destination, text.len());
        }
    }
    i32::from(text.is_some())
}

fn clipboard_text() -> Option<String> {
    unsafe { OpenClipboard(None) }.ok()?;
    struct Clipboard;
    impl Drop for Clipboard {
        fn drop(&mut self) {
            let _ = unsafe { CloseClipboard() };
        }
    }
    let _clipboard = Clipboard;
    let handle = unsafe { GetClipboardData(13) }.ok()?; // CF_UNICODETEXT
    let handle = HGLOBAL(handle.0);
    let capacity = unsafe { GlobalSize(handle) } / size_of::<u16>();
    if capacity == 0 {
        return None;
    }
    let pointer = unsafe { GlobalLock(handle) }.cast::<u16>();
    if pointer.is_null() {
        return None;
    }
    struct Locked(HGLOBAL);
    impl Drop for Locked {
        fn drop(&mut self) {
            let _ = unsafe { GlobalUnlock(self.0) };
        }
    }
    let _locked = Locked(handle);
    let units = unsafe { std::slice::from_raw_parts(pointer, capacity.min(COMPOSITION_CAPACITY)) };
    clipboard_line(units)
}

fn clipboard_line(units: &[u16]) -> Option<String> {
    let mut output = String::new();
    for character in char::decode_utf16(units.iter().copied()) {
        let character = character.ok()?;
        if matches!(character, '\0' | '\r' | '\n') {
            break;
        }
        let character = if character.is_control() {
            ' '
        } else {
            character
        };
        if output.len() + character.len_utf8() >= COMPOSITION_CAPACITY {
            break;
        }
        output.push(character);
    }
    Some(output)
}

unsafe fn read_text(address: usize, capacity: usize) -> Option<String> {
    let bytes = unsafe { std::slice::from_raw_parts(address as *const u8, capacity) };
    let length = bytes.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&bytes[..length])
        .ok()
        .map(str::to_owned)
}

unsafe fn read<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_volatile(address as *const T) }
}

unsafe fn write<T>(address: usize, value: T) {
    unsafe {
        ptr::write_volatile(address as *mut T, value);
    }
}

#[cfg(test)]
mod tests {
    use super::{clipboard_line, layout};

    #[test]
    fn layout_inserts_composition_before_the_suffix_and_styles_whole_scalars() {
        let layout = layout("A中Z", 1, "😀é", 4, &[1, 1, 1, 1, 0, 0], false, 0, 20);
        let runs: Vec<_> = layout
            .runs
            .iter()
            .map(|run| (run.text.as_str(), run.column, run.style))
            .collect();
        assert_eq!(
            runs,
            [("A", 0, 0), ("😀", 1, 3), ("é", 3, 2), ("中Z", 4, 0)]
        );
        assert_eq!(layout.cursor, 3);
        assert_eq!(layout.scroll, 0);
    }

    #[test]
    fn scrolling_and_password_caret_use_display_columns() {
        let text = "A中😀e\u{301}Z";
        let layout = layout(text, text.len(), "", 0, &[], false, 0, 5);
        assert_eq!(layout.scroll, 5);
        assert_eq!(layout.cursor, 2);
        assert_eq!(
            layout
                .runs
                .iter()
                .map(|run| run.text.as_str())
                .collect::<String>(),
            "e\u{301}Z"
        );
        let layout = super::layout("中😀x", 7, "", 0, &[], true, 0, 10);
        assert_eq!(layout.cursor, 2);
        assert_eq!(layout.runs[0].text, "***");
    }

    #[test]
    fn clipping_removes_combining_marks_with_their_hidden_base() {
        let layout = layout("Ae\u{301}Zxxx", 6, "", 0, &[], false, 2, 5);
        assert_eq!(layout.scroll, 2);
        assert_eq!(layout.runs[0].text, "Zxxx");
        assert_eq!(layout.runs[0].column, 0);
    }

    #[test]
    fn highlighting_keeps_joined_emoji_and_combining_marks_in_one_run() {
        let composition = "👩‍💻e\u{301}";
        let mut attributes = vec![0; composition.len()];
        attributes[..4].fill(1);
        attributes[12..].fill(1);
        let layout = layout(
            "AZ",
            1,
            composition,
            composition.len(),
            &attributes,
            false,
            0,
            20,
        );
        let runs: Vec<_> = layout
            .runs
            .iter()
            .map(|run| (run.text.as_str(), run.column, run.style))
            .collect();
        assert_eq!(runs, [("A", 0, 0), ("👩‍💻e\u{301}", 1, 3), ("Z", 4, 0)]);
        assert_eq!(layout.cursor, 4);
        let idle = super::layout("Ae\u{301}Z", 2, "", 0, &[], false, 0, 20);
        assert_eq!(idle.runs.len(), 1);
        assert_eq!(idle.runs[0].style, 0);
    }

    #[test]
    fn clipboard_decodes_non_bmp_text_and_truncates_before_a_scalar() {
        assert_eq!(
            clipboard_line(&"中😀\t文\r\nnext\0".encode_utf16().collect::<Vec<_>>()).as_deref(),
            Some("中😀 文")
        );
        assert_eq!(clipboard_line(&[0xd800]), None);
        let text = "😀".repeat(130);
        let output = clipboard_line(&text.encode_utf16().collect::<Vec<_>>()).unwrap();
        assert_eq!(output.len(), 508);
        assert_eq!(output.chars().count(), 127);
    }
}
