use egui::{
    Id, Rect, Response, Sense, Stroke, TextStyle, TextWrapMode, Ui, Vec2, Widget, WidgetInfo,
    WidgetText, WidgetType, pos2, vec2,
};

use crate::Icon;
use crate::primitives::focus::{focus_on_click, scroll_on_focus};
use crate::theme::paint;

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
    label: &'a str,
    kind: ButtonKind,
    selected: Option<bool>,
    icon: Option<Icon>,
    min_size: Vec2,
    full_width: bool,
    sense: Sense,
}

impl<'a> Button<'a> {
    pub fn new(label: &'a str) -> Self {
        Self {
            id: None,
            label,
            kind: ButtonKind::Default,
            selected: None,
            icon: None,
            min_size: Vec2::ZERO,
            full_width: false,
            sense: Sense::click(),
        }
    }

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
    /// Use `Sense::CLICK` for mouse-only parts of a composite widget.
    pub fn sense(mut self, sense: Sense) -> Self {
        self.sense = sense;
        self
    }
}

impl Widget for Button<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let icon = self
            .icon
            .or((self.kind == ButtonKind::Danger).then_some(Icon::Warning));
        let padding = ui.spacing().button_padding;
        let icon_width = ui.spacing().icon_width;
        let icon_space = if icon.is_some() {
            icon_width + ui.spacing().icon_spacing
        } else {
            0.0
        };
        let marker_space = if self.selected.is_some() {
            icon_width
        } else {
            0.0
        };
        let galley = WidgetText::from(self.label).into_galley(
            ui,
            Some(TextWrapMode::Extend),
            f32::INFINITY,
            TextStyle::Button,
        );
        let content_size = vec2(
            galley.size().x + icon_space + marker_space,
            galley
                .size()
                .y
                .max(if icon.is_some() { icon_width } else { 0.0 }),
        );
        let mut size = (content_size + padding * 2.0)
            .max(self.min_size)
            .max(ui.spacing().interact_size);
        if self.full_width {
            size.x = ui.available_width();
        }
        size.x = size
            .x
            .min(ui.available_width().max(ui.spacing().interact_size.x));
        let (rect, response) = if let Some(id) = self.id {
            let (_, rect) = ui.allocate_space(size);
            (rect, ui.interact(rect, id, self.sense))
        } else {
            ui.allocate_at_least(size, self.sense)
        };
        focus_on_click(&response);
        scroll_on_focus(&response);
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
            let selected = self.selected == Some(true);
            let painter = ui.painter_at(rect);
            let visuals = paint::button_visuals(ui, &response, selected, self.kind);
            paint::control(ui, rect, &visuals);
            let color = ui
                .visuals()
                .override_text_color
                .unwrap_or_else(|| visuals.text_color());
            let content_width = galley.size().x + icon_space;
            let text_rect = Rect::from_min_max(
                pos2(rect.left() + padding.x, rect.top()),
                pos2(rect.right() - padding.x - marker_space, rect.bottom()),
            );
            let x = if self.full_width || content_width > text_rect.width() {
                text_rect.left()
            } else {
                text_rect.center().x - content_width * 0.5
            };
            let y = rect.center().y + f32::from(response.is_pointer_button_down_on());
            if let Some(icon) = icon {
                icon.paint(
                    &painter,
                    Rect::from_center_size(pos2(x + icon_width * 0.5, y), Vec2::splat(icon_width)),
                    if self.kind == ButtonKind::Danger {
                        ui.visuals().error_fg_color
                    } else {
                        color
                    },
                );
            }
            painter.with_clip_rect(text_rect).galley(
                pos2(x + icon_space, y - galley.size().y * 0.5),
                galley,
                color,
            );
            if selected {
                let side = ui.spacing().icon_width_inner;
                paint::selection_mark(
                    ui,
                    Rect::from_center_size(
                        pos2(rect.right() - padding.x - side * 0.5, y),
                        Vec2::splat(side),
                    ),
                );
            }
        }
        response
    }
}

pub struct Checkbox<'a> {
    checked: &'a mut bool,
    label: &'a str,
}
pub struct Toggle<'a> {
    checked: &'a mut bool,
    label: &'a str,
}

impl<'a> Checkbox<'a> {
    pub fn new(checked: &'a mut bool, label: &'a str) -> Self {
        Self { checked, label }
    }
}

impl<'a> Toggle<'a> {
    pub fn new(checked: &'a mut bool, label: &'a str) -> Self {
        Self { checked, label }
    }
}

impl Widget for Checkbox<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        choice(ui, self.checked, self.label, false)
    }
}
impl Widget for Toggle<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        choice(ui, self.checked, self.label, true)
    }
}

fn choice(ui: &mut Ui, checked: &mut bool, label: &str, switch: bool) -> Response {
    let galley = WidgetText::from(label).into_galley(
        ui,
        Some(TextWrapMode::Extend),
        f32::INFINITY,
        TextStyle::Button,
    );
    let icon_height = ui.spacing().icon_width;
    let icon_width = icon_height * if switch { 2.0 } else { 1.0 };
    let desired = vec2(
        icon_width + ui.spacing().icon_spacing + galley.size().x,
        ui.spacing()
            .interact_size
            .y
            .max(galley.size().y)
            .max(icon_height),
    );
    let (rect, mut response) = ui.allocate_at_least(
        vec2(
            desired.x.min(ui.available_width().max(icon_width)),
            desired.y,
        ),
        Sense::click(),
    );
    focus_on_click(&response);
    scroll_on_focus(&response);
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
            vec2(icon_width, icon_height),
        );
        let visuals = paint::visuals(ui, &response, *checked);
        paint::control(ui, icon_rect, &visuals);
        let color = ui
            .visuals()
            .override_text_color
            .unwrap_or_else(|| visuals.text_color());
        if switch {
            let amount = ui.ctx().animate_bool_with_time(
                response.id.with("toggle"),
                *checked,
                ui.style().animation_time,
            );
            let x = egui::lerp(
                (icon_rect.left() + icon_height * 0.5)..=(icon_rect.right() - icon_height * 0.5),
                amount,
            );
            painter.add(paint::chamfer(
                Rect::from_center_size(
                    pos2(x, icon_rect.center().y),
                    Vec2::splat(ui.spacing().icon_width_inner),
                ),
                crate::theme::Tokens::get(ui).cut * 0.6,
                color,
                Stroke::NONE,
            ));
        } else if *checked {
            let check_rect = Rect::from_center_size(
                icon_rect.center(),
                Vec2::splat(ui.spacing().icon_width_inner),
            );
            paint::selection_mark(ui, check_rect);
        }
        let text_rect = Rect::from_min_max(
            pos2(icon_rect.right() + ui.spacing().icon_spacing, rect.top()),
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
