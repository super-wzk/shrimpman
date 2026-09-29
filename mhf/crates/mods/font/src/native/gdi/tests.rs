use super::*;
use std::{
    ptr,
    sync::atomic::{AtomicI32, Ordering},
};
use windows_sys::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLACKNESS, CreateCompatibleDC, CreateDIBSection,
    DEFAULT_QUALITY, DIB_RGB_COLORS, DeleteDC, GdiFlush, HBITMAP, PatBlt, SHIFTJIS_CHARSET,
    SelectObject, SetBkColor, SetTextAlign, SetTextColor,
};

#[test]
fn derives_ms_gothic_layout_correction() {
    assert_eq!(
        font_layout_correction(29, 25),
        FontLayoutCorrection {
            extent_height: 29,
            y_offset: 0
        }
    );
    assert_eq!(
        font_layout_correction(29, 30),
        FontLayoutCorrection {
            extent_height: 29,
            y_offset: -5
        }
    );
}

struct Surface {
    hdc: HDC,
    font: HFONT,
    bitmap: HBITMAP,
    old_font: HGDIOBJ,
    old_bitmap: HGDIOBJ,
    pixels: *mut u32,
    width: i32,
    height: i32,
}

impl Surface {
    fn new() -> Self {
        Self::with_font(96, 64, c"Arial")
    }

    fn with_font(width: i32, height: i32, family: &CStr) -> Self {
        unsafe {
            let hdc = CreateCompatibleDC(ptr::null_mut());
            assert!(!hdc.is_null());
            let info = BITMAPINFO {
                bmiHeader: BITMAPINFOHEADER {
                    biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
                    biWidth: width,
                    biHeight: -height,
                    biPlanes: 1,
                    biBitCount: 32,
                    biCompression: BI_RGB,
                    ..Default::default()
                },
                ..Default::default()
            };
            let mut pixels = ptr::null_mut();
            let bitmap = CreateDIBSection(
                hdc,
                &info,
                DIB_RGB_COLORS,
                &raw mut pixels,
                ptr::null_mut(),
                0,
            );
            assert!(!bitmap.is_null());
            let font = Gdi::CreateFontA(
                -29,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                SHIFTJIS_CHARSET as u32,
                0,
                0,
                DEFAULT_QUALITY as u32,
                0,
                family.as_ptr().cast(),
            );
            assert!(!font.is_null());
            let old_bitmap = SelectObject(hdc, bitmap);
            let old_font = SelectObject(hdc, font);
            SetTextAlign(hdc, TA_TOP);
            SetBkColor(hdc, 0);
            SetTextColor(hdc, 0x00FF_FFFF);
            Self {
                hdc,
                font,
                bitmap,
                old_font,
                old_bitmap,
                pixels: pixels.cast(),
                width,
                height,
            }
        }
    }

    fn clear(&self) {
        unsafe {
            assert_ne!(
                PatBlt(self.hdc, 0, 0, self.width, self.height, BLACKNESS),
                0
            )
        };
    }

    fn pixels(&self) -> Vec<u32> {
        unsafe {
            GdiFlush();
            std::slice::from_raw_parts(self.pixels, (self.width * self.height) as usize).to_vec()
        }
    }
}

impl Drop for Surface {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.hdc, self.old_font);
            SelectObject(self.hdc, self.old_bitmap);
            Gdi::DeleteObject(self.font);
            Gdi::DeleteObject(self.bitmap);
            DeleteDC(self.hdc);
        }
    }
}

static DRAW_Y: AtomicI32 = AtomicI32::new(i32::MIN);

unsafe extern "system" fn measure_text(_: HDC, _: PCSTR, _: i32, size: *mut SIZE) -> BOOL {
    unsafe { *size = SIZE { cx: 37, cy: 90 } };
    1
}

unsafe extern "system" fn record_draw_y(
    _: HDC,
    _: i32,
    y: i32,
    _: ETO_OPTIONS,
    _: *const RECT,
    _: PCSTR,
    _: u32,
    _: *const i32,
) -> BOOL {
    DRAW_Y.store(y, Ordering::Relaxed);
    1
}

// One test owns the process-global GDI group across all cases.
#[test]
fn native_encoding_keeps_font_corrections_without_unicode() {
    let mut hooks = unsafe { install_game(c"Arial".to_bytes_with_nul(), None) }.unwrap();
    {
        let surface = Surface::new();
        let invocation = HOOK_STATE.enter();
        let state = invocation.state().unwrap();
        let native = b"\x82\xA0A"; // CP932, intentionally invalid UTF-8.
        let mut original_size = SIZE::default();
        let mut size = SIZE::default();
        unsafe {
            assert_ne!(
                (state.get_text_extent_point32_a)(
                    surface.hdc,
                    native.as_ptr(),
                    native.len() as i32,
                    &raw mut original_size
                ),
                0
            );
            assert_ne!(
                Gdi::GetTextExtentPoint32A(
                    surface.hdc,
                    native.as_ptr(),
                    native.len() as i32,
                    &raw mut size
                ),
                0
            );
        }
        assert_eq!(
            size.cx, original_size.cx,
            "retain the native encoding's width"
        );
        assert_eq!(size.cy, 29, "correct height without any Unicode renderer");

        // Exercise a nonzero change even if this machine's fallback font has
        // exactly the original MS Gothic metrics.
        let correction = FontLayoutCorrection {
            extent_height: original_size.cy + 7,
            y_offset: -5,
        };
        state.font_correction_cache.lock().unwrap().insert(
            surface.font as usize,
            FontCorrectionState::Measured(correction),
        );
        unsafe {
            assert_ne!(
                Gdi::GetTextExtentPoint32A(
                    surface.hdc,
                    native.as_ptr(),
                    native.len() as i32,
                    &raw mut size,
                ),
                0
            );
        }
        assert_eq!(size.cy, correction.extent_height);

        surface.clear();
        unsafe {
            assert_ne!(
                (state.ext_text_out_a)(
                    surface.hdc,
                    4,
                    10 + correction.y_offset,
                    0,
                    ptr::null(),
                    native.as_ptr(),
                    native.len() as u32,
                    ptr::null()
                ),
                0
            );
        }
        let expected = surface.pixels();
        assert!(expected.iter().any(|pixel| *pixel != 0));
        surface.clear();
        unsafe {
            assert_ne!(
                Gdi::ExtTextOutA(
                    surface.hdc,
                    4,
                    10,
                    0,
                    ptr::null(),
                    native.as_ptr(),
                    native.len() as u32,
                    ptr::null()
                ),
                0
            );
        }
        assert_eq!(
            surface.pixels(),
            expected,
            "apply the baseline shift to native A drawing"
        );

        unsafe { SetTextAlign(surface.hdc, TA_BASELINE) };
        assert_eq!(unsafe { state.corrected_y(surface.hdc, 10) }, 10);

        // A font created outside this group's configured name remains untouched.
        unsafe { SelectObject(surface.hdc, surface.old_font) };
        unsafe {
            assert_ne!(
                (state.get_text_extent_point32_a)(
                    surface.hdc,
                    native.as_ptr(),
                    native.len() as i32,
                    &raw mut original_size
                ),
                0
            );
            assert_ne!(
                Gdi::GetTextExtentPoint32A(
                    surface.hdc,
                    native.as_ptr(),
                    native.len() as i32,
                    &raw mut size
                ),
                0
            );
        }
        assert_eq!((size.cx, size.cy), (original_size.cx, original_size.cy));
    }
    hooks.uninstall().unwrap();

    let renderer = TextRenderer {
        measure: measure_text,
        draw: record_draw_y,
    };
    let mut hooks = unsafe { install_game(c"Arial".to_bytes_with_nul(), Some(renderer)) }.unwrap();
    {
        let surface = Surface::new();
        let invocation = HOOK_STATE.enter();
        let state = invocation.state().unwrap();
        // Give this tracked font a nonzero correction so an accidental double
        // application cannot hide behind the machine's installed font metrics.
        state.font_correction_cache.lock().unwrap().insert(
            surface.font as usize,
            FontCorrectionState::Measured(FontLayoutCorrection {
                extent_height: 29,
                y_offset: -5,
            }),
        );
        unsafe {
            assert_ne!(
                Gdi::ExtTextOutA(
                    surface.hdc,
                    4,
                    10,
                    0,
                    ptr::null(),
                    c"A".as_ptr().cast(),
                    1,
                    ptr::null()
                ),
                0
            );
        }
        assert_eq!(
            DRAW_Y.load(Ordering::Relaxed),
            5,
            "the adapter receives one placement correction"
        );
        let mut size = SIZE::default();
        unsafe {
            assert_ne!(
                Gdi::GetTextExtentPoint32A(surface.hdc, c"A".as_ptr().cast(), 1, &raw mut size),
                0
            );
        }
        assert_eq!(
            (size.cx, size.cy),
            (37, 29),
            "correct the adapter result after conversion"
        );

        DRAW_Y.store(i32::MIN, Ordering::Relaxed);
        let glyph = [1u16];
        unsafe {
            Gdi::ExtTextOutA(
                surface.hdc,
                4,
                10,
                ETO_GLYPH_INDEX,
                ptr::null(),
                glyph.as_ptr().cast(),
                1,
                ptr::null(),
            );
        }
        assert_eq!(
            DRAW_Y.load(Ordering::Relaxed),
            i32::MIN,
            "glyph indices bypass text conversion"
        );
    }
    hooks.uninstall().unwrap();

    native_atlas_clears_previous_glyph_pixels();
}

fn native_atlas_clears_previous_glyph_pixels() {
    let _registration = crate::native::register_for(crate::FAMILY_NAME).unwrap();
    let family = CString::new(crate::FAMILY_NAME).unwrap();
    let mut hooks = unsafe { install_game(family.as_bytes_with_nul(), None) }.unwrap();
    {
        let surface = Surface::with_font(32, 32, &family);
        let mut metrics = TEXTMETRICW::default();
        unsafe { assert_ne!(GetTextMetricsW(surface.hdc, &raw mut metrics), 0) };
        let reference_y = 2 + font_layout_correction(29, metrics.tmAscent).y_offset;
        let invocation = HOOK_STATE.enter();
        let original = invocation.state().unwrap().ext_text_out_a;

        // Fullwidth, halfwidth, and a missing glyph in the configured font.
        for (character, bytes) in [
            ('頭', b"\x93\xAA".as_slice()),
            ('0', b"0".as_slice()),
            ('０', b"\x82\x4F".as_slice()),
        ] {
            surface.clear();
            unsafe {
                assert_ne!(
                    original(
                        surface.hdc,
                        0,
                        reference_y,
                        ETO_OPAQUE,
                        ptr::null(),
                        bytes.as_ptr(),
                        bytes.len() as u32,
                        ptr::null()
                    ),
                    0
                );
            }
            let expected = surface.pixels();
            unsafe {
                // Poison the entire reused cell, including the area outside
                // this glyph's extent, to catch stale strokes at every edge.
                assert_ne!(PatBlt(surface.hdc, 0, 0, 32, 32, Gdi::WHITENESS), 0);
                assert_ne!(
                    Gdi::ExtTextOutA(
                        surface.hdc,
                        0,
                        2,
                        ETO_OPAQUE,
                        ptr::null(),
                        bytes.as_ptr(),
                        bytes.len() as u32,
                        ptr::null()
                    ),
                    0
                );
            }
            assert!(
                surface.pixels() == expected,
                "native atlas glyph {character}"
            );
        }
    }
    hooks.uninstall().unwrap();
}
