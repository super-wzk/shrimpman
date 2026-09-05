use std::ops::RangeInclusive;

use egui::{
    Align2, Color32, FontId, Id, Rect, Response, Sense, Stroke, TextureId, Ui, Vec2, Widget,
    WidgetInfo, WidgetType, pos2, vec2,
};

use crate::{Icon, Theme, paint};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ButtonKind {
    #[default]
    Default,
    Primary,
    Danger,
}

/// A keyboard-accessible button, tab or selectable row. Use `Ui::push_id` for
/// stable identity in filtered/reordered collections, just as with egui widgets.
pub struct Button<'a> {
    id: Option<Id>,
    theme: &'a Theme,
    label: &'a str,
    kind: ButtonKind,
    selected: Option<bool>,
    icon: Option<Icon>,
    min_size: Vec2,
    full_width: bool,
}

impl Theme {
    pub fn button<'a>(&'a self, label: &'a str) -> Button<'a> {
        Button {
            id: None,
            theme: self,
            label,
            kind: ButtonKind::Default,
            selected: None,
            icon: None,
            min_size: vec2(88.0, self.metrics.control_height),
            full_width: false,
        }
    }

    pub fn checkbox<'a>(&'a self, checked: &'a mut bool, label: &'a str) -> Checkbox<'a> {
        Checkbox {
            theme: self,
            checked,
            label,
        }
    }

    pub fn toggle<'a>(&'a self, checked: &'a mut bool, label: &'a str) -> Toggle<'a> {
        Toggle {
            theme: self,
            checked,
            label,
        }
    }

    /// The native slider retains egui's numeric editing, keyboard and drag behavior.
    pub fn slider<'a>(&self, value: &'a mut f32, range: RangeInclusive<f32>) -> egui::Slider<'a> {
        egui::Slider::new(value, range)
            .trailing_fill(true)
            .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.6 })
    }

    pub fn item_slot<'a>(&'a self, label: &'a str) -> ItemSlot<'a> {
        ItemSlot {
            theme: self,
            label,
            icon: None,
            image: None,
            quantity: None,
            selected: false,
            size: 72.0,
            tint: self.palette.text,
            hover_text: true,
        }
    }
}

impl Button<'_> {
    /// Explicit identity for initial focus and controls that move between layouts.
    pub fn id(mut self, id: Id) -> Self {
        self.id = Some(id);
        self
    }
    pub fn kind(mut self, kind: ButtonKind) -> Self {
        self.kind = kind;
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = Some(selected);
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn min_size(mut self, size: Vec2) -> Self {
        self.min_size = size;
        self
    }
    pub fn full_width(mut self) -> Self {
        self.full_width = true;
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let theme = self.theme;
        let c = theme.palette;
        let icon = self
            .icon
            .or((self.kind == ButtonKind::Danger).then_some(Icon::Warning));
        let icon_space = if icon.is_some() { 28.0 } else { 0.0 };
        let marker_space = if self.selected.is_some() { 20.0 } else { 0.0 };
        let galley = ui.painter().layout_no_wrap(
            self.label.to_owned(),
            FontId::proportional(theme.metrics.body_size),
            c.text,
        );
        let mut size =
            (galley.size() + vec2(32.0 + icon_space + marker_space, 16.0)).max(self.min_size);
        if self.full_width {
            size.x = ui.available_width();
        }
        size.x = size.x.min(ui.available_width().max(32.0));
        let (rect, response) = if let Some(id) = self.id {
            let (_, rect) = ui.allocate_space(size);
            (rect, ui.interact(rect, id, Sense::click()))
        } else {
            ui.allocate_at_least(size, Sense::click())
        };
        response.widget_info(|| match self.selected {
            Some(selected) => WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                selected,
                self.label,
            ),
            None => WidgetInfo::labeled(WidgetType::Button, ui.is_enabled(), self.label),
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let accent = match self.kind {
                ButtonKind::Default => None,
                ButtonKind::Primary => Some(c.brass),
                ButtonKind::Danger => Some(c.danger),
            };
            paint::control(
                &painter,
                rect,
                &response,
                theme,
                self.selected == Some(true),
                accent,
            );
            let color = paint::text_color(c, &response);
            let content_width = galley.size().x + icon_space;
            let x = if self.full_width {
                rect.left() + 14.0
            } else {
                rect.center().x - content_width * 0.5 - marker_space * 0.5
            };
            let y = rect.center().y + f32::from(response.is_pointer_button_down_on());
            if let Some(icon) = icon {
                icon.paint(
                    &painter,
                    Rect::from_center_size(pos2(x + 10.0, y), Vec2::splat(20.0)),
                    if self.kind == ButtonKind::Danger {
                        c.danger
                    } else {
                        color
                    },
                );
            }
            let text_rect = Rect::from_min_max(
                pos2(rect.left() + 10.0 + icon_space, rect.top()),
                pos2(rect.right() - 10.0 - marker_space, rect.bottom()),
            );
            painter.with_clip_rect(text_rect).galley(
                pos2(x + icon_space, y - galley.size().y * 0.5),
                galley,
                color,
            );
            if self.selected == Some(true) {
                paint::diamond(&painter, pos2(rect.right() - 14.0, y), 4.0, color);
            }
        }
        response
    }
}

pub struct Checkbox<'a> {
    theme: &'a Theme,
    checked: &'a mut bool,
    label: &'a str,
}
pub struct Toggle<'a> {
    theme: &'a Theme,
    checked: &'a mut bool,
    label: &'a str,
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        choice(ui, self.theme, self.checked, self.label, false)
    }
}
impl Widget for Toggle<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        choice(ui, self.theme, self.checked, self.label, true)
    }
}

fn choice(ui: &mut Ui, theme: &Theme, checked: &mut bool, label: &str, switch: bool) -> Response {
    let c = theme.palette;
    let galley = ui.painter().layout_no_wrap(
        label.to_owned(),
        FontId::proportional(theme.metrics.body_size),
        ui.visuals().text_color(),
    );
    let icon_width = if switch { 42.0 } else { 22.0 };
    let desired = vec2(
        icon_width + theme.metrics.gap + galley.size().x,
        theme.metrics.control_height,
    );
    let (rect, mut response) = ui.allocate_at_least(
        vec2(
            desired.x.min(ui.available_width().max(icon_width)),
            desired.y,
        ),
        Sense::click(),
    );
    if response.clicked() {
        *checked = !*checked;
        response.mark_changed();
    }
    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::Checkbox, ui.is_enabled(), *checked, label)
    });
    if ui.is_rect_visible(rect) {
        let painter = ui.painter_at(rect);
        let icon_rect = Rect::from_center_size(
            pos2(rect.left() + icon_width * 0.5, rect.center().y),
            vec2(icon_width, 22.0),
        );
        paint::control(&painter, icon_rect, &response, theme, *checked, None);
        let color = paint::text_color(c, &response);
        if switch {
            let amount =
                ui.ctx()
                    .animate_bool_with_time(response.id.with("toggle"), *checked, 0.14);
            let x = egui::lerp(
                (icon_rect.left() + 10.0)..=(icon_rect.right() - 10.0),
                amount,
            );
            painter.add(paint::chamfer(
                Rect::from_center_size(pos2(x, icon_rect.center().y), vec2(14.0, 16.0)),
                3.0,
                color,
                Stroke::NONE,
            ));
        } else if *checked {
            Icon::Check.paint(&painter, icon_rect.shrink(3.0), color);
        }
        let text_rect = Rect::from_min_max(
            pos2(icon_rect.right() + theme.metrics.gap, rect.top()),
            rect.max,
        );
        painter.with_clip_rect(text_rect).galley(
            pos2(text_rect.left(), rect.center().y - galley.size().y * 0.5),
            galley,
            color,
        );
    }
    response
}

pub struct ItemSlot<'a> {
    theme: &'a Theme,
    label: &'a str,
    icon: Option<Icon>,
    image: Option<TextureId>,
    quantity: Option<u32>,
    selected: bool,
    size: f32,
    tint: Color32,
    hover_text: bool,
}

impl ItemSlot<'_> {
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    pub fn image(mut self, texture: TextureId) -> Self {
        self.image = Some(texture);
        self
    }
    pub fn quantity(mut self, quantity: u32) -> Self {
        self.quantity = Some(quantity);
        self
    }
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }
    pub fn size(mut self, size: f32) -> Self {
        self.size = size.max(32.0);
        self
    }
    pub fn tint(mut self, color: Color32) -> Self {
        self.tint = color;
        self
    }
    /// Disable the plain label when attaching a rich tooltip to the response.
    pub fn hover_text(mut self, show: bool) -> Self {
        self.hover_text = show;
        self
    }
}

impl Widget for ItemSlot<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::click());
        response.widget_info(|| {
            WidgetInfo::selected(
                WidgetType::SelectableLabel,
                ui.is_enabled(),
                self.selected,
                self.label,
            )
        });
        if ui.is_rect_visible(rect) {
            let painter = ui.painter_at(rect);
            let c = self.theme.palette;
            paint::control(&painter, rect, &response, self.theme, self.selected, None);
            let art = rect.shrink(self.size * 0.22);
            if let Some(texture) = self.image {
                painter.image(
                    texture,
                    art,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            } else if let Some(icon) = self.icon {
                icon.paint(&painter, art, self.tint);
            } else {
                paint::diamond(&painter, rect.center(), 5.0, c.border.gamma_multiply(0.6));
            }
            if self.selected {
                paint::diamond(&painter, rect.right_top() + vec2(-10.0, 10.0), 3.5, c.text);
            }
            if let Some(quantity) = self.quantity {
                painter.text(
                    rect.right_bottom() + vec2(-7.0, -5.0),
                    Align2::RIGHT_BOTTOM,
                    quantity.to_string(),
                    FontId::proportional(13.0),
                    c.text,
                );
            }
        }
        if self.hover_text {
            response.on_hover_text(self.label)
        } else {
            response
        }
    }
}
