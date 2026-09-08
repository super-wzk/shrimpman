//! Font resources, registration and native layout corrections.

mod gdi;

pub(crate) use gdi::install_game;
#[cfg(feature = "unicode")]
pub(crate) use gdi::{TextRenderer, corrected_y};

use windows_sys::Win32::{
    Foundation::HANDLE,
    Graphics::Gdi::{AddFontMemResourceEx, RemoveFontMemResourceEx},
};

pub(crate) const FAMILY_NAME: &str = "JetBrains Maple Mono NF NL HT";
static BYTES: &[u8] =
    include_bytes!("../../assets/fonts/JetBrainsMapleMono-NF-XX-NL-HT-Regular.ttf");

pub fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    fonts.font_data.insert(
        FAMILY_NAME.to_owned(),
        egui::FontData::from_static(BYTES).into(),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(FAMILY_NAME.to_owned());
    }
    context.set_fonts(fonts);
}

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
