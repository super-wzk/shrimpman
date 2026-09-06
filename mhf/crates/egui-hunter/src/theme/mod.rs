use egui::{Color32, Context, CornerRadius, FontId, Margin, Stroke, TextStyle, vec2};

mod icons;
pub(crate) mod paint;
mod tokens;
pub use icons::Icon;
pub use tokens::Tokens;

/// An installable egui style preset, plus the few hunter-specific paint values.
/// Widgets read the current [`egui::Ui`] style, never this preset.
#[derive(Clone, Debug)]
pub struct Theme {
    pub style: egui::Style,
    pub tokens: Tokens,
}

impl Theme {
    /// Install during host initialization or when changing themes.
    /// Font data and the host's input configuration remain owned by the host.
    pub fn apply(&self, context: &Context) {
        let mode = if self.style.visuals.dark_mode {
            egui::Theme::Dark
        } else {
            egui::Theme::Light
        };
        context.set_theme(mode);
        context.set_style_of(mode, self.style.clone());
        self.tokens.install(context);
    }
}

impl Default for Theme {
    fn default() -> Self {
        let background = Color32::from_rgb(0x17, 0x1D, 0x1B);
        let panel = Color32::from_rgb(0x24, 0x2C, 0x27);
        let raised = Color32::from_rgb(0x30, 0x39, 0x30);
        let text = Color32::from_rgb(0xE6, 0xDD, 0xC6);
        let muted = Color32::from_rgb(0xA3, 0xA5, 0x8D);
        let brass = Color32::from_rgb(0xB9, 0x9B, 0x5F);
        let border = Color32::from_rgb(0x46, 0x50, 0x44);
        let tokens = Tokens::default();
        let mut style = egui::Style {
            visuals: egui::Visuals::dark(),
            ..Default::default()
        };
        let visuals = &mut style.visuals;
        visuals.panel_fill = background;
        visuals.window_fill = panel;
        visuals.window_stroke = Stroke::new(1.0, border);
        visuals.window_corner_radius = CornerRadius::same(2);
        visuals.menu_corner_radius = CornerRadius::same(2);
        visuals.extreme_bg_color = background;
        visuals.faint_bg_color = raised;
        visuals.weak_text_color = Some(muted);
        visuals.hyperlink_color = brass;
        visuals.warn_fg_color = brass;
        visuals.error_fg_color = Color32::from_rgb(0xE0, 0xA0, 0x8B);
        visuals.selection.bg_fill = Color32::from_rgb(0x29, 0x3A, 0x2D);
        visuals.selection.stroke = Stroke::new(1.5, Color32::from_rgb(0xC2, 0xD3, 0xB0));
        visuals.slider_trailing_fill = true;
        visuals.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.6 };
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.bg_fill = panel;
            widget.weak_bg_fill = panel;
            widget.bg_stroke = Stroke::new(1.0, border);
            widget.fg_stroke = Stroke::new(1.5, text);
            widget.corner_radius = CornerRadius::same(2);
            widget.expansion = 0.0;
        }
        visuals.widgets.hovered.bg_fill = raised;
        visuals.widgets.hovered.weak_bg_fill = raised;
        let active = Color32::from_rgb(0x69, 0x5F, 0x45);
        visuals.widgets.active.bg_fill = active;
        visuals.widgets.active.weak_bg_fill = active;
        visuals.widgets.open.bg_fill = raised;
        visuals.widgets.open.weak_bg_fill = raised;
        style.spacing.item_spacing = vec2(8.0, 8.0);
        style.spacing.window_margin = Margin::same(16);
        style.spacing.button_padding = vec2(14.0, 8.0);
        style.spacing.icon_width = 22.0;
        style.spacing.icon_width_inner = 16.0;
        style.spacing.icon_spacing = 8.0;
        style.spacing.interact_size = vec2(36.0, 36.0);
        style.spacing.slider_width = 180.0;
        style.spacing.tooltip_width = 320.0;
        style.spacing.scroll.bar_width = 6.0;
        style.animation_time = 0.14;
        for (kind, size) in [
            (TextStyle::Body, 16.0),
            (TextStyle::Button, 16.0),
            (TextStyle::Heading, 22.0),
            (TextStyle::Small, 12.0),
        ] {
            style.text_styles.insert(kind, FontId::proportional(size));
        }
        Self { style, tokens }
    }
}

/// A local surface preset shared by native descendants and hunter painters.
pub(crate) fn parchment(ui: &mut egui::Ui, tokens: Tokens) -> Tokens {
    let visuals = ui.visuals_mut();
    visuals.dark_mode = false;
    visuals.override_text_color = Some(tokens.ink);
    visuals.weak_text_color = Some(tokens.ink.gamma_multiply(0.75));
    visuals.window_fill = tokens.parchment;
    visuals.panel_fill = tokens.parchment;
    visuals.window_stroke.color = tokens.ink.gamma_multiply(0.6);
    visuals.extreme_bg_color = tokens.parchment;
    visuals.text_edit_bg_color = Some(tokens.parchment);
    visuals.code_bg_color = tokens.parchment;
    visuals.faint_bg_color = tokens.ink.gamma_multiply(0.06);
    visuals.selection.bg_fill = tokens.parchment.lerp_to_gamma(tokens.ink, 0.12);
    visuals.selection.stroke.color = tokens.ink;
    visuals.text_cursor.stroke.color = tokens.ink;
    visuals.hyperlink_color = tokens.ink;
    visuals.warn_fg_color = Color32::from_rgb(0x62, 0x40, 0x0F);
    visuals.error_fg_color = Color32::from_rgb(0x7A, 0x32, 0x2C);
    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.fg_stroke.color = tokens.ink;
        widget.bg_stroke.color = visuals.window_stroke.color;
        widget.bg_fill = tokens.parchment;
        widget.weak_bg_fill = tokens.parchment;
    }
    let hover = tokens.parchment.lerp_to_gamma(tokens.ink, 0.06);
    let active = tokens.parchment.lerp_to_gamma(tokens.ink, 0.25);
    visuals.widgets.hovered.bg_fill = hover;
    visuals.widgets.hovered.weak_bg_fill = hover;
    visuals.widgets.active.bg_fill = active;
    visuals.widgets.active.weak_bg_fill = active;
    visuals.widgets.open.bg_fill = hover;
    visuals.widgets.open.weak_bg_fill = hover;
    Tokens {
        primary: tokens.parchment.lerp_to_gamma(tokens.ink, 0.18),
        success: tokens.ink.lerp_to_gamma(tokens.success, 0.25),
        ..tokens
    }
}
