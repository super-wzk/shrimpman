use crate::FAMILY_NAME;

pub(crate) static BYTES: &[u8] =
    include_bytes!("../assets/JetBrainsMapleMono-NF-XX-NL-HT-Regular.ttf");

/// Install the embedded family in a native egui context.
pub fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        FAMILY_NAME.to_owned(),
        egui::FontData::from_static(BYTES).into(),
    );
    // Keep proportional Latin UI text and use the embedded family for CJK and
    // game glyphs. Native game font hooks continue to use the same embedded font.
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .push(FAMILY_NAME.to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, FAMILY_NAME.to_owned());
    context.set_fonts(fonts);
}
