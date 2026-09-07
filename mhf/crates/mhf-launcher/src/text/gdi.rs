use super::{HOOK_STATE, HookState};
use mhf_hooks::HookSet;
use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_void},
    sync::Mutex,
};
use windows_sys::{
    Win32::{
        Foundation::{RECT, SIZE},
        Graphics::Gdi::{
            self, CreateFontW, ETO_GLYPH_INDEX, ETO_OPTIONS, ETO_PDY, ExtTextOutW,
            GetCurrentObject, GetTextAlign, GetTextExtentPoint32W, GetTextMetricsW, HDC, HFONT,
            HGDIOBJ, OBJ_FONT, TA_BASELINE, TA_TOP, TEXTMETRICW,
        },
    },
    core::{BOOL, PCSTR},
};

// The game positions TA_TOP glyphs for the original MS Gothic design metrics.
const MS_GOTHIC_ASCENT_UNITS: i32 = 220;
const MS_GOTHIC_DESCENT_UNITS: i32 = 36;
const MS_GOTHIC_EM_UNITS: i32 = MS_GOTHIC_ASCENT_UNITS + MS_GOTHIC_DESCENT_UNITS;

type GetTextExtentPoint32AFn = unsafe extern "system" fn(HDC, PCSTR, i32, *mut SIZE) -> BOOL;
type ExtTextOutAFn = unsafe extern "system" fn(
    HDC,
    i32,
    i32,
    ETO_OPTIONS,
    *const RECT,
    PCSTR,
    u32,
    *const i32,
) -> BOOL;
type DeleteObjectFn = unsafe extern "system" fn(HGDIOBJ) -> BOOL;
type CreateFontAFn = unsafe extern "system" fn(
    i32,
    i32,
    i32,
    i32,
    i32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    u32,
    PCSTR,
) -> HFONT;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FontLayoutCorrection {
    extent_height: i32,
    y_offset: i32,
}

#[derive(Clone, Copy)]
enum FontCorrectionState {
    Unmeasured(i32),
    Measured(FontLayoutCorrection),
}

pub(super) struct RenderingHooks {
    create_font_a: CreateFontAFn,
    get_text_extent_point32_a: GetTextExtentPoint32AFn,
    ext_text_out_a: ExtTextOutAFn,
    delete_object: DeleteObjectFn,
    configured_font_name: CString,
    font_correction_cache: Mutex<HashMap<usize, FontCorrectionState>>,
}

pub(super) unsafe fn create_hooks(
    hooks: &mut HookSet<HookState>,
    font_name: &CStr,
) -> Result<RenderingHooks, String> {
    let mut create_hook = |name, detour| unsafe { hooks.create_api(c"gdi32.dll", name, detour) };
    let create_font_a = create_hook(
        c"CreateFontA",
        create_font_a_detour as CreateFontAFn as *mut c_void,
    )?;
    let get_text_extent_point32_a = create_hook(
        c"GetTextExtentPoint32A",
        get_text_extent_point32_a_detour as GetTextExtentPoint32AFn as *mut c_void,
    )?;
    let ext_text_out_a = create_hook(
        c"ExtTextOutA",
        ext_text_out_a_detour as ExtTextOutAFn as *mut c_void,
    )?;
    let delete_object = create_hook(
        c"DeleteObject",
        delete_object_detour as DeleteObjectFn as *mut c_void,
    )?;
    Ok(RenderingHooks {
        create_font_a: unsafe { std::mem::transmute::<*mut c_void, CreateFontAFn>(create_font_a) },
        get_text_extent_point32_a: unsafe {
            std::mem::transmute::<*mut c_void, GetTextExtentPoint32AFn>(get_text_extent_point32_a)
        },
        ext_text_out_a: unsafe {
            std::mem::transmute::<*mut c_void, ExtTextOutAFn>(ext_text_out_a)
        },
        delete_object: unsafe { std::mem::transmute::<*mut c_void, DeleteObjectFn>(delete_object) },
        configured_font_name: font_name.to_owned(),
        font_correction_cache: Mutex::new(HashMap::new()),
    })
}

impl RenderingHooks {
    unsafe fn owns_font(&self, hdc: HDC) -> bool {
        let font = unsafe { GetCurrentObject(hdc, OBJ_FONT as u32) };
        self.font_correction_cache
            .lock()
            .is_ok_and(|cache| cache.contains_key(&(font as usize)))
    }

    unsafe fn correction_for(&self, hdc: HDC) -> Option<FontLayoutCorrection> {
        let font = unsafe { GetCurrentObject(hdc, OBJ_FONT as u32) };
        if font.is_null() {
            return None;
        }

        let mut cache = self.font_correction_cache.lock().ok()?;
        let character_height = match cache.get(&(font as usize))? {
            FontCorrectionState::Measured(correction) => return Some(*correction),
            FontCorrectionState::Unmeasured(character_height) => *character_height,
        };

        let mut metrics = TEXTMETRICW::default();
        if unsafe { GetTextMetricsW(hdc, &raw mut metrics) } == 0 {
            return None;
        }
        let correction = font_layout_correction(character_height, metrics.tmAscent);
        cache.insert(font as usize, FontCorrectionState::Measured(correction));
        Some(correction)
    }

    fn track_font(&self, font: HFONT, character_height: i32) {
        if let Ok(mut cache) = self.font_correction_cache.lock() {
            cache.insert(
                font as usize,
                FontCorrectionState::Unmeasured(character_height),
            );
        }
    }

    unsafe fn matches_configured_font(&self, face_name: PCSTR) -> bool {
        !face_name.is_null()
            && unsafe { CStr::from_ptr(face_name.cast()) }
                .to_bytes()
                .eq_ignore_ascii_case(self.configured_font_name.as_bytes())
    }

    fn untrack_font(&self, object: HGDIOBJ) {
        if let Ok(mut cache) = self.font_correction_cache.lock() {
            cache.remove(&(object as usize));
        }
    }

    pub(super) unsafe fn correct_extent_height(&self, hdc: HDC, size: *mut SIZE, result: BOOL) {
        if result != 0
            && !size.is_null()
            && let Some(correction) = unsafe { self.correction_for(hdc) }
        {
            unsafe { (*size).cy = correction.extent_height };
        }
    }

    pub(super) unsafe fn corrected_y(&self, hdc: HDC, y: i32) -> i32 {
        if unsafe { GetTextAlign(hdc) } & TA_BASELINE != TA_TOP {
            return y;
        }
        unsafe { self.correction_for(hdc) }
            .map_or(y, |correction| y.saturating_add(correction.y_offset))
    }
}

fn font_layout_correction(character_height: i32, ascent: i32) -> FontLayoutCorrection {
    let reference_ascent = ((i64::from(character_height) * i64::from(MS_GOTHIC_ASCENT_UNITS)
        + i64::from(MS_GOTHIC_EM_UNITS / 2))
        / i64::from(MS_GOTHIC_EM_UNITS)) as i32;
    FontLayoutCorrection {
        extent_height: character_height,
        y_offset: reference_ascent.saturating_sub(ascent),
    }
}

#[allow(clippy::too_many_arguments)]
unsafe extern "system" fn create_font_a_detour(
    height: i32,
    width: i32,
    escapement: i32,
    orientation: i32,
    weight: i32,
    italic: u32,
    underline: u32,
    strike_out: u32,
    character_set: u32,
    output_precision: u32,
    clip_precision: u32,
    quality: u32,
    pitch_and_family: u32,
    face_name: PCSTR,
) -> HFONT {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let original = state.map_or(Gdi::CreateFontA as CreateFontAFn, |state| {
        state.gdi.create_font_a
    });
    let configured =
        state.is_some_and(|state| unsafe { state.gdi.matches_configured_font(face_name) });
    let wide_name = if configured {
        unsafe { CStr::from_ptr(face_name.cast()) }
            .to_str()
            .ok()
            .map(|name| name.encode_utf16().chain([0]).collect::<Vec<_>>())
    } else {
        None
    };
    let font = unsafe {
        if let Some(name) = wide_name.as_ref() {
            CreateFontW(
                height,
                width,
                escapement,
                orientation,
                weight,
                italic,
                underline,
                strike_out,
                character_set,
                output_precision,
                clip_precision,
                quality,
                pitch_and_family,
                name.as_ptr(),
            )
        } else {
            original(
                height,
                width,
                escapement,
                orientation,
                weight,
                italic,
                underline,
                strike_out,
                character_set,
                output_precision,
                clip_precision,
                quality,
                pitch_and_family,
                face_name,
            )
        }
    };
    if !font.is_null()
        && let Some(state) = state
        && configured
        && let Some(character_height) = height.checked_abs().filter(|height| *height != 0)
    {
        state.gdi.track_font(font, character_height);
    }
    font
}

unsafe extern "system" fn get_text_extent_point32_a_detour(
    hdc: HDC,
    string: PCSTR,
    count: i32,
    size: *mut SIZE,
) -> BOOL {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return unsafe { Gdi::GetTextExtentPoint32A(hdc, string, count, size) };
    };
    if !unsafe { state.gdi.owns_font(hdc) } {
        return unsafe { (state.gdi.get_text_extent_point32_a)(hdc, string, count, size) };
    }
    let Some(text) = (unsafe { utf8_text(string, count.try_into().ok()) }) else {
        return 0;
    };
    let utf16 = text.encode_utf16().collect::<Vec<_>>();
    let result = unsafe { GetTextExtentPoint32W(hdc, utf16.as_ptr(), utf16.len() as i32, size) };
    unsafe { state.gdi.correct_extent_height(hdc, size, result) };
    result
}

unsafe extern "system" fn ext_text_out_a_detour(
    hdc: HDC,
    x: i32,
    y: i32,
    options: ETO_OPTIONS,
    rect: *const RECT,
    string: PCSTR,
    count: u32,
    spacing: *const i32,
) -> BOOL {
    let invocation = HOOK_STATE.enter();
    let Some(state) = invocation.state() else {
        return unsafe { Gdi::ExtTextOutA(hdc, x, y, options, rect, string, count, spacing) };
    };
    if !unsafe { state.gdi.owns_font(hdc) } || options & ETO_GLYPH_INDEX != 0 {
        return unsafe {
            (state.gdi.ext_text_out_a)(hdc, x, y, options, rect, string, count, spacing)
        };
    }
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
    let y = unsafe { state.gdi.corrected_y(hdc, y) };
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

unsafe extern "system" fn delete_object_detour(object: HGDIOBJ) -> BOOL {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let original = state.map_or(Gdi::DeleteObject as DeleteObjectFn, |state| {
        state.gdi.delete_object
    });
    let result = unsafe { original(object) };
    if result != 0
        && let Some(state) = state
    {
        state.gdi.untrack_font(object);
    }
    result
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
    use super::{FontLayoutCorrection, font_layout_correction, utf16_advances};

    #[test]
    fn preserves_byte_advances_across_utf8_and_surrogate_pairs() {
        assert_eq!(
            utf16_advances("A中🙂", &[1, 2, 3, 4, 5, 6, 7, 8], 1),
            [1, 9, 0, 26]
        );
        assert_eq!(utf16_advances("é", &[2, 10, 3, 20], 2), [5, 30]);
    }

    #[test]
    fn derives_ms_gothic_layout_correction() {
        assert_eq!(
            font_layout_correction(29, 25),
            FontLayoutCorrection {
                extent_height: 29,
                y_offset: 0,
            }
        );
        assert_eq!(
            font_layout_correction(29, 30),
            FontLayoutCorrection {
                extent_height: 29,
                y_offset: -5,
            }
        );
    }
}
