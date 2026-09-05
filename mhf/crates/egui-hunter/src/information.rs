use egui::{
    Align, Color32, FontId, InnerResponse, Layout, Rect, Response, RichText, Sense, Shape, Stroke,
    Ui, Vec2, Widget, WidgetInfo, WidgetType, pos2, vec2,
};

use crate::{Icon, Theme, paint};

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

/// A native delayed tooltip with a styled panel and freely composed contents.
/// Keyboard focus also reveals it by default. No popup/menu state is opened.
pub struct RichTooltip<'a> {
    theme: &'a Theme,
    anchor: &'a Response,
    title: &'a str,
    width: f32,
    on_focus: bool,
}

impl Theme {
    pub fn tooltip<'a>(&'a self, anchor: &'a Response, title: &'a str) -> RichTooltip<'a> {
        RichTooltip {
            theme: self,
            anchor,
            title,
            width: 320.0,
            on_focus: true,
        }
    }

    /// Full-width, wrapping label/value rows. Below 280 points each label stacks
    /// above its value. Inherits the parent surface's text colors.
    pub fn properties(&self, ui: &mut Ui, properties: &[Property<'_>]) -> Response {
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
                        columns[0]
                            .add(egui::Label::new(RichText::new(property.label).weak()).wrap());
                        columns[1].with_layout(Layout::top_down(Align::Max), |ui| {
                            ui.add(egui::Label::new(value).wrap());
                        });
                    });
                }
            }
        })
        .response
    }

    /// `fraction` is normalized to 0..=1. Non-finite values display as empty.
    pub fn meter(&self, fraction: f32) -> Meter<'_> {
        Meter {
            theme: self,
            fraction,
            label: "",
            color: self.palette.moss,
            width: None,
        }
    }

    /// A display-only keycap. Input-device mapping remains the host's responsibility.
    pub fn key_hint(&self, ui: &mut Ui, key: &str, label: &str) -> Response {
        let p = self.palette;
        ui.horizontal(|ui| {
            let galley =
                ui.painter()
                    .layout_no_wrap(key.to_owned(), FontId::proportional(12.0), p.text);
            let (rect, _) = ui.allocate_exact_size(
                vec2((galley.size().x + 16.0).max(24.0), 24.0),
                Sense::hover(),
            );
            ui.painter().add(paint::chamfer(
                rect.shrink(0.5),
                3.0,
                p.background,
                Stroke::new(1.0, p.border),
            ));
            ui.painter()
                .galley(rect.center() - galley.size() * 0.5, galley, p.text);
            ui.label(
                egui::RichText::new(label)
                    .size(13.0)
                    .color(ui.visuals().weak_text_color()),
            );
        })
        .response
    }

    /// An inline notice. Use [`crate::Notifications`] for queued, timed messages.
    pub fn notice(&self, ui: &mut Ui, kind: NoticeKind, text: &str) -> Response {
        let (icon, accent) = match kind {
            NoticeKind::Success => (Icon::Check, self.palette.moss),
            NoticeKind::Warning => (Icon::Warning, self.palette.brass),
            NoticeKind::Danger => (Icon::Warning, self.palette.danger),
        };
        let background = ui.painter().add(Shape::Noop);
        let response = egui::Frame::new()
            .inner_margin(12)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(22.0), Sense::hover());
                    icon.paint(ui.painter(), rect, accent);
                    ui.add(
                        egui::Label::new(egui::RichText::new(text).color(self.palette.text)).wrap(),
                    );
                });
            })
            .response;
        ui.painter().set(
            background,
            paint::chamfer(
                response.rect.shrink(0.5),
                self.metrics.cut,
                self.palette.background,
                Stroke::new(1.0, accent),
            ),
        );
        response
    }
}

impl RichTooltip<'_> {
    pub fn width(mut self, width: f32) -> Self {
        self.width = width.max(80.0);
        self
    }
    pub fn on_focus(mut self, on_focus: bool) -> Self {
        self.on_focus = on_focus;
        self
    }

    pub fn show<R>(self, content: impl FnOnce(&mut Ui) -> R) -> Option<InnerResponse<R>> {
        let ctx = &self.anchor.ctx;
        if !self.anchor.interact_rect.is_positive()
            || !ctx.memory(|m| m.allows_interaction(self.anchor.layer_id))
            || (egui::Popup::is_any_open(ctx)
                && self.anchor.layer_id.order != egui::Order::Foreground)
        {
            return None;
        }
        let mut native = if self.on_focus && self.anchor.has_focus() {
            egui::Tooltip::for_widget(self.anchor)
        } else if self.anchor.enabled() {
            egui::Tooltip::for_enabled(self.anchor)
        } else {
            egui::Tooltip::for_disabled(self.anchor)
        };
        let width = self.width.min((ctx.content_rect().width() - 16.0).max(1.0));
        native.popup = native.popup.frame(egui::Frame::NONE);
        native.width(width).show(|ui| {
            ui.set_width(width);
            self.theme.panel(self.title).show(ui, content).inner
        })
    }
}

pub struct Meter<'a> {
    theme: &'a Theme,
    fraction: f32,
    label: &'a str,
    color: Color32,
    width: Option<f32>,
}

impl<'a> Meter<'a> {
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = label;
        self
    }
    pub fn color(mut self, color: Color32) -> Self {
        self.color = color;
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
        let (rect, response) = ui.allocate_exact_size(vec2(width, 30.0), Sense::hover());
        let c = self.theme.palette;
        response.widget_info(|| {
            WidgetInfo::labeled(
                WidgetType::ProgressIndicator,
                ui.is_enabled(),
                format!("{} {:.0}%", self.label, fraction * 100.0),
            )
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let label = painter.layout_no_wrap(
                self.label.to_owned(),
                FontId::proportional(13.0),
                ui.visuals().text_color(),
            );
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
                    c.text,
                );
            let bar = Rect::from_min_max(
                pos2(rect.left() + label_width, rect.center().y - 5.0),
                pos2(rect.right(), rect.center().y + 5.0),
            );
            painter.add(paint::chamfer(
                bar,
                4.0,
                c.background,
                Stroke::new(1.0, c.border),
            ));
            let fill = Rect::from_min_size(
                bar.min + vec2(2.0, 2.0),
                vec2((bar.width() - 4.0) * fraction, bar.height() - 4.0),
            );
            if fill.width() > 0.0 {
                painter.add(paint::chamfer(fill, 2.0, self.color, Stroke::NONE));
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
