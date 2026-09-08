use std::{ffi::CStr, ops::Range, ptr};

use crate::text::utf8;
use egui::{ImeEvent, Pos2, Rect, pos2, vec2};
use windows::Win32::{
    Foundation::{HWND, LPARAM},
    UI::{
        Input::Ime::{
            GCS_CURSORPOS, ImmGetCompositionStringW, ImmGetContext, ImmReleaseContext,
            ImmSetCompositionStringW, SCS_SETSTR,
        },
        WindowsAndMessaging::{GetCaretBlinkTime, KillTimer, SetTimer},
    },
};

const ACTIVE_EDITOR: usize = 0x0EDB_A1BC;
const WINDOW: usize = 0x0E81_1A38;
const COMPOSING: usize = 0x0E81_1A20;
const CURSOR_INTERVAL: usize = 0x0E81_1A24;
const CURSOR_BLINK: usize = 0x0E81_1A28;
const CANDIDATES_VISIBLE: usize = 0x0E81_1A2C;
const COMPOSITION: usize = 0x0E39_2CB0;
const COMPOSITION_LEN: usize = 0x0E38_E880;
const ATTRIBUTES: usize = 0x0E38_E680;
const ATTRIBUTES_LEN: usize = 0x0E39_2EB4;
const CLAUSES: usize = 0x0E39_2AB0;
const CLAUSES_LEN: usize = 0x0E39_2AAC;
const COMPOSITION_CURSOR: usize = 0x0E39_28A4;
const CLIENT_WIDTH: usize = 0x0EBE_E5A0;
const CLIENT_HEIGHT: usize = 0x0EBE_E59C;

const TEXT_OFFSET: usize = 43;
const TEXT_CAPACITY: usize = 256;
const COMPOSITION_CAPACITY: usize = 512;

/// Adapts Unicode events from the shared IMM context to native UTF-8 buffers.
/// The game's input and drawing callbacks own these buffers; no Rust references
/// to their contents survive a callback or an IMM call.
pub(super) struct Editor {
    base: usize,
}

impl Editor {
    /// The caller must validate the native layout, retain
    /// the module, and stop all callbacks before unloading it.
    pub(super) unsafe fn new(base: usize) -> Self {
        Self { base }
    }

    pub(super) fn target_id(&self) -> Option<usize> {
        let id = unsafe { read::<u32>(self.base + ACTIVE_EDITOR) } as usize;
        if id == 0
            || unsafe { read::<u8>(id + 21) } == 0
            || unsafe { read::<u8>(id + 328) } & 1 != 0
        {
            return None;
        }
        Some(id)
    }

    pub(super) fn cursor_rect(&self, id: usize) -> Option<Rect> {
        if self.target_id() != Some(id) {
            return None;
        }
        let anchor = unsafe {
            native_anchor(
                read::<i32>(self.base + CLIENT_WIDTH),
                read::<i32>(self.base + CLIENT_HEIGHT),
                read::<f32>(id + 32),
                read::<i16>(id + 24),
                read::<u8>(id + 325),
                read::<u8>(id + 326),
            )
        };
        let cursor = usize::from(unsafe { read::<u16>(id + 16) });
        let extra = usize::from(unsafe { read::<u16>(id + 18) });
        let scroll = unsafe { read::<i32>(id + 28) };
        let width = unsafe { read::<f32>(id + 304) };
        let height = unsafe { read::<u32>(id + 308) } as f32;
        if !width.is_finite() || width <= 0.0 || height == 0.0 {
            return None;
        }
        let text = unsafe { read_text(id + TEXT_OFFSET, TEXT_CAPACITY) }?;
        let masked = unsafe { read::<u8>(id + 324) } != 0;
        let prefix = utf8::truncate(&text, cursor.saturating_add(extra));
        let mut columns = if masked {
            prefix.chars().count()
        } else {
            utf8::display_columns(prefix)
        } as i64;
        if unsafe { read::<u32>(self.base + COMPOSING) } != 0 {
            let composition = unsafe { read_text(self.base + COMPOSITION, COMPOSITION_CAPACITY) }?;
            let cursor = unsafe { read::<u32>(self.base + COMPOSITION_CURSOR) } as usize;
            columns += utf8::display_columns(utf8::truncate(&composition, cursor)) as i64;
        }
        columns -= i64::from(scroll);
        let rect = Rect::from_min_size(
            anchor + vec2(columns as f32 * width * 0.5, 0.0),
            vec2(1.0, height),
        );
        rect.is_finite().then_some(rect)
    }

    /// The broker has decoded UTF-16 from either WM_CHAR or the shared context.
    pub(super) fn event(&self, id: usize, event: &ImeEvent) {
        if matches!(event, ImeEvent::Preedit { text, .. } if text.is_empty()) {
            // Cancellation may arrive after the active native editor changed.
            // These globals must still be cleared, without dereferencing `id`.
            self.clear_preedit();
            return;
        }
        if self.target_id() != Some(id) {
            return;
        }
        match event {
            ImeEvent::Preedit {
                text,
                active_range_chars,
            } => self.preedit(id, text, active_range_chars.as_ref()),
            ImeEvent::Commit(text) => self.commit(id, text),
            // The IMM broker emits only Preedit and Commit.
            _ => {}
        }
    }

    /// `lparam` for 0x7EA points to the native caller's NUL-terminated text.
    pub(super) unsafe fn composition_command(
        &self,
        hwnd: HWND,
        message: u32,
        lparam: LPARAM,
    ) -> bool {
        if !matches!(message, 0x7EA | 0x7EC) {
            return false;
        }
        if self.target_id().is_none() {
            return true;
        }
        let Some(text) = (unsafe { self.composition_command_text(message, lparam) }) else {
            return true;
        };
        let context = unsafe { ImmGetContext(hwnd) };
        if !context.is_invalid() {
            // The shared context remains the owner. A suspended context makes
            // this a no-op, and its normal Unicode event updates the preedit.
            let wide: Vec<u16> = text.encode_utf16().collect();
            unsafe {
                let _ = ImmSetCompositionStringW(
                    context,
                    SCS_SETSTR,
                    Some(wide.as_ptr().cast()),
                    (wide.len() * 2) as u32,
                    None,
                    0,
                );
                let _ = ImmReleaseContext(hwnd, context);
            }
        }
        true
    }

    unsafe fn composition_command_text(&self, message: u32, lparam: LPARAM) -> Option<String> {
        match message {
            0x7EA if lparam.0 != 0 => {
                let text = unsafe { CStr::from_ptr(lparam.0 as *const _) }
                    .to_str()
                    .ok()?;
                Some(utf8::truncate(text, COMPOSITION_CAPACITY - 1).to_owned())
            }
            0x7EC => {
                let mut text = unsafe { read_text(self.base + COMPOSITION, COMPOSITION_CAPACITY) }?;
                // 114D3960 removes one or two bytes, corrupting UTF-8 (and
                // writing before the buffer when the composition is empty).
                text.pop();
                Some(text)
            }
            _ => None,
        }
    }

    fn preedit(&self, id: usize, text: &str, active_range: Option<&Range<usize>>) {
        let preedit = Preedit::new(text, active_range, self.composition_cursor());
        if self.target_id() != Some(id) {
            return;
        }
        unsafe {
            self.copy_composition_data(COMPOSITION, preedit.text.as_bytes());
            self.copy_composition_data(ATTRIBUTES, &preedit.attributes);
            self.copy_composition_data(CLAUSES, &preedit.clauses);
            write(self.base + COMPOSITION_LEN, preedit.text.len() as u32);
            write(self.base + ATTRIBUTES_LEN, preedit.attributes.len() as u32);
            write(self.base + CLAUSES_LEN, preedit.clauses.len() as u32);
            write(self.base + COMPOSITION_CURSOR, preedit.cursor as u32);
            write(self.base + COMPOSING, u32::from(!preedit.text.is_empty()));
            write(self.base + CANDIDATES_VISIBLE, 0u32);
            write(self.base + CURSOR_BLINK, 1u32);
        }
    }

    fn composition_cursor(&self) -> Option<usize> {
        // egui's active range describes the highlighted conversion clause. Its
        // end need not be the insertion caret. Read only this UTF-16 offset from
        // the broker's still-associated context; the Unicode text remains the
        // event payload and is never re-read through a code-page conversion.
        let hwnd = HWND(unsafe { read::<u32>(self.base + WINDOW) } as usize as *mut _);
        let context = unsafe { ImmGetContext(hwnd) };
        if context.is_invalid() {
            return None;
        }
        let cursor = unsafe { ImmGetCompositionStringW(context, GCS_CURSORPOS, None, 0) };
        unsafe {
            let _ = ImmReleaseContext(hwnd, context);
        }
        usize::try_from(cursor).ok()
    }

    fn commit(&self, id: usize, text: &str) {
        if self.target_id() == Some(id) {
            let destination = (id + TEXT_OFFSET) as *mut u8;
            let cursor = unsafe { read::<u16>(id + 16) };
            let limit = unsafe { read::<u16>(id + 26) };
            let buffer = unsafe { std::slice::from_raw_parts_mut(destination, TEXT_CAPACITY) };
            if let Some(inserted) =
                utf8::insert(buffer, usize::from(cursor), usize::from(limit), text)
            {
                unsafe {
                    write(id + 16, cursor + inserted as u16);
                    if read::<u8>(id + 320) & 1 != 0 {
                        write(id + 28, 0i32);
                    }
                }
            }
        }
        self.clear_preedit();
        self.reset_cursor_blink();
    }

    fn reset_cursor_blink(&self) {
        // Preserve 114D4AA0's keypress behavior now that Unicode WM_CHAR bypasses
        // the native single-byte insertion branch and its caret timer reset.
        let hwnd = HWND(unsafe { read::<u32>(self.base + WINDOW) } as usize as *mut _);
        if !hwnd.is_invalid() {
            let mut interval = unsafe { read::<u32>(self.base + CURSOR_INTERVAL) };
            if interval != 0 && unsafe { GetCaretBlinkTime() } == 0 {
                interval = 0;
                unsafe {
                    write(self.base + CURSOR_INTERVAL, 0u32);
                    let _ = KillTimer(Some(hwnd), 0xDEAD);
                }
            }
            if interval != 0 {
                unsafe {
                    SetTimer(Some(hwnd), 0xDEAD, interval, None);
                }
            }
        }
        unsafe { write(self.base + CURSOR_BLINK, 1u32) };
    }

    fn clear_preedit(&self) {
        unsafe {
            write(self.base + COMPOSITION, 0u8);
            write(self.base + ATTRIBUTES, 0u8);
            write(self.base + CLAUSES, 0u32);
            for offset in [
                COMPOSITION_LEN,
                ATTRIBUTES_LEN,
                CLAUSES_LEN,
                COMPOSITION_CURSOR,
                COMPOSING,
                CANDIDATES_VISIBLE,
            ] {
                write(self.base + offset, 0u32);
            }
        }
    }

    unsafe fn copy_composition_data(&self, offset: usize, data: &[u8]) {
        debug_assert!(data.len() <= COMPOSITION_CAPACITY);
        let target = (self.base + offset) as *mut u8;
        unsafe {
            ptr::write_bytes(target, 0, COMPOSITION_CAPACITY);
            ptr::copy_nonoverlapping(data.as_ptr(), target, data.len());
        }
    }
}

unsafe fn read_text(address: usize, capacity: usize) -> Option<String> {
    let buffer = unsafe { std::slice::from_raw_parts(address as *const u8, capacity) };
    let length = buffer.iter().position(|byte| *byte == 0)?;
    std::str::from_utf8(&buffer[..length])
        .ok()
        .map(str::to_owned)
}

struct Preedit<'a> {
    text: &'a str,
    attributes: Vec<u8>,
    clauses: Vec<u8>,
    cursor: usize,
}

impl<'a> Preedit<'a> {
    fn new(
        text: &'a str,
        active_range: Option<&Range<usize>>,
        cursor_utf16: Option<usize>,
    ) -> Self {
        let text = utf8::truncate(
            text.split('\0').next().unwrap_or_default(),
            COMPOSITION_CAPACITY - 1,
        );
        let range = active_range.map_or(text.len()..text.len(), |range| {
            let start = utf8::byte_offset(text, range.start);
            start..utf8::byte_offset(text, range.end).max(start)
        });
        let mut attributes = vec![0; text.len()];
        attributes[range.clone()].fill(1);
        let mut boundaries = vec![0, range.start, range.end, text.len()];
        boundaries.dedup();
        let clauses = boundaries
            .into_iter()
            .flat_map(|offset| (offset as u32).to_le_bytes())
            .collect();
        let cursor = cursor_utf16.map_or(range.end, |cursor| {
            let mut units = 0;
            let mut bytes = 0;
            for character in text.chars() {
                if units + character.len_utf16() > cursor {
                    break;
                }
                units += character.len_utf16();
                bytes += character.len_utf8();
            }
            bytes
        });
        Self {
            text,
            attributes,
            clauses,
            cursor,
        }
    }
}

/// The sole native drawing caller normalizes against a 640 by 448 client area.
fn native_anchor(
    width: i32,
    height: i32,
    input_x: f32,
    input_y: i16,
    flags_x: u8,
    flags_y: u8,
) -> Pos2 {
    let truncate = |value: f32| f32::from(value as i32 as i16);
    let width = width as f32;
    let height = height as f32;
    let input_y = f32::from(input_y);
    let mut x = truncate(width * (1.0 / 640.0) * input_x);
    if flags_x & 9 != 0 {
        x = truncate(width * 0.5 - (320.0 - input_x));
    }
    if flags_x & 0x20 != 0 {
        x = input_x;
    }
    let mut y = truncate(height * (1.0 / 448.0) * input_y);
    if flags_y & 0x10 != 0 {
        y = truncate(height * 0.5 - (224.0 - input_y));
    }
    if flags_y & 0x20 != 0 {
        y = input_y;
    }
    pos2(x + 6.0, y + 4.0)
}

unsafe fn read<T: Copy>(address: usize) -> T {
    // Native scalar fields are read by both WindowProc and rendering callbacks.
    unsafe { ptr::read_volatile(address as *const T) }
}

unsafe fn write<T>(address: usize, value: T) {
    unsafe { ptr::write_volatile(address as *mut T, value) };
}

#[cfg(test)]
mod tests {
    use super::{
        ACTIVE_EDITOR, COMPOSITION, COMPOSITION_CAPACITY, CURSOR_BLINK, Editor, Preedit,
        native_anchor, read, write,
    };
    use egui::pos2;

    #[test]
    fn committed_characters_preserve_native_utf8_offsets_and_buffer_limits() {
        use windows::Win32::System::Memory::{
            MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_NOACCESS, PAGE_READWRITE, VirtualAlloc,
            VirtualFree,
        };
        struct Image(*mut core::ffi::c_void);
        impl Drop for Image {
            fn drop(&mut self) {
                unsafe {
                    VirtualFree(self.0, 0, MEM_RELEASE).unwrap();
                }
            }
        }
        // Reserve the supported RVA layout, committing only its four data pages.
        // This exercises the adapter itself without loading or starting the game.
        let image = Image(unsafe { VirtualAlloc(None, 0x0EDB_C000, MEM_RESERVE, PAGE_NOACCESS) });
        assert!(!image.0.is_null());
        let base = image.0 as usize;
        for page in [0x0EDB_A000, 0x0E81_1000, 0x0E39_2000, 0x0E38_E000] {
            assert!(
                !unsafe {
                    VirtualAlloc(
                        Some((base + page) as *const _),
                        4096,
                        MEM_COMMIT,
                        PAGE_READWRITE,
                    )
                }
                .is_null()
            );
        }
        let mut native = [0u32; 84];
        let id = native.as_mut_ptr() as usize;
        unsafe {
            write(base + ACTIVE_EDITOR, u32::try_from(id).unwrap());
            write(id + 21, 1u8);
            write(id + 16, 1u16);
            write(id + 26, 255u16);
            write(id + 28, 88i32);
            write(id + 320, 1u8);
            write(id + 42, 0xA5u8);
            write(id + 299, 0xA5u8);
            std::ptr::copy_nonoverlapping(c"A中".as_ptr().cast(), (id + 43) as *mut u8, 5);
        }
        let editor = unsafe { Editor::new(base) };
        for text in ["é", "😀"] {
            editor.event(id, &egui::ImeEvent::Commit(text.to_owned()));
        }
        assert_eq!(
            unsafe { super::read_text(id + 43, 256) }.as_deref(),
            Some("Aé😀中")
        );
        assert_eq!(unsafe { read::<u16>(id + 16) }, 7);
        assert_eq!(unsafe { read::<i32>(id + 28) }, 0);
        assert_eq!(unsafe { read::<u32>(base + CURSOR_BLINK) }, 1);
        unsafe {
            write(id + 26, 11u16);
        }
        editor.event(id, &egui::ImeEvent::Commit("𠮷".to_owned()));
        assert_eq!(unsafe { read::<u16>(id + 16) }, 7);
        editor.event(id, &egui::ImeEvent::Commit("!".to_owned()));
        assert_eq!(
            unsafe { super::read_text(id + 43, 256) }.as_deref(),
            Some("Aé😀!中")
        );
        assert_eq!(unsafe { read::<u16>(id + 16) }, 8);
        unsafe {
            write(id + 16, 2u16);
        }
        editor.event(id, &egui::ImeEvent::Commit("X".to_owned()));
        assert_eq!(
            unsafe { super::read_text(id + 43, 256) }.as_deref(),
            Some("Aé😀!中")
        );
        assert_eq!(unsafe { read::<u16>(id + 16) }, 2);
        assert_eq!(unsafe { read::<u8>(id + 42) }, 0xA5);
        assert_eq!(unsafe { read::<u8>(id + 299) }, 0xA5);

        // Soft-keyboard delete must preserve the preceding UTF-8 scalar and
        // leave native memory untouched until IMM publishes the new preedit.
        for (text, expected) in [("中😀", "中"), ("中", ""), ("", "")] {
            unsafe {
                editor.copy_composition_data(COMPOSITION, text.as_bytes());
                write(base + COMPOSITION - 1, 0xA5u8);
            }
            let command = unsafe { editor.composition_command_text(0x7EC, Default::default()) };
            assert_eq!(command.as_deref(), Some(expected));
            assert_eq!(
                unsafe { super::read_text(base + COMPOSITION, COMPOSITION_CAPACITY) }.as_deref(),
                Some(text)
            );
            assert_eq!(unsafe { read::<u8>(base + COMPOSITION - 1) }, 0xA5);
        }
        let replacement = c"猎人😀";
        let command = unsafe {
            editor.composition_command_text(
                0x7EA,
                windows::Win32::Foundation::LPARAM(replacement.as_ptr() as isize),
            )
        };
        assert_eq!(command.as_deref(), Some("猎人😀"));
    }

    #[test]
    fn cursor_anchor_matches_native_scaling_and_alignment_flags() {
        assert_eq!(
            native_anchor(1280, 896, 100.5, 50, 0, 0),
            pos2(207.0, 104.0)
        );
        assert_eq!(
            native_anchor(1280, 896, 100.5, 50, 1, 0x10),
            pos2(426.0, 278.0)
        );
        assert_eq!(
            native_anchor(1280, 896, 100.5, 50, 8, 0x10),
            pos2(426.0, 278.0)
        );
        assert_eq!(
            native_anchor(1280, 896, 100.5, 50, 0x29, 0x30),
            pos2(106.5, 54.0)
        );
    }

    #[test]
    fn preedit_maps_character_ranges_to_utf8_byte_metadata() {
        let preedit = Preedit::new("A中😀e\u{301}", Some(&(1..3)), None);
        assert_eq!(preedit.text, "A中😀e\u{301}");
        assert_eq!(preedit.cursor, 8);
        assert_eq!(preedit.attributes, [0, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0]);
        let clauses: Vec<_> = preedit
            .clauses
            .as_chunks::<4>()
            .0
            .iter()
            .map(|value| u32::from_le_bytes(*value))
            .collect();
        assert_eq!(clauses, [0, 1, 8, 11]);
    }

    #[test]
    fn preedit_clamps_capacity_and_range_without_splitting_non_bmp_text() {
        let text = "😀".repeat(COMPOSITION_CAPACITY / 4 + 1);
        let preedit = Preedit::new(&text, Some(&(1..usize::MAX)), None);
        assert_eq!(preedit.text.len(), 508);
        assert_eq!(preedit.cursor, 508);
        assert_eq!(preedit.attributes.len(), preedit.text.len());
        assert!(preedit.attributes[..4].iter().all(|value| *value == 0));
        assert!(preedit.attributes[4..].iter().all(|value| *value == 1));
        assert_eq!(Preedit::new("中\0文", None, None).text, "中");
        let reversed = std::ops::Range { start: 1, end: 0 };
        assert_eq!(Preedit::new("中", Some(&reversed), None).cursor, 3);
        assert_eq!(Preedit::new("中", Some(&(0..0)), None).cursor, 0);
    }

    #[test]
    fn composition_caret_is_independent_from_the_converted_clause() {
        let range = 0..3;
        for (units, bytes) in [(0, 0), (1, 1), (2, 1), (3, 5), (4, 8), (99, 8)] {
            let preedit = Preedit::new("A😀中", Some(&range), Some(units));
            assert_eq!(preedit.cursor, bytes);
            assert_eq!(preedit.attributes, [1; 8]);
        }
    }
}
