use egui::{Atom, Id, Response, RichText, Shape, Ui, Vec2, Widget};

use crate::{
    Icon,
    theme::{Tokens, paint},
};

/// Validation is supplied by the host. A message replaces the ordinary help text.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Validation<'a> {
    #[default]
    None,
    Warning(&'a str),
    Error(&'a str),
    Success(&'a str),
}

/// A labeled native single-line editor with optional help and validation.
/// The returned response belongs to the editor, including `changed`/`lost_focus`.
/// Enter preserves focus; values update through `changed`, and the host owns submission.
pub struct TextField<'a> {
    id: Id,
    value: &'a mut String,
    label: Option<&'a str>,
    hint: &'a str,
    help: Option<&'a str>,
    validation: Validation<'a>,
    icon: Option<Icon>,
    read_only: bool,
    password: bool,
}

impl<'a> TextField<'a> {
    pub fn new(id: Id, value: &'a mut String) -> Self {
        Self {
            id,
            value,
            label: None,
            hint: "",
            help: None,
            validation: Validation::None,
            icon: None,
            read_only: false,
            password: false,
        }
    }
    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
        self
    }
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }
    pub fn help(mut self, help: &'a str) -> Self {
        self.help = Some(help);
        self
    }
    pub fn validation(mut self, validation: Validation<'a>) -> Self {
        self.validation = validation;
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
}

impl Widget for TextField<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        ui.vertical(|ui| {
            let tokens = Tokens::get(ui);
            let enabled = ui.is_enabled();
            if !enabled {
                ui.memory_mut(|m| m.surrender_focus(self.id));
            }
            let label = self.label.map(|label| ui.label(label));
            let status = match self.validation {
                Validation::None => None,
                Validation::Warning(message) => {
                    Some((message, Icon::Warning, ui.visuals().warn_fg_color))
                }
                Validation::Error(message) => {
                    Some((message, Icon::Warning, ui.visuals().error_fg_color))
                }
                Validation::Success(message) => Some((message, Icon::Check, tokens.success)),
            };
            let background = ui.painter().add(Shape::Noop);
            let mut view = self.value.as_str();
            let buffer: &mut dyn egui::TextBuffer = if self.read_only {
                &mut view
            } else {
                self.value
            };
            let icon_id = self.id.with("icon");
            let mut editor = egui::TextEdit::singleline(buffer)
                .id(self.id)
                .hint_text(self.hint)
                .password(self.password)
                .interactive(enabled)
                .return_key(None)
                .desired_width(ui.available_width())
                .frame(egui::Frame::new().inner_margin(ui.spacing().button_padding));
            if self.icon.is_some() {
                editor = editor.prefix(Atom::custom(
                    icon_id,
                    Vec2::splat(ui.spacing().icon_width_inner),
                ));
            }
            let output = editor.show(ui);
            let icon_rect = output.response.rect(icon_id);
            let mut response = output.response.response;
            if let Some(label) = label {
                response = response.labelled_by(label.id);
            }
            crate::primitives::focus::consume_escape_on_blur(&response);
            crate::primitives::focus::scroll_on_focus(&response);
            if ui.is_rect_visible(response.rect) {
                let visuals = paint::field_visuals(ui, &response);
                ui.painter().set(
                    background,
                    paint::chamfer(
                        response.rect.shrink(0.5),
                        tokens.cut,
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
            if let Some((message, icon, color)) = status {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(
                        Vec2::splat(ui.spacing().icon_width_inner),
                        egui::Sense::hover(),
                    );
                    icon.paint(ui.painter(), rect, color);
                    ui.add(egui::Label::new(RichText::new(message).small().color(color)).wrap());
                });
            } else if let Some(help) = self.help {
                ui.add(egui::Label::new(RichText::new(help).small().weak()).wrap());
            }
            response
        })
        .inner
    }
}
