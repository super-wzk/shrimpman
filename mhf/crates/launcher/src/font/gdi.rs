//! GDI ownership and font corrections, independent of the text encoding.

use mhf_hooks::{HookGuard, HookSlot};
use std::{
    collections::HashMap,
    ffi::{CStr, CString, c_void},
    sync::Mutex,
};
use windows_sys::{
    Win32::{
        Foundation::{RECT, SIZE},
        Graphics::Gdi::{
            self, CreateFontW, ETO_GLYPH_INDEX, ETO_OPTIONS, GetCurrentObject, GetTextAlign,
            GetTextMetricsW, HDC, HFONT, HGDIOBJ, OBJ_FONT, TA_BASELINE, TA_TOP, TEXTMETRICW,
        },
    },
    core::{BOOL, PCSTR},
};

// The game positions TA_TOP glyphs for the original MS Gothic design metrics.
const MS_GOTHIC_ASCENT_UNITS: i32 = 220;
const MS_GOTHIC_DESCENT_UNITS: i32 = 36;
const MS_GOTHIC_EM_UNITS: i32 = MS_GOTHIC_ASCENT_UNITS + MS_GOTHIC_DESCENT_UNITS;

pub(crate) type GetTextExtentPoint32AFn =
    unsafe extern "system" fn(HDC, PCSTR, i32, *mut SIZE) -> BOOL;
pub(crate) type ExtTextOutAFn = unsafe extern "system" fn(
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

/// Optional text conversion. Font ownership, measurement and placement stay here.
#[derive(Clone, Copy)]
pub(crate) struct TextRenderer {
    pub(crate) measure: GetTextExtentPoint32AFn,
    pub(crate) draw: ExtTextOutAFn,
}

pub(crate) struct HookState {
    create_font_a: CreateFontAFn,
    get_text_extent_point32_a: GetTextExtentPoint32AFn,
    ext_text_out_a: ExtTextOutAFn,
    delete_object: DeleteObjectFn,
    configured_font_name: CString,
    renderer: Option<TextRenderer>,
    font_correction_cache: Mutex<HashMap<usize, FontCorrectionState>>,
}

static HOOK_STATE: HookSlot<HookState> = HookSlot::new();

/// Install before the game creates its fonts; remove after native text callers stop.
pub(crate) unsafe fn install_game(
    font_name: &[u8],
    renderer: Option<TextRenderer>,
) -> Result<HookGuard<HookState>, String> {
    let font_name = CStr::from_bytes_until_nul(font_name)
        .map_err(|_| "configured font name is not NUL-terminated".to_owned())?;
    let mut hooks = HOOK_STATE.prepare()?;
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
    let state = HookState {
        create_font_a: unsafe { std::mem::transmute::<*mut c_void, CreateFontAFn>(create_font_a) },
        get_text_extent_point32_a: unsafe {
            std::mem::transmute::<*mut c_void, GetTextExtentPoint32AFn>(get_text_extent_point32_a)
        },
        ext_text_out_a: unsafe {
            std::mem::transmute::<*mut c_void, ExtTextOutAFn>(ext_text_out_a)
        },
        delete_object: unsafe { std::mem::transmute::<*mut c_void, DeleteObjectFn>(delete_object) },
        configured_font_name: font_name.to_owned(),
        renderer,
        font_correction_cache: Mutex::new(HashMap::new()),
    };
    unsafe { hooks.install(state) }
}

impl HookState {
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

    unsafe fn correct_extent_height(&self, hdc: HDC, size: *mut SIZE, result: BOOL) {
        if result != 0
            && !size.is_null()
            && let Some(correction) = unsafe { self.correction_for(hdc) }
        {
            unsafe { (*size).cy = correction.extent_height };
        }
    }

    unsafe fn corrected_y(&self, hdc: HDC, y: i32) -> i32 {
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
        state.create_font_a
    });
    let configured = state.is_some_and(|state| unsafe { state.matches_configured_font(face_name) });
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
        state.track_font(font, character_height);
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
    if !unsafe { state.owns_font(hdc) } {
        return unsafe { (state.get_text_extent_point32_a)(hdc, string, count, size) };
    }
    let measure = state
        .renderer
        .map_or(state.get_text_extent_point32_a, |renderer| renderer.measure);
    let result = unsafe { measure(hdc, string, count, size) };
    unsafe { state.correct_extent_height(hdc, size, result) };
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
    if !unsafe { state.owns_font(hdc) } {
        return unsafe { (state.ext_text_out_a)(hdc, x, y, options, rect, string, count, spacing) };
    }
    let y = unsafe { state.corrected_y(hdc, y) };
    // Glyph indices are already shaped, so only their font placement is adjusted.
    let draw = state
        .renderer
        .filter(|_| options & ETO_GLYPH_INDEX == 0)
        .map_or(state.ext_text_out_a, |renderer| renderer.draw);
    unsafe { draw(hdc, x, y, options, rect, string, count, spacing) }
}

unsafe extern "system" fn delete_object_detour(object: HGDIOBJ) -> BOOL {
    let invocation = HOOK_STATE.enter();
    let state = invocation.state();
    let original = state.map_or(Gdi::DeleteObject as DeleteObjectFn, |state| {
        state.delete_object
    });
    let result = unsafe { original(object) };
    if result != 0
        && let Some(state) = state
    {
        state.untrack_font(object);
    }
    result
}

/// Apply the same placement to native Unicode atlas calls that already use W APIs.
#[cfg(feature = "unicode")]
pub(crate) unsafe fn corrected_y(hdc: HDC, y: i32) -> i32 {
    let invocation = HOOK_STATE.enter();
    invocation
        .state()
        .map_or(y, |state| unsafe { state.corrected_y(hdc, y) })
}

#[cfg(test)]
mod tests;
