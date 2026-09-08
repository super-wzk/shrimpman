use std::ops::Range;

use egui::{ImeEvent, Rect};
use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::UI::Input::Ime::{
    ATTR_TARGET_CONVERTED, ATTR_TARGET_NOTCONVERTED, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT,
    COMPOSITIONFORM, CPS_CANCEL, GCS_COMPATTR, GCS_COMPSTR, GCS_CURSORPOS, HIMC,
    IME_COMPOSITION_STRING, IME_CONVERSION_MODE, IME_SENTENCE_MODE, ImmAssociateContext,
    ImmCreateContext, ImmDestroyContext, ImmGetCompositionStringW, ImmGetContext,
    ImmGetConversionStatus, ImmGetOpenStatus, ImmNotifyIME, ImmReleaseContext,
    ImmSetCandidateWindow, ImmSetCompositionWindow, ImmSetConversionStatus, ImmSetOpenStatus,
    NI_COMPOSITIONSTR,
};

const MAX_COMPOSITION_BYTES: usize = 1024 * 1024;

/// A private context for the overlay; every native operation runs on the window thread.
/// Clones are borrowed handle snapshots, with no native cleanup on drop. Only the
/// owning instance may be passed to `destroy`, after all snapshots are finished.
#[derive(Clone)]
pub(super) struct NativeContext {
    handle: usize,
    original: usize,
}

impl NativeContext {
    /// Create an unassociated context, preserving the user's current input mode.
    pub(super) fn new(hwnd: HWND) -> Option<Self> {
        let handle = unsafe { ImmCreateContext() };
        if handle.is_invalid() {
            return None;
        }

        let host = unsafe { ImmGetContext(hwnd) };
        if !host.is_invalid() {
            let open = unsafe { ImmGetOpenStatus(host).as_bool() };
            let mut conversion = IME_CONVERSION_MODE::default();
            let mut sentence = IME_SENTENCE_MODE::default();
            let has_conversion = unsafe {
                ImmGetConversionStatus(host, Some(&mut conversion), Some(&mut sentence)).as_bool()
            };
            unsafe {
                let _ = ImmReleaseContext(hwnd, host);
                if has_conversion {
                    let _ = ImmSetConversionStatus(handle, conversion, sentence);
                }
                let _ = ImmSetOpenStatus(handle, open);
            }
        }

        Some(Self {
            handle: handle.0 as usize,
            original: host.0 as usize,
        })
    }

    /// The caller installs its transitional message route before entering IMM.
    pub(super) fn associate(&mut self, hwnd: HWND) -> bool {
        if !self.is_associated(hwnd) {
            unsafe { ImmAssociateContext(hwnd, self.himc()) };
        }
        self.is_associated(hwnd)
    }

    /// Disable text input for this window without changing the cached input mode.
    pub(super) fn suspend(&self, hwnd: HWND) {
        let current = current_context(hwnd);
        if !current.is_invalid() {
            unsafe {
                // The broker already cancelled its own composition before
                // releasing the recipient. Only an initial host context needs it.
                if current != self.himc() {
                    cancel_composition(current);
                }
                ImmAssociateContext(hwnd, HIMC::default());
            }
        }
    }

    pub(super) fn cancel(&self) {
        cancel_composition(self.himc());
    }

    /// `rect` is the text cursor in the host's client-pixel coordinate system.
    pub(super) fn set_cursor(&self, rect: Rect) {
        if !rect.is_finite() {
            return;
        }
        let left = rect.min.x.floor() as i32;
        let top = rect.min.y.floor() as i32;
        let area = RECT {
            left,
            top,
            right: (rect.max.x.ceil() as i32).max(left.saturating_add(1)),
            bottom: (rect.max.y.ceil() as i32).max(top.saturating_add(1)),
        };
        let composition = COMPOSITIONFORM {
            dwStyle: CFS_POINT,
            ptCurrentPos: POINT {
                x: left,
                y: area.bottom,
            },
            rcArea: area,
        };
        let candidate = CANDIDATEFORM {
            dwIndex: 0,
            dwStyle: CFS_EXCLUDE,
            ptCurrentPos: POINT { x: left, y: top },
            rcArea: area,
        };
        unsafe {
            let _ = ImmSetCompositionWindow(self.himc(), &composition);
            let _ = ImmSetCandidateWindow(self.himc(), &candidate);
        }
    }

    pub(super) fn read_string(&self, flag: IME_COMPOSITION_STRING) -> Option<String> {
        let bytes = self.read_data(flag)?;
        if bytes.len() % 2 != 0 {
            return None;
        }
        char::decode_utf16(
            bytes
                .as_chunks::<2>()
                .0
                .iter()
                .copied()
                .map(u16::from_ne_bytes),
        )
        .collect::<Result<String, _>>()
        .ok()
    }

    pub(super) fn preedit(&self) -> Option<ImeEvent> {
        let text = self.read_string(GCS_COMPSTR)?;
        let attributes = self.read_data(GCS_COMPATTR).unwrap_or_default();
        let cursor = unsafe { ImmGetCompositionStringW(self.himc(), GCS_CURSORPOS, None, 0) };
        let active_range_chars = active_range(&text, &attributes, cursor);
        Some(ImeEvent::Preedit {
            text,
            active_range_chars,
        })
    }

    /// Restore the association captured before the broker took input ownership.
    pub(super) fn restore(&mut self, hwnd: HWND) -> bool {
        let current = current_context(hwnd);
        let original = HIMC(self.original as *mut _);
        if current == original || (!current.is_invalid() && current != self.himc()) {
            return false;
        }
        unsafe {
            ImmAssociateContext(hwnd, original);
        }
        current_context(hwnd) == original
    }

    /// Release the cached context when the broker returns input to the host.
    pub(super) fn destroy(mut self, hwnd: HWND) {
        self.restore(hwnd);
        // Never destroy an input context that remains associated after failure.
        if !self.is_associated(hwnd) {
            unsafe {
                let _ = ImmDestroyContext(self.himc());
            }
        }
    }

    fn himc(&self) -> HIMC {
        HIMC(self.handle as *mut _)
    }

    pub(super) fn is_associated(&self, hwnd: HWND) -> bool {
        current_context(hwnd) == self.himc()
    }

    fn read_data(&self, flag: IME_COMPOSITION_STRING) -> Option<Vec<u8>> {
        let size = unsafe { ImmGetCompositionStringW(self.himc(), flag, None, 0) };
        let size = usize::try_from(size).ok()?;
        if size > MAX_COMPOSITION_BYTES {
            return None;
        }
        if size == 0 {
            return Some(Vec::new());
        }

        let mut data = vec![0; size];
        let copied = unsafe {
            ImmGetCompositionStringW(
                self.himc(),
                flag,
                Some(data.as_mut_ptr().cast()),
                size as u32,
            )
        };
        let copied = usize::try_from(copied).ok()?;
        if copied > size {
            return None;
        }
        data.truncate(copied);
        Some(data)
    }
}

fn current_context(hwnd: HWND) -> HIMC {
    let context = unsafe { ImmGetContext(hwnd) };
    if !context.is_invalid() {
        let _ = unsafe { ImmReleaseContext(hwnd, context) };
    }
    context
}

fn cancel_composition(context: HIMC) {
    // Wine's built-in IME closes fOpen on CPS_CANCEL. A focus change should
    // discard only the preedit, preserving the user's selected input mode.
    unsafe {
        let open = ImmGetOpenStatus(context).as_bool();
        let _ = ImmNotifyIME(context, NI_COMPOSITIONSTR, CPS_CANCEL, 0);
        if ImmGetOpenStatus(context).as_bool() != open {
            let _ = ImmSetOpenStatus(context, open);
        }
    }
}

/// IMM indexes UTF-16 code units; egui indexes Unicode scalar values.
fn active_range(text: &str, attributes: &[u8], cursor: i32) -> Option<Range<usize>> {
    if text.is_empty() {
        return None;
    }

    let mut units_before = 0;
    let mut targeted = None;
    for (index, character) in text.chars().enumerate() {
        let is_targeted = attributes.get(units_before).is_some_and(|attribute| {
            matches!(
                u32::from(*attribute),
                ATTR_TARGET_CONVERTED | ATTR_TARGET_NOTCONVERTED
            )
        });
        match (&mut targeted, is_targeted) {
            (None, true) => targeted = Some(index..index + 1),
            (Some(range), true) => range.end = index + 1,
            (Some(_), false) => break,
            (None, false) => {}
        }
        units_before += character.len_utf16();
    }
    if targeted.is_some() {
        return targeted;
    }

    let cursor = usize::try_from(cursor).ok()?;
    let mut units_before = 0;
    let mut chars_before = 0;
    for character in text.chars() {
        let end = units_before + character.len_utf16();
        if end > cursor {
            break;
        }
        units_before = end;
        chars_before += 1;
    }
    Some(chars_before..chars_before)
}

#[cfg(test)]
mod tests {
    use super::active_range;

    #[test]
    fn converts_target_attributes_from_utf16_to_character_indices() {
        assert_eq!(active_range("你😀好", &[0, 1, 1, 0], 0), Some(1..2));
        assert_eq!(active_range("你好世界", &[0, 3, 3, 0], 0), Some(1..3));
    }

    #[test]
    fn converts_cursor_from_utf16_without_splitting_surrogate_pairs() {
        assert_eq!(active_range("你😀好", &[], 3), Some(2..2));
        assert_eq!(active_range("你😀好", &[], 2), Some(1..1));
        assert_eq!(active_range("你😀好", &[], 99), Some(3..3));
        assert_eq!(active_range("你😀好", &[], -1), None);
        assert_eq!(active_range("", &[], 0), None);
    }
}
