use windows_sys::Win32::{
    Foundation::HANDLE,
    Graphics::Gdi::{AddFontMemResourceEx, RemoveFontMemResourceEx},
};

pub(super) const FAMILY_NAME: &str = "JetBrains Maple Mono NF NL HT";
pub(super) static BYTES: &[u8] =
    include_bytes!("../../../assets/fonts/JetBrainsMapleMono-NF-XX-NL-HT-Regular.ttf");

pub(super) struct Registration(HANDLE);

pub(super) fn register() -> Result<Registration, String> {
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
        Ok(Registration(handle))
    }
}

impl Drop for Registration {
    fn drop(&mut self) {
        unsafe {
            RemoveFontMemResourceEx(self.0);
        }
    }
}
