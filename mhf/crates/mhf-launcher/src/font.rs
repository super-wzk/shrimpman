//! The embedded font shared by the launcher, game font registration and overlay.

pub const FAMILY_NAME: &str = "JetBrains Maple Mono NF NL HT";
pub static BYTES: &[u8] =
    include_bytes!("../assets/fonts/JetBrainsMapleMono-NF-XX-NL-HT-Regular.ttf");

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
