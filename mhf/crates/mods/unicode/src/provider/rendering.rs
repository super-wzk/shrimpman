use super::{CodeHook, HOOK_STATE, HookState, utf8};
use std::{
    collections::HashMap,
    ffi::{CStr, c_void},
    mem, ptr,
    sync::{
        PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};
use unicode_segmentation::UnicodeSegmentation;
use windows::{
    Win32::Graphics::Direct3D9::{D3DLOCKED_RECT, IDirect3DTexture9},
    core::Interface,
};
use windows_sys::Win32::Graphics::Gdi::{BLACKNESS, ExtTextOutW, GdiFlush, HDC, PatBlt};

const CONTEXT_RVA: usize = 0x0E3C_BD64;
const GLYPH_HDC_RVA: usize = 0x0E38_E67C;
const GLYPH_PIXELS_RVA: usize = 0x0E38_E678;
const QUALITY_RVA: usize = 0x0E73_AB44;
const CAPACITY: usize = 896;
const PENDING_COUNT: usize = 144;
const GENERATED_COUNT: usize = 140;
const PENDING_SLOTS: usize = 148;
const NEXT_SLOT: usize = 133012;
const ATLASES: usize = 133016;
const TEXTURES_RVA: usize = 0x01AA_7D80;

static COLLECT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static RASTER_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static DRAW_TEXT_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static DRAW_GLYPH_ORIGINAL: AtomicUsize = AtomicUsize::new(0);
static UPLOAD_ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// These are atlas slots, never an encoding written into game text buffers.
/// Store full graphemes so UTF-16 shaping sees combining marks and emoji joins.
#[derive(Default)]
pub(super) struct GlyphCache {
    context: usize,
    slots: HashMap<Box<str>, u16>,
    glyphs: Vec<Box<str>>,
}

impl GlyphCache {
    fn synchronize(&mut self, context: usize, pending: usize) {
        if self.context != context {
            self.context = context;
            self.slots.clear();
            self.glyphs.clear();
        } else if pending < self.glyphs.len() {
            // The frame reset rolls pending back to successfully uploaded slots.
            // Retain that prefix: its textures are still valid and generated is
            // still the prefix length. Reassigning those slots would draw stale
            // pixels after a later page failed to lock or unlock.
            self.glyphs.truncate(pending);
            self.slots.retain(|_, slot| usize::from(*slot) < pending);
        }
    }

    fn insert(&mut self, text: &str) -> Option<(u16, bool)> {
        if let Some(slot) = self.slots.get(text) {
            return Some((*slot, false));
        }
        if self.glyphs.len() == CAPACITY {
            return None;
        }
        let slot = self.glyphs.len() as u16;
        self.slots.insert(text.into(), slot);
        self.glyphs.push(text.into());
        Some((slot, true))
    }
}

#[repr(C)]
#[derive(Clone, Copy)]
struct DrawRecord {
    x: i16,
    y: i16,
    scale: f32,
    width: u8,
    height: u8,
    style: u8,
    _padding: u8,
    color: u32,
    text: u32,
}

#[repr(C)]
#[derive(Clone, Copy)]
struct TexturedQuad {
    x: i16,
    y: i16,
    width: i16,
    height: i16,
    color: u32,
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
}

#[repr(C)]
struct SolidQuad {
    left: i16,
    top: i16,
    right: i16,
    bottom: i16,
    color: u32,
}

pub(super) const fn required_image_end() -> usize {
    // The IME group also uses the active editor global near the image end.
    0x0EDB_A1C0
}

pub(super) fn code_hooks() -> Vec<CodeHook> {
    vec![
        CodeHook {
            name: "Unicode atlas upload",
            rva: 0x014D_F2C0,
            signature: &[
                (0, 0x56),
                (1, 0x57),
                (2, 0xE8),
                (7, 0x85),
                (8, 0xC0),
                (9, 0x74),
            ],
            detour: upload_hook as *const () as *mut c_void,
            original: &UPLOAD_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 glyph collection",
            rva: 0x014D_F4C0,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x83),
                (4, 0xEC),
                (5, 0x14),
            ],
            detour: collect_hook as *const () as *mut c_void,
            original: &COLLECT_ORIGINAL,
        },
        CodeHook {
            name: "Unicode atlas rasterization",
            rva: 0x014D_3410,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x83),
                (4, 0xEC),
                (5, 0x10),
                (6, 0x53),
            ],
            detour: raster_hook as *const () as *mut c_void,
            original: &RASTER_ORIGINAL,
        },
        CodeHook {
            name: "UTF-8 text drawing",
            rva: 0x014D_EEC0,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x51),
                (4, 0x53),
                (5, 0x8B),
            ],
            detour: draw_text_hook as *const () as *mut c_void,
            original: &DRAW_TEXT_ORIGINAL,
        },
        CodeHook {
            name: "Unicode atlas lookup",
            rva: 0x014D_EFE0,
            signature: &[
                (0, 0x55),
                (1, 0x8B),
                (2, 0xEC),
                (3, 0x8B),
                (4, 0x0D),
                (9, 0x83),
            ],
            detour: draw_glyph_hook as *const () as *mut c_void,
            original: &DRAW_GLYPH_ORIGINAL,
        },
    ]
}

unsafe fn context(state: &HookState) -> usize {
    unsafe { ptr::read((state.module_base + CONTEXT_RVA) as *const u32) as usize }
}

unsafe extern "C" fn collect_hook() -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { mem::transmute(COLLECT_ORIGINAL.load(Ordering::Acquire)) };
        return unsafe { original() };
    };
    let context = unsafe { context(state) };
    if context == 0 {
        return 0;
    }
    let pending = unsafe { ptr::read((context + PENDING_COUNT) as *const u32) } as usize;
    let mut cache = state.glyphs.lock().unwrap_or_else(PoisonError::into_inner);
    // The native upload routine resets its counters before rebuilding an atlas.
    cache.synchronize(context, pending);
    for layer in 0..7 {
        let count = unsafe { ptr::read((context + 28 + 4 * layer) as *const i32) };
        let commands =
            unsafe { ptr::read((context + 56 + 4 * layer) as *const u32) } as *const DrawRecord;
        if commands.is_null() || count <= 0 {
            continue;
        }
        for index in 0..count as usize {
            let command = unsafe { ptr::read_unaligned(commands.add(index)) };
            if command.text == 0 {
                continue;
            }
            let source = unsafe { CStr::from_ptr(command.text as *const i8) };
            let text = source.to_string_lossy();
            for glyph in text.graphemes(true).filter(|glyph| needs_glyph(glyph)) {
                let Some((slot, added)) = cache.insert(glyph) else {
                    return 1;
                };
                if added {
                    unsafe {
                        ptr::write(
                            (context + PENDING_SLOTS + usize::from(slot) * 2) as *mut u16,
                            slot,
                        );
                        ptr::write(
                            (context + PENDING_COUNT) as *mut u32,
                            cache.glyphs.len() as u32,
                        );
                        ptr::write((context + NEXT_SLOT) as *mut u32, cache.glyphs.len() as u32);
                    }
                }
            }
        }
    }
    0
}

fn needs_glyph(text: &str) -> bool {
    !text
        .chars()
        .all(|character| character.is_whitespace() || character.is_control())
}

unsafe extern "C" fn upload_hook() -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn() -> i32 =
            unsafe { mem::transmute(UPLOAD_ORIGINAL.load(Ordering::Acquire)) };
        return unsafe { original() };
    };
    let context = unsafe { context(state) };
    if context == 0 {
        return 0;
    }
    if unsafe { collect_hook() } != 0 {
        // Keep the native frame/cache rebuild policy, without its obsolete u16
        // character-index table or its inclusive end-page texture access.
        unsafe {
            ptr::write((context + NEXT_SLOT) as *mut u32, 0);
            ptr::write((context + GENERATED_COUNT) as *mut u32, 0);
            ptr::write((context + PENDING_COUNT) as *mut u32, 0);
            ptr::write_bytes((context + 104) as *mut u32, 0, 7);
            collect_hook();
        }
    }
    let pending = unsafe { ptr::read((context + PENDING_COUNT) as *const u32) } as usize;
    let mut generated = unsafe { ptr::read((context + GENERATED_COUNT) as *const u32) } as usize;
    while generated < pending.min(CAPACITY) {
        let sheet = generated / 64;
        let handle = unsafe { ptr::read((context + ATLASES + sheet * 4) as *const u32) } as usize;
        let raw = unsafe {
            ptr::read((state.module_base + TEXTURES_RVA + handle * 216) as *const *mut c_void)
        };
        let Some(texture) = (unsafe { IDirect3DTexture9::from_raw_borrowed(&raw) }) else {
            return 0;
        };
        let mut locked = D3DLOCKED_RECT::default();
        if unsafe { texture.LockRect(0, &mut locked, ptr::null(), 0) }.is_err() {
            return 0;
        }
        let end = pending.min((sheet + 1) * 64).min(CAPACITY);
        if locked.pBits.is_null() || locked.Pitch < 256 * 4 {
            let _ = unsafe { texture.UnlockRect(0) };
            return 0;
        }
        let pitch = locked.Pitch as usize / 4;
        for slot in generated..end {
            let glyph = state
                .glyphs
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .glyphs
                .get(slot)
                .cloned();
            if let Some(glyph) = glyph {
                let cell = slot % 64;
                let destination = unsafe {
                    locked
                        .pBits
                        .cast::<u32>()
                        .add((cell / 8) * 32 * pitch + (cell % 8) * 32)
                };
                unsafe {
                    rasterize(state, &glyph, destination, pitch);
                }
            }
        }
        if unsafe { texture.UnlockRect(0) }.is_err() {
            return 0;
        }
        generated = end;
        unsafe {
            ptr::write((context + GENERATED_COUNT) as *mut u32, generated as u32);
        }
    }
    0
}

#[unsafe(naked)]
unsafe extern "C" fn raster_hook() {
    core::arch::naked_asm!(
        "pushfd", "pushad", "mov ecx, esp", "push ecx",
        "call {dispatch}", "add esp, 4", "test eax, eax", "jz 2f",
        "popad", "popfd", "ret",
        "2:", "popad", "popfd", "jmp dword ptr [{original}]",
        dispatch = sym raster_dispatch,
        original = sym RASTER_ORIGINAL,
    );
}

unsafe extern "C" fn raster_dispatch(registers: *mut u32) -> u32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return 0;
    };
    let slot = unsafe { *registers.add(7) } as u16;
    let stack = unsafe { *registers.add(3) } as usize + 4;
    let destination = unsafe { ptr::read((stack + 4) as *const u32) } as *mut u32;
    let glyph = state
        .glyphs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .glyphs
        .get(usize::from(slot))
        .cloned();
    if let Some(glyph) = glyph
        && !destination.is_null()
    {
        unsafe { rasterize(state, &glyph, destination, 256) };
    }
    // Preserve the original return convention: its source DIB end pointer.
    let pixels = unsafe { ptr::read((state.module_base + GLYPH_PIXELS_RVA) as *const u32) };
    unsafe {
        *registers.add(7) = pixels.wrapping_add(32 * 32 * 4);
    }
    1
}

unsafe fn rasterize(state: &HookState, text: &str, destination: *mut u32, pitch: usize) {
    let hdc = unsafe { ptr::read((state.module_base + GLYPH_HDC_RVA) as *const u32) } as HDC;
    let pixels =
        unsafe { ptr::read((state.module_base + GLYPH_PIXELS_RVA) as *const u32) } as *const u32;
    if hdc.is_null() || pixels.is_null() {
        return;
    }
    let wide = text.encode_utf16().collect::<Vec<_>>();
    let columns = utf8::display_columns(text);
    let y = unsafe { mhf_font::corrected_y(hdc, 2) };
    unsafe {
        PatBlt(hdc, 0, 0, 32, 32, BLACKNESS);
        ExtTextOutW(
            hdc,
            0,
            y,
            2,
            ptr::null(),
            wide.as_ptr(),
            wide.len() as u32,
            ptr::null(),
        );
        GdiFlush();
        copy_pixels(pixels, destination, pitch, columns != 1);
    }
}

unsafe fn copy_pixels(source: *const u32, destination: *mut u32, pitch: usize, wide: bool) {
    for y in 0..32 {
        for x in 0..32 {
            let pixel = if wide || x < 16 {
                (unsafe { *source.add(y * 32 + x) } << 24) | 0x00FF_FFFF
            } else {
                0
            };
            unsafe {
                *destination.add(y * pitch + x) = pixel;
            }
        }
    }
}

unsafe extern "C" fn draw_text_hook(source: *const u8, record: *const DrawRecord) -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(*const u8, *const DrawRecord) -> i32 =
            unsafe { mem::transmute(DRAW_TEXT_ORIGINAL.load(Ordering::Acquire)) };
        return unsafe { original(source, record) };
    };
    let context = unsafe { context(state) };
    if context == 0 || source.is_null() || record.is_null() {
        return 0;
    }
    let record = unsafe { ptr::read_unaligned(record) };
    let bytes = unsafe { CStr::from_ptr(source.cast()) };
    let text = bytes.to_string_lossy();
    let mut x = record.x;
    let mut y = record.y;
    let half_width = unsafe { ptr::read((context + 96) as *const u32) } != 0;
    for glyph in text.graphemes(true) {
        if glyph == "\n" || glyph == "\r\n" {
            x = record.x;
            y = y.wrapping_add(i16::from(record.height));
            continue;
        }
        unsafe {
            ptr::write((context + 20) as *mut i16, x);
            ptr::write((context + 22) as *mut i16, y);
        }
        let columns = utf8::display_columns(glyph);
        if needs_glyph(glyph) {
            unsafe { draw_glyph(state, context, glyph, &record, false) };
        }
        x = x.wrapping_add(advance(columns, record.width, half_width));
    }
    unsafe {
        ptr::write((context + 20) as *mut i16, x);
        ptr::write((context + 22) as *mut i16, y);
    }
    0
}

fn advance(columns: usize, width: u8, half_width: bool) -> i16 {
    match columns {
        0 => 0,
        1 if !half_width => (u16::from(width) * 2 / 3) as i16,
        _ => (columns * usize::from(width) / 2) as i16,
    }
}

unsafe extern "C" fn draw_glyph_hook(
    codepoint: u32,
    record: *const DrawRecord,
    outline: i32,
) -> i32 {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        let original: unsafe extern "C" fn(u32, *const DrawRecord, i32) -> i32 =
            unsafe { mem::transmute(DRAW_GLYPH_ORIGINAL.load(Ordering::Acquire)) };
        return unsafe { original(codepoint, record, outline) };
    };
    let context = unsafe { context(state) };
    if context == 0 || record.is_null() {
        return 0;
    }
    if let Some(character) = char::from_u32(codepoint) {
        let mut bytes = [0; 4];
        let glyph = character.encode_utf8(&mut bytes);
        unsafe { draw_glyph(state, context, glyph, &*record, outline != 0) };
    }
    0
}

unsafe fn draw_glyph(
    state: &HookState,
    context: usize,
    glyph: &str,
    record: &DrawRecord,
    outline: bool,
) {
    let slot = state
        .glyphs
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .slots
        .get(glyph)
        .copied();
    let Some(slot) = slot else {
        return;
    };
    let base = state.module_base;
    let x = unsafe { ptr::read((context + 20) as *const i16) };
    let y = unsafe { ptr::read((context + 22) as *const i16) };
    let atlas =
        unsafe { ptr::read((context + ATLASES + usize::from(slot) / 64 * 4) as *const u32) };
    let set_state: unsafe extern "fastcall" fn(u32, u32) =
        unsafe { mem::transmute(base + 0x0000_C7D0) };
    let quality = unsafe { ptr::read((base + QUALITY_RVA) as *const u32) };
    let point_filter = quality == 3 || (quality != 4 && record.width == 32 && record.height == 32);
    unsafe {
        set_state(atlas, 4);
        set_state(if point_filter { 0x10000 } else { 0 }, 99);
    }
    let cell = slot % 64;
    let mut quad = TexturedQuad {
        x,
        y,
        width: i16::from(record.width),
        height: i16::from(record.height),
        color: record.color,
        left: (cell % 8 * 32) as i16,
        top: (cell / 8 * 32) as i16,
        right: (cell % 8 * 32 + 32) as i16,
        bottom: (cell / 8 * 32 + 32) as i16,
    };
    let mode: unsafe extern "C" fn(i32) = unsafe { mem::transmute(base + 0x0001_0D50) };
    let textured: unsafe extern "C" fn(*const TexturedQuad) =
        unsafe { mem::transmute(base + 0x0000_E800) };
    let solid: unsafe extern "thiscall" fn(*const SolidQuad) =
        unsafe { mem::transmute(base + 0x0000_DEC0) };
    let submit: unsafe extern "C" fn() = unsafe { mem::transmute(base + 0x0001_1060) };
    if outline {
        let brightness =
            (record.color & 0xFF) + ((record.color >> 8) & 0xFF) + ((record.color >> 16) & 0xFF);
        quad.color =
            (record.color >> 2) & 0x3F00_0000 | if brightness > 0x40 { 0 } else { 0x00FF_0000 };
        quad.width += 2;
        quad.height += 2;
        for (dx, dy) in [(-1, -1), (0, 0), (-1, 1), (1, -1)] {
            quad.x = x.wrapping_add(dx);
            quad.y = y.wrapping_add(dy);
            unsafe {
                mode(1);
                textured(&quad);
                submit();
            }
        }
        return;
    }
    if matches!(record.style, 2 | 3) {
        let columns = utf8::display_columns(glyph);
        let background = SolidQuad {
            left: x,
            top: y,
            right: x.wrapping_add((usize::from(record.width) * columns / 2) as i16),
            bottom: y.wrapping_add(i16::from(record.height)),
            color: if record.style == 2 {
                0xFF80_8080
            } else {
                0xFF57_FFFF
            },
        };
        unsafe {
            mode(0);
            solid(&background);
            submit();
        }
        if record.style == 3 {
            quad.color ^= 0x00FF_FFFF;
        }
    }
    unsafe {
        mode(1);
        textured(&quad);
        submit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atlas_keys_preserve_supplementary_and_combined_characters() {
        let mut cache = GlyphCache::default();
        for text in ["A", "中", "𠮷", "😀", "e\u{301}", "👩‍👩‍👧‍👦"] {
            let (slot, added) = cache.insert(text).unwrap();
            assert!(added);
            assert_eq!(&*cache.glyphs[usize::from(slot)], text);
            assert_eq!(cache.insert(text), Some((slot, false)));
        }
        cache.synchronize(42, 0);
        assert!(cache.glyphs.is_empty());
    }

    #[test]
    fn collection_and_drawing_use_the_same_grapheme_boundaries() {
        let text = "A中e\u{301}👩‍👩‍👧‍👦";
        let glyphs = text.graphemes(true).collect::<Vec<_>>();
        assert_eq!(glyphs, ["A", "中", "e\u{301}", "👩‍👩‍👧‍👦"]);
        assert_eq!(
            glyphs
                .iter()
                .map(|glyph| utf8::display_columns(glyph))
                .sum::<usize>(),
            utf8::display_columns(text)
        );
    }

    #[test]
    fn failed_upload_retry_keeps_the_successfully_uploaded_prefix() {
        let mut cache = GlyphCache::default();
        cache.synchronize(42, 0);
        for index in 0..100 {
            cache.insert(&char::from_u32(0x4E00 + index).unwrap().to_string());
        }
        cache.synchronize(42, 64);
        assert_eq!(cache.glyphs.len(), 64);
        assert_eq!(cache.insert("一"), Some((0, false)));
        assert_eq!(cache.insert("🙂"), Some((64, true)));
        assert_eq!(&*cache.glyphs[0], "一");
        cache.synchronize(42, 0);
        assert!(cache.glyphs.is_empty());
    }

    #[test]
    fn atlas_writes_respect_texture_pitch_and_clear_narrow_cell_remainders() {
        let source = [0x0000_007Fu32; 32 * 32];
        let mut destination = vec![0xDEAD_BEEF; 256 * 32];
        unsafe {
            copy_pixels(source.as_ptr(), destination.as_mut_ptr(), 256, false);
        }
        for row in destination.as_chunks::<256>().0 {
            assert!(row[..16].iter().all(|pixel| *pixel == 0x7FFF_FFFF));
            assert!(row[16..32].iter().all(|pixel| *pixel == 0));
            assert!(row[32..].iter().all(|pixel| *pixel == 0xDEAD_BEEF));
        }
    }

    #[test]
    fn native_record_layout_and_width_modes_remain_compatible() {
        assert_eq!(mem::size_of::<DrawRecord>(), 20);
        assert_eq!(mem::offset_of!(DrawRecord, text), 16);
        assert_eq!(mem::size_of::<TexturedQuad>(), 20);
        assert_eq!(mem::size_of::<SolidQuad>(), 12);
        assert_eq!(advance(0, 24, true), 0);
        assert_eq!(advance(1, 24, true), 12);
        assert_eq!(advance(1, 24, false), 16);
        assert_eq!(advance(2, 24, true), 24);
    }
}
