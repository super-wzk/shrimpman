//! Process-local GDI font registration and native layout corrections.

mod gdi;

pub use gdi::{HookState, TextRenderer, corrected_y, install_game};

use crate::{FAMILY_NAME, resources::BYTES};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Graphics::Gdi::{AddFontMemResourceEx, RemoveFontMemResourceEx},
};

pub(crate) struct Registration(HANDLE);

pub(crate) fn register_for(name: &str) -> Result<Option<Registration>, String> {
    if !name.eq_ignore_ascii_case(FAMILY_NAME) {
        return Ok(None);
    }

    let byte_count = BYTES
        .len()
        .try_into()
        .map_err(|_| "embedded font is too large".to_owned())?;
    let mut font_count = 0;
    let handle = unsafe {
        AddFontMemResourceEx(
            BYTES.as_ptr().cast(),
            byte_count,
            std::ptr::null(),
            &raw mut font_count,
        )
    };
    if handle.is_null() {
        Err(format!("failed to register embedded {FAMILY_NAME} font"))
    } else {
        Ok(Some(Registration(handle)))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            RemoveFontMemResourceEx(self.0);
        }
    }
}
