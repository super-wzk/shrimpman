pub(super) const BG: egui::Color32 = egui::Color32::from_rgb(17, 19, 24);
pub(super) const CARD_BG: egui::Color32 = egui::Color32::from_rgb(26, 29, 38);
const CONTROL_BG: egui::Color32 = egui::Color32::from_rgb(20, 23, 30);
const HOVER_BG: egui::Color32 = egui::Color32::from_rgb(38, 43, 56);
pub(super) const BORDER: egui::Color32 = egui::Color32::from_rgb(48, 53, 68);
pub(super) const ACCENT: egui::Color32 = egui::Color32::from_rgb(232, 163, 61);
const ACCENT_HOVER: egui::Color32 = egui::Color32::from_rgb(244, 184, 93);
const ACCENT_ACTIVE: egui::Color32 = egui::Color32::from_rgb(199, 135, 45);
const ON_ACCENT: egui::Color32 = egui::Color32::from_rgb(31, 21, 6);
const TEXT: egui::Color32 = egui::Color32::from_rgb(232, 230, 227);
pub(super) const TEXT_WEAK: egui::Color32 = egui::Color32::from_rgb(156, 162, 178);
pub(super) const ERROR: egui::Color32 = egui::Color32::from_rgb(224, 85, 97);
pub(super) const ERROR_TEXT: egui::Color32 = egui::Color32::from_rgb(240, 138, 146);
const WARNING: egui::Color32 = egui::Color32::from_rgb(229, 192, 123);
const WARNING_TEXT: egui::Color32 = egui::Color32::from_rgb(240, 208, 148);
pub(super) const TEXT_EDIT_MARGIN: egui::Margin = egui::Margin::symmetric(10, 8);

pub(super) fn install(context: &egui::Context) {
    let mut fonts = egui::FontDefinitions::empty();
    let font_name = crate::font::FAMILY_NAME.to_owned();
    fonts.font_data.insert(
        font_name.clone(),
        std::sync::Arc::new(egui::FontData::from_static(crate::font::BYTES)),
    );
    for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(font_name.clone());
    }
    context.set_fonts(fonts);

    context.set_theme(egui::ThemePreference::Dark);
    let mut visuals = egui::Visuals::dark();
    let control_corner_radius = egui::CornerRadius::same(6);

    visuals.panel_fill = BG;
    visuals.window_fill = BG;
    visuals.window_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.extreme_bg_color = CONTROL_BG;
    visuals.faint_bg_color = CARD_BG;
    visuals.weak_text_color = Some(TEXT_WEAK);
    visuals.hyperlink_color = ACCENT;

    visuals.widgets.noninteractive.weak_bg_fill = CARD_BG;
    visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.inactive.weak_bg_fill = CONTROL_BG;
    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, BORDER);
    visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.hovered.weak_bg_fill = HOVER_BG;
    visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, HOVER_BG);
    visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, TEXT);

    visuals.widgets.active.weak_bg_fill = HOVER_BG;
    visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, TEXT);

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = control_corner_radius;
    }

    visuals.selection.bg_fill = ACCENT.gamma_multiply(0.35);
    visuals.selection.stroke = egui::Stroke::new(1.0, TEXT);

    context.all_styles_mut(|style| {
        style.visuals = visuals.clone();
        style.spacing.item_spacing = egui::vec2(10.0, 8.0);
        style.spacing.button_padding = egui::vec2(14.0, 7.0);
        style.spacing.interact_size.y = 34.0;
    });
}

/// Accent-colored call-to-action button with scoped hover/press/disabled states.
pub(super) fn primary_button(
    ui: &mut egui::Ui,
    text: &str,
    enabled: bool,
    width: f32,
) -> egui::Response {
    ui.scope(|ui| {
        let visuals = ui.visuals_mut();
        visuals.widgets.inactive.weak_bg_fill = ACCENT;
        visuals.widgets.inactive.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, ON_ACCENT);
        visuals.widgets.hovered.weak_bg_fill = ACCENT_HOVER;
        visuals.widgets.hovered.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, ON_ACCENT);
        visuals.widgets.active.weak_bg_fill = ACCENT_ACTIVE;
        visuals.widgets.active.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, ON_ACCENT);
        visuals.widgets.noninteractive.weak_bg_fill = ACCENT.gamma_multiply(0.30);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::NONE;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, ON_ACCENT);
        ui.add_enabled(
            enabled,
            egui::Button::new(
                egui::RichText::new(text)
                    .strong()
                    .color(ON_ACCENT)
                    .size(15.0),
            )
            .left_text(egui::Atom::grow())
            .right_text(egui::Atom::grow())
            .min_size(egui::vec2(width, 0.0)),
        )
    })
    .inner
}

pub(super) fn error_banner(ui: &mut egui::Ui, text: &str) {
    banner(ui, text, ERROR, ERROR_TEXT);
}

pub(super) fn warning_banner(ui: &mut egui::Ui, text: &str) {
    banner(ui, text, WARNING, WARNING_TEXT);
}

fn banner(ui: &mut egui::Ui, text: &str, color: egui::Color32, text_color: egui::Color32) {
    egui::Frame::new()
        .fill(color.gamma_multiply(0.12))
        .stroke(egui::Stroke::new(1.0, color.gamma_multiply(0.55)))
        .corner_radius(8)
        .inner_margin(egui::Margin::symmetric(12, 9))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(egui::RichText::new(text).color(text_color).size(13.0));
        });
}
