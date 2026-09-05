use egui::{Color32, Context, CornerRadius, FontId, Stroke, TextStyle, vec2};

/// Semantic colors shared by custom widgets and egui's standard controls.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    pub background: Color32,
    pub panel: Color32,
    pub raised: Color32,
    pub text: Color32,
    pub muted: Color32,
    pub brass: Color32,
    pub border: Color32,
    pub moss: Color32,
    pub danger: Color32,
    pub parchment: Color32,
    pub ink: Color32,
}

impl Default for Palette {
    fn default() -> Self {
        Self {
            background: Color32::from_rgb(0x17, 0x1D, 0x1B),
            panel: Color32::from_rgb(0x24, 0x2C, 0x27),
            raised: Color32::from_rgb(0x30, 0x39, 0x30),
            text: Color32::from_rgb(0xE6, 0xDD, 0xC6),
            muted: Color32::from_rgb(0xA3, 0xA5, 0x8D),
            brass: Color32::from_rgb(0xB9, 0x9B, 0x5F),
            border: Color32::from_rgb(0x65, 0x5D, 0x43),
            moss: Color32::from_rgb(0x71, 0x83, 0x5F),
            danger: Color32::from_rgb(0xA5, 0x52, 0x45),
            parchment: Color32::from_rgb(0xCC, 0xB7, 0x8F),
            ink: Color32::from_rgb(0x36, 0x30, 0x23),
        }
    }
}

/// Spacing and sizing in logical points. Scaling is controlled by the egui host.
#[derive(Clone, Copy, Debug)]
pub struct Metrics {
    pub gap: f32,
    pub padding: i8,
    pub control_height: f32,
    pub body_size: f32,
    pub heading_size: f32,
    pub cut: f32,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            gap: 8.0,
            padding: 16,
            control_height: 36.0,
            body_size: 16.0,
            heading_size: 22.0,
            cut: 5.0,
        }
    }
}

/// A value-owned design system. No renderer, font loading or global state.
#[derive(Clone, Copy, Debug, Default)]
pub struct Theme {
    pub palette: Palette,
    pub metrics: Metrics,
}

impl Theme {
    /// Apply once during host initialization (and again when changing the theme).
    /// Preserves the host's installed fonts and input configuration.
    pub fn apply(&self, context: &Context) {
        let p = self.palette;
        let m = self.metrics;
        context.set_theme(egui::ThemePreference::Dark);
        context.all_styles_mut(|style| {
            let mut v = egui::Visuals::dark();
            v.panel_fill = p.background;
            v.window_fill = p.panel;
            v.window_stroke = Stroke::new(1.0, p.border);
            v.window_corner_radius = CornerRadius::same(2);
            v.menu_corner_radius = CornerRadius::same(2);
            v.extreme_bg_color = p.background;
            v.faint_bg_color = p.raised;
            v.weak_text_color = Some(p.muted);
            v.hyperlink_color = p.brass;
            v.warn_fg_color = p.brass;
            v.error_fg_color = p.danger;
            v.selection.bg_fill = p.moss.gamma_multiply(0.5);
            v.selection.stroke = Stroke::new(1.5, p.text);
            for w in [
                &mut v.widgets.noninteractive,
                &mut v.widgets.inactive,
                &mut v.widgets.hovered,
                &mut v.widgets.active,
                &mut v.widgets.open,
            ] {
                w.bg_fill = p.panel;
                w.weak_bg_fill = p.panel;
                w.bg_stroke = Stroke::new(1.0, p.border);
                w.fg_stroke = Stroke::new(1.5, p.text);
                w.corner_radius = CornerRadius::same(2);
                w.expansion = 0.0;
            }
            v.widgets.hovered.bg_fill = p.raised;
            v.widgets.hovered.weak_bg_fill = p.raised;
            v.widgets.hovered.bg_stroke.color = p.brass;
            v.widgets.active.bg_fill = p.background;
            v.widgets.active.bg_stroke = Stroke::new(1.5, p.brass);
            v.widgets.open.bg_stroke.color = p.brass;
            style.visuals = v;
            style.spacing.item_spacing = vec2(m.gap, m.gap);
            style.spacing.button_padding = vec2(14.0, 8.0);
            style.spacing.interact_size = vec2(36.0, m.control_height);
            style.spacing.slider_width = 180.0;
            style.spacing.scroll.bar_width = 6.0;
            style.animation_time = 0.14;
            for (kind, size) in [
                (TextStyle::Body, m.body_size),
                (TextStyle::Button, m.body_size),
                (TextStyle::Heading, m.heading_size),
                (TextStyle::Small, 12.0),
            ] {
                style.text_styles.insert(kind, FontId::proportional(size));
            }
        });
    }
}
