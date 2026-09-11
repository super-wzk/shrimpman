use egui::{Color32, Context, CornerRadius, FontId, Stroke, TextStyle};

mod density;
mod icons;
pub(crate) mod paint;
mod tokens;
pub use density::Density;
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
    /// Choose the host's initial density. Use `Density::scope` for one local UI.
    pub fn density(mut self, density: Density) -> Self {
        density.apply_spacing(&mut self.style);
        self.tokens.density = density;
        self
    }

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
        let background = Color32::from_rgb(0x11, 0x12, 0x14);
        let panel = Color32::from_rgb(0x1A, 0x1C, 0x1F);
        let raised = Color32::from_rgb(0x22, 0x25, 0x2A);
        let input = Color32::from_rgb(0x14, 0x16, 0x19);
        let input_border = Color32::from_rgb(0x70, 0x76, 0x80);
        let text = Color32::from_rgb(0xF2, 0xF0, 0xEA);
        let muted = Color32::from_rgb(0x98, 0x9D, 0xA6);
        let border = Color32::from_rgb(0x32, 0x36, 0x3D);
        let tokens = Tokens::default();
        let gold = tokens.primary;
        let mut style = egui::Style {
            visuals: egui::Visuals::dark(),
            ..Default::default()
        };
        let visuals = &mut style.visuals;
        visuals.panel_fill = background;
        visuals.window_fill = panel;
        visuals.window_stroke = Stroke::new(1.0, border);
        visuals.window_corner_radius = CornerRadius::same(12);
        visuals.menu_corner_radius = CornerRadius::same(12);
        visuals.extreme_bg_color = input;
        visuals.text_edit_bg_color = Some(input);
        visuals.code_bg_color = input;
        visuals.faint_bg_color = raised;
        visuals.weak_text_color = Some(muted);
        visuals.hyperlink_color = gold;
        visuals.warn_fg_color = gold;
        visuals.error_fg_color = Color32::from_rgb(0xF0, 0xA3, 0x9B);
        visuals.selection.bg_fill = Color32::from_rgb(0x2D, 0x29, 0x21);
        visuals.selection.stroke = Stroke::new(1.5, gold);
        visuals.slider_trailing_fill = true;
        visuals.handle_shape = egui::style::HandleShape::Rect {
            aspect_ratio: 0.625,
        };
        for widget in [
            &mut visuals.widgets.noninteractive,
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
            &mut visuals.widgets.open,
        ] {
            widget.bg_fill = panel;
            widget.weak_bg_fill = panel;
            widget.bg_stroke = Stroke::new(1.0, input_border);
            widget.fg_stroke = Stroke::new(1.5, text);
            widget.corner_radius = CornerRadius::same(8);
            widget.expansion = 0.0;
        }
        visuals.widgets.hovered.bg_fill = raised;
        visuals.widgets.hovered.weak_bg_fill = raised;
        let active = Color32::from_rgb(0x2B, 0x2E, 0x34);
        visuals.widgets.active.bg_fill = active;
        visuals.widgets.active.weak_bg_fill = active;
        visuals.widgets.open.bg_fill = raised;
        visuals.widgets.open.weak_bg_fill = raised;
        visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, border);
        visuals.text_cursor.stroke.color = gold;
        Density::Standard.apply_spacing(&mut style);
        style.spacing.tooltip_width = 320.0;
        style.spacing.scroll.bar_width = 6.0;
        style.animation_time = 0.14;
        for (kind, size) in [
            (TextStyle::Body, 14.0),
            (TextStyle::Button, 14.0),
            (TextStyle::Heading, 24.0),
            (TextStyle::Small, 12.0),
        ] {
            style.text_styles.insert(kind, FontId::proportional(size));
        }
        Self { style, tokens }
    }
}
