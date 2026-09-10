use egui::{Atom, Id, Response, Shape, Ui, Vec2, Widget};

use crate::{
    Icon,
    theme::{Tokens, paint},
};

use super::form::{Field, Validation, validation_color};

/// A labeled native single-line editor with optional help and validation.
/// The returned response belongs to the editor, including `changed`/`lost_focus`.
/// Enter preserves focus; values update through `changed`, and the host owns submission.
pub struct TextField<'a> {
    id: Id,
    value: &'a mut String,
    field: Field<'a>,
    hint: &'a str,
    icon: Option<Icon>,
    read_only: bool,
    password: bool,
    password_visible: Option<&'a mut bool>,
}

impl<'a> TextField<'a> {
    pub fn new(id: Id, value: &'a mut String) -> Self {
        Self {
            id,
            value,
            field: Field::new(id),
            hint: "",
            icon: None,
            read_only: false,
            password: false,
            password_visible: None,
        }
    }
    pub fn label(mut self, label: &'a str) -> Self {
        self.field = self.field.label(label);
        self
    }
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }
    pub fn help(mut self, help: &'a str) -> Self {
        self.field = self.field.help(help);
        self
    }
    pub fn validation(mut self, validation: Validation<'a>) -> Self {
        self.field = self.field.validation(validation);
        self
    }
    pub fn required(mut self, required: bool) -> Self {
        self.field = self.field.required(required);
        self
    }
    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }
    /// Read-only text remains selectable and copyable. Disable the whole field
    /// with `ui.add_enabled(false, field)` when interaction should be unavailable.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }
    pub fn password(mut self, password: bool) -> Self {
        self.password = password;
        self
    }
    /// Show an embedded visibility button for a password field. The editor keeps
    /// its own response; the button uses `field_id.with("visibility")` for focus.
    pub fn password_visible(mut self, visible: &'a mut bool) -> Self {
        self.password_visible = Some(visible);
        self
    }
}

impl Widget for TextField<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        if self.field.has_details() {
            self.field.show(ui, |ui| self.show_editor(ui))
        } else {
            self.show_editor(ui)
        }
    }
}

impl TextField<'_> {
    fn show_editor(self, ui: &mut Ui) -> Response {
        let tokens = Tokens::get(ui);
        let enabled = ui.is_enabled();
        let visibility_id = self.id.with("visibility");
        let password_visible = self.password_visible;
        if !enabled {
            ui.memory_mut(|memory| {
                memory.surrender_focus(self.id);
                memory.surrender_focus(visibility_id);
            });
        }
        let status_color = validation_color(ui);
        let background = ui.painter().add(Shape::Noop);
        let mut view = self.value.as_str();
        let buffer: &mut dyn egui::TextBuffer = if self.read_only {
            &mut view
        } else {
            self.value
        };
        let icon_id = self.id.with("icon");
        let content_height =
            ui.text_style_height(&egui::TextStyle::Body)
                .max(if self.icon.is_some() {
                    ui.spacing().icon_width_inner
                } else {
                    0.0
                });
        let vertical_padding = (40.0_f32.max(ui.spacing().interact_size.y) - content_height)
            .max(0.0)
            .ceil();
        let margin = egui::Margin {
            left: ui.spacing().button_padding.x.round() as i8,
            right: ui.spacing().button_padding.x.round() as i8,
            top: (vertical_padding / 2.0).ceil() as i8,
            bottom: (vertical_padding / 2.0).floor() as i8,
        };
        let mut editor = egui::TextEdit::singleline(buffer)
            .id(self.id)
            .hint_text(self.hint)
            .password(
                password_visible
                    .as_deref()
                    .map_or(self.password, |visible| !visible),
            )
            .interactive(enabled)
            .return_key(None)
            .desired_width(ui.available_width())
            .frame(egui::Frame::new().inner_margin(margin));
        if self.icon.is_some() {
            editor = editor.prefix(Atom::custom(
                icon_id,
                Vec2::splat(ui.spacing().icon_width_inner),
            ));
        }
        if password_visible.is_some() {
            editor = editor.suffix(Atom::custom(
                visibility_id,
                egui::vec2(28.0, ui.spacing().icon_width_inner),
            ));
        }
        let output = editor.show(ui);
        let icon_rect = output.response.rect(icon_id);
        let visibility_rect = output.response.rect(visibility_id);
        let response = output.response.response;
        let password_toggle = password_visible
            .zip(visibility_rect)
            .map(|(visible, slot)| {
                let rect = egui::Rect::from_center_size(
                    slot.center(),
                    egui::vec2(slot.width(), (response.rect.height() - 8.0).max(0.0)),
                )
                .intersect(response.rect.shrink(4.0));
                // Consume both pointer senses so the underlying editor cannot
                // start a cursor drag when the embedded button is pressed.
                let button = ui
                    .interact(rect, visibility_id, egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                crate::primitives::focus::focus_on_click(&button);
                crate::primitives::focus::scroll_on_focus(&button);
                if button.clicked() {
                    *visible = !*visible;
                    ui.ctx().request_repaint();
                }
                let label = if *visible {
                    "隐藏密码"
                } else {
                    "显示密码"
                };
                button.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, label)
                });
                (button.on_hover_text(label), *visible)
            });
        crate::primitives::focus::consume_escape_on_blur(&response);
        crate::primitives::focus::scroll_on_focus(&response);
        if ui.is_rect_visible(response.rect) {
            let mut visuals = paint::field_visuals(ui, &response);
            if let Some(color) = status_color {
                visuals.bg_stroke.color = color;
            }
            if paint::focused(ui, &response)
                || password_toggle
                    .as_ref()
                    .is_some_and(|(button, _)| button.has_focus())
            {
                visuals.bg_stroke.width = 2.0;
                if status_color.is_none() {
                    visuals.bg_stroke.color = tokens.focus;
                }
            }
            ui.painter().set(
                background,
                paint::rounded(
                    response.rect,
                    visuals.corner_radius,
                    visuals.bg_fill,
                    visuals.bg_stroke,
                ),
            );
            if let (Some(icon), Some(rect)) = (self.icon, icon_rect) {
                icon.paint(
                    &ui.painter_at(rect),
                    rect,
                    ui.visuals()
                        .override_text_color
                        .unwrap_or_else(|| ui.visuals().weak_text_color()),
                );
            }
        }
        if let Some((button, visible)) = password_toggle {
            let painter = ui.painter_at(button.rect);
            let active = button.enabled()
                && (button.hovered() || button.has_focus() || button.is_pointer_button_down_on());
            if active {
                painter.rect_filled(button.rect, 4, ui.visuals().faint_bg_color);
            }
            let color = if button.has_focus() {
                tokens.focus
            } else {
                ui.visuals().weak_text_color()
            };
            let icon = if visible { Icon::EyeOff } else { Icon::Eye };
            icon.paint(
                &painter,
                egui::Rect::from_center_size(
                    button.rect.center(),
                    Vec2::splat(ui.spacing().icon_width_inner),
                ),
                color,
            );
        }
        response
    }
}
