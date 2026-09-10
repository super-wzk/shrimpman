use egui::{
    Align, Color32, CornerRadius, FontSelection, Layout, Rect, Response, RichText, Sense, Stroke,
    TextStyle, Ui, Vec2, Widget, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::{Icon, Tokens, theme::paint};

/// Display-only label/value data. Hosts format units and comparison text; use
/// `color` for an accent alongside explicit text such as "+12" or "未满足".
#[derive(Clone, Copy, Debug)]
pub struct Property<'a> {
    pub label: &'a str,
    pub value: &'a str,
    pub color: Option<Color32>,
}

impl<'a> Property<'a> {
    pub fn new(label: &'a str, value: &'a str) -> Self {
        Self {
            label,
            value,
            color: None,
        }
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }
}

/// Full-width, wrapping label/value rows. Below 280 points each label stacks
/// above its value. Inherits the parent surface's text colors.
pub fn properties(ui: &mut Ui, properties: &[Property<'_>]) -> Response {
    ui.vertical(|ui| {
        let stacked = ui.available_width() < 280.0;
        for property in properties {
            let value = RichText::new(property.value)
                .color(property.color.unwrap_or(ui.visuals().text_color()));
            if stacked {
                ui.label(RichText::new(property.label).small().weak());
                ui.add(egui::Label::new(value).wrap());
            } else {
                ui.columns(2, |columns| {
                    columns[0].add(egui::Label::new(RichText::new(property.label).weak()).wrap());
                    columns[1].with_layout(Layout::top_down(Align::Max), |ui| {
                        ui.add(egui::Label::new(value).wrap());
                    });
                });
            }
        }
    })
    .response
}

/// A display-only keycap. Input-device mapping remains the host's responsibility.
pub fn key_hint(ui: &mut Ui, key: &str, label: &str) -> Response {
    let text_color = ui.visuals().text_color();
    let stroke = ui.style().noninteractive().bg_stroke;
    ui.horizontal(|ui| {
        let galley = ui.painter().layout_no_wrap(
            key.to_owned(),
            FontSelection::Default.resolve_with_fallback(ui.style(), TextStyle::Small.into()),
            text_color,
        );
        let size = (galley.size() + ui.spacing().button_padding)
            .max(Vec2::splat(ui.spacing().interact_size.y * (2.0 / 3.0)));
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        ui.painter().add(paint::rounded(
            rect.shrink(0.5),
            CornerRadius::same(4),
            ui.visuals().extreme_bg_color,
            stroke,
        ));
        ui.painter()
            .galley(rect.center() - galley.size() * 0.5, galley, text_color);
        ui.label(
            egui::RichText::new(label)
                .small()
                .color(ui.visuals().weak_text_color()),
        );
    })
    .response
}

/// An inline notice. Use [`crate::Notifications`] for queued, timed messages.
pub fn notice(ui: &mut Ui, kind: NoticeKind, text: &str) -> Response {
    let tokens = Tokens::get(ui);
    let (icon, accent, fill) = match kind {
        NoticeKind::Success => (Icon::Check, tokens.success, tokens.success_fill),
        NoticeKind::Warning => (
            Icon::Warning,
            ui.visuals().warn_fg_color,
            ui.visuals().selection.bg_fill,
        ),
        NoticeKind::Danger => (
            Icon::Warning,
            ui.visuals().error_fg_color,
            tokens.danger_fill,
        ),
    };
    egui::Frame::new()
        .fill(fill)
        .corner_radius(ui.visuals().widgets.inactive.corner_radius)
        .inner_margin(12)
        .show(ui, |ui| {
            ui.horizontal_top(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(Vec2::splat(ui.spacing().icon_width), Sense::hover());
                icon.paint(ui.painter(), rect, accent);
                ui.add(egui::Label::new(RichText::new(text).color(accent)).wrap());
            });
        })
        .response
}

pub struct Meter<'a> {
    fraction: f32,
    label: &'a str,
    color: Option<Color32>,
    width: Option<f32>,
}

impl<'a> Meter<'a> {
    /// `fraction` is normalized to 0..=1. Non-finite values display as empty.
    pub fn new(fraction: f32) -> Self {
        Self {
            fraction,
            label: "",
            color: None,
            width: None,
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.color = Some(color);
        self
    }
    pub fn width(mut self, width: f32) -> Self {
        self.width = Some(width);
        self
    }
}

impl Widget for Meter<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let fraction = if self.fraction.is_finite() {
            self.fraction.clamp(0.0, 1.0)
        } else {
            0.0
        };
        let width = self.width.unwrap_or(ui.available_width()).max(64.0);
        let font =
            FontSelection::Default.resolve_with_fallback(ui.style(), TextStyle::Small.into());
        let height = ui
            .fonts_mut(|fonts| fonts.row_height(&font))
            .max(ui.spacing().interact_size.y);
        let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
        let color = self.color.unwrap_or(Tokens::get(ui).primary);
        response.widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::ProgressIndicator,
                ui.is_enabled(),
                format!("{} {:.0}%", self.label, fraction * 100.0),
            )
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let label =
                painter.layout_no_wrap(self.label.to_owned(), font, ui.visuals().text_color());
            let label_width = if self.label.is_empty() {
                0.0
            } else {
                (label.size().x + 12.0).min(width * 0.4)
            };
            painter
                .with_clip_rect(Rect::from_min_max(
                    rect.min,
                    pos2(rect.left() + label_width, rect.bottom()),
                ))
                .galley(
                    rect.left_center() - vec2(0.0, label.size().y * 0.5),
                    label,
                    ui.visuals().text_color(),
                );
            let bar = Rect::from_min_max(
                pos2(rect.left() + label_width, rect.center().y - 5.0),
                pos2(rect.right(), rect.center().y + 5.0),
            );
            painter.add(paint::rounded(
                bar,
                CornerRadius::same(5),
                ui.visuals().extreme_bg_color,
                ui.visuals().widgets.noninteractive.bg_stroke,
            ));
            let fill = Rect::from_min_size(
                bar.min + vec2(2.0, 2.0),
                vec2((bar.width() - 4.0) * fraction, bar.height() - 4.0),
            );
            if fill.width() > 0.0 {
                painter.add(paint::rounded(
                    fill,
                    CornerRadius::same(3),
                    color,
                    Stroke::NONE,
                ));
            }
        }
        response
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoticeKind {
    Success,
    Warning,
    Danger,
}
