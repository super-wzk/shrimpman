use std::ptr;

use egui::{ImeEvent, Pos2, Rect, pos2, vec2};
use windows::Win32::{
    Foundation::HWND,
    UI::Input::Ime::{
        GCS_COMPATTR, GCS_COMPCLAUSE, GCS_COMPSTR, GCS_CURSORPOS, GCS_RESULTSTR, HIMC,
        IME_COMPOSITION_STRING, ImmGetCompositionStringA, ImmGetContext, ImmReleaseContext,
    },
};

const ACTIVE_EDITOR: usize = 0x0EDB_A1BC;
const WINDOW: usize = 0x0E81_1A38;
const INSERT_TEXT: usize = 0x0080_FBA0;
const COMPOSING: usize = 0x0E81_1A20;
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

/// Adapts the shared IMM context to the game's existing ANSI editor buffers.
/// The game's input and drawing callbacks own these buffers; no Rust references
/// to their contents survive a callback or an IMM call.
pub(super) struct Editor {
    base: usize,
}

impl Editor {
    /// The caller must validate the native layout and insertion routine, retain
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
        let cursor = unsafe { read::<u16>(id + 16) };
        let extra = unsafe { read::<u16>(id + 18) };
        let scroll = unsafe { read::<i32>(id + 28) };
        let width = unsafe { read::<f32>(id + 304) };
        let height = unsafe { read::<u32>(id + 308) } as f32;
        let composition_cursor = if unsafe { read::<u32>(self.base + COMPOSING) } != 0 {
            unsafe { read::<u32>(self.base + COMPOSITION_CURSOR) }
        } else {
            0
        };
        if !width.is_finite() || width <= 0.0 || height == 0.0 {
            return None;
        }
        let columns = i64::from(cursor) + i64::from(extra) - i64::from(scroll)
            + i64::from(composition_cursor);
        let rect = Rect::from_min_size(
            anchor + vec2(columns as f32 * width * 0.5, 0.0),
            vec2(1.0, height),
        );
        rect.is_finite().then_some(rect)
    }

    /// Runs on the window thread while the shared context is still associated.
    /// Read ANSI data from that same context, preserving the native game's IMM
    /// code-page conversion instead of deriving an encoding from its language.
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
        let hwnd = HWND(unsafe { read::<u32>(self.base + WINDOW) } as usize as *mut _);
        let context = unsafe { ImmGetContext(hwnd) };
        if context.is_invalid() {
            return;
        }
        match event {
            ImeEvent::Preedit { .. } => self.preedit(id, context),
            ImeEvent::Commit(_) => self.commit(id, context),
            // The IMM broker emits only Preedit and Commit.
            _ => {}
        }
        unsafe {
            let _ = ImmReleaseContext(hwnd, context);
        }
    }

    fn preedit(&self, id: usize, context: HIMC) {
        let Some(text) = read_data(context, GCS_COMPSTR, COMPOSITION_CAPACITY - 1) else {
            self.clear_preedit();
            return;
        };
        let attributes = read_data(context, GCS_COMPATTR, COMPOSITION_CAPACITY).unwrap_or_default();
        let clauses = read_data(context, GCS_COMPCLAUSE, COMPOSITION_CAPACITY).unwrap_or_default();
        let cursor = unsafe { ImmGetCompositionStringA(context, GCS_CURSORPOS, None, 0) };
        if self.target_id() != Some(id) {
            return;
        }
        unsafe {
            self.copy_composition_data(COMPOSITION, &text);
            self.copy_composition_data(ATTRIBUTES, &attributes);
            self.copy_composition_data(CLAUSES, &clauses);
            write(self.base + COMPOSITION_LEN, text.len() as u32);
            write(self.base + ATTRIBUTES_LEN, attributes.len() as u32);
            write(self.base + CLAUSES_LEN, clauses.len() as u32);
            write(
                self.base + COMPOSITION_CURSOR,
                (cursor.max(0) as usize).min(text.len()) as u32,
            );
            write(self.base + COMPOSING, u32::from(!text.is_empty()));
            write(self.base + CANDIDATES_VISIBLE, 0u32);
            write(self.base + CURSOR_BLINK, 1u32);
        }
    }

    fn commit(&self, id: usize, context: HIMC) {
        if let Some(mut text) = read_data(context, GCS_RESULTSTR, COMPOSITION_CAPACITY - 1)
            && self.target_id() == Some(id)
        {
            text.push(0);
            let destination = (id + TEXT_OFFSET) as *mut u8;
            let cursor = unsafe { read::<u16>(id + 16) };
            let limit = unsafe { read::<u16>(id + 26) };
            let buffer = unsafe { std::slice::from_raw_parts(destination, TEXT_CAPACITY) };
            if valid_insertion(buffer, cursor, limit) {
                let inserted = unsafe {
                    insert_text(
                        self.base + INSERT_TEXT,
                        text.as_ptr(),
                        destination,
                        u32::from(cursor),
                        u32::from(limit),
                    )
                };
                unsafe {
                    write(id + 16, cursor.wrapping_add(inserted as u16));
                    if read::<u8>(id + 320) & 1 != 0 {
                        write(id + 28, 0i32);
                    }
                }
            }
        }
        self.clear_preedit();
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

fn read_data(context: HIMC, flag: IME_COMPOSITION_STRING, capacity: usize) -> Option<Vec<u8>> {
    let size = unsafe { ImmGetCompositionStringA(context, flag, None, 0) };
    let size = usize::try_from(size)
        .ok()
        .filter(|size| *size <= capacity)?;
    if size == 0 {
        return Some(Vec::new());
    }
    let mut data = vec![0; size];
    let copied = unsafe {
        ImmGetCompositionStringA(context, flag, Some(data.as_mut_ptr().cast()), size as u32)
    };
    let copied = usize::try_from(copied)
        .ok()
        .filter(|copied| *copied <= size)?;
    data.truncate(copied);
    Some(data)
}

fn valid_insertion(buffer: &[u8], cursor: u16, limit: u16) -> bool {
    let Some(length) = buffer.iter().position(|byte| *byte == 0) else {
        return false;
    };
    usize::from(cursor) <= length
        && length <= usize::from(limit)
        && usize::from(limit) < TEXT_CAPACITY
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

/// The native routine receives its source in ECX and three caller-cleaned stack
/// arguments. Preserve its DBCS-aware byte limit and insertion behavior exactly.
#[unsafe(naked)]
unsafe extern "C" fn insert_text(
    _target: usize,
    _source: *const u8,
    _destination: *mut u8,
    _cursor: u32,
    _limit: u32,
) -> u32 {
    core::arch::naked_asm!(
        "mov eax, [esp + 4]",
        "mov ecx, [esp + 8]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "push dword ptr [esp + 20]",
        "call eax",
        "add esp, 12",
        "movzx eax, ax",
        "ret",
    );
}

#[cfg(test)]
mod tests {
    use super::{TEXT_CAPACITY, insert_text, native_anchor, valid_insertion};
    use egui::pos2;

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
    fn refuses_native_insertion_outside_the_buffer_and_limit() {
        let mut buffer = [0; TEXT_CAPACITY];
        buffer[..4].copy_from_slice(b"text");
        assert!(valid_insertion(&buffer, 0, 255));
        assert!(valid_insertion(&buffer, 4, 4));
        assert!(!valid_insertion(&buffer, 5, 255));
        assert!(!valid_insertion(&buffer, 4, 3));
        assert!(!valid_insertion(&buffer, 0, 256));
        assert!(!valid_insertion(&[b'x'; TEXT_CAPACITY], 0, 255));
    }

    #[unsafe(naked)]
    unsafe extern "C" fn check_insert_arguments() {
        core::arch::naked_asm!(
            "mov edx, [esp + 4]",
            "mov al, [ecx]",
            "mov [edx], al",
            "mov eax, [esp + 8]",
            "mov [edx + 4], eax",
            "mov eax, [esp + 12]",
            "mov [edx + 8], eax",
            "mov eax, 0xABCD0012",
            "ret",
        );
    }

    #[test]
    fn bridge_passes_native_register_and_stack_arguments_and_returns_ax() {
        let source = b"x\0";
        let mut received = [0u32; 3];
        let inserted = unsafe {
            insert_text(
                check_insert_arguments as *const () as usize,
                source.as_ptr(),
                received.as_mut_ptr().cast(),
                23,
                201,
            )
        };
        assert_eq!(received, [u32::from(b'x'), 23, 201]);
        assert_eq!(inserted, 0x12);
    }
}
