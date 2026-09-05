use egui::{Id, Margin, Rect, Response, RichText, Shape, Stroke, Ui, Vec2, Widget, pos2};

use crate::{Icon, Theme, paint};

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
pub struct TextField<'a> {
    theme: &'a Theme,
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

impl Theme {
    /// A search-style input using the native editor for selection, clipboard and IME.
    pub fn text_edit(&self, ui: &mut Ui, id: Id, value: &mut String, hint: &str) -> Response {
        ui.add(self.text_field(id, value).hint(hint).icon(Icon::Search))
    }

    pub fn text_field<'a>(&'a self, id: Id, value: &'a mut String) -> TextField<'a> {
        TextField {
            theme: self,
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
}

impl<'a> TextField<'a> {
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
            let theme = self.theme;
            let p = theme.palette;
            let enabled = ui.is_enabled();
            if !enabled {
                ui.memory_mut(|m| m.surrender_focus(self.id));
            }
            let label = self.label.map(|label| ui.label(label));
            let status = match self.validation {
                Validation::None => None,
                Validation::Warning(message) => Some((message, Icon::Warning, p.brass)),
                Validation::Error(message) => Some((message, Icon::Warning, p.danger)),
                Validation::Success(message) => Some((message, Icon::Check, p.moss)),
            };
            let background = ui.painter().add(Shape::Noop);
            let mut view = self.value.as_str();
            let buffer: &mut dyn egui::TextBuffer = if self.read_only || !enabled {
                &mut view
            } else {
                self.value
            };
            let mut response = ui.add(
                egui::TextEdit::singleline(buffer)
                    .id(self.id)
                    .hint_text(RichText::new(self.hint).color(p.muted))
                    .text_color(p.text)
                    .password(self.password)
                    .interactive(enabled)
                    .desired_width(ui.available_width())
                    .frame(egui::Frame::new().inner_margin(Margin {
                        left: if self.icon.is_some() { 36 } else { 12 },
                        right: 12,
                        top: 10,
                        bottom: 10,
                    })),
            );
            if let Some(label) = label {
                response = response.labelled_by(label.id);
            }
            if ui.is_rect_visible(response.rect) {
                let border = if let Some((_, _, color)) = status {
                    color
                } else if response.has_focus() || response.hovered() {
                    p.brass
                } else {
                    p.border
                };
                ui.painter().set(
                    background,
                    paint::chamfer(
                        response.rect.shrink(0.5),
                        theme.metrics.cut,
                        p.background,
                        Stroke::new(1.0, border),
                    ),
                );
                if let Some(icon) = self.icon {
                    icon.paint(
                        ui.painter(),
                        Rect::from_center_size(
                            pos2(response.rect.left() + 18.0, response.rect.center().y),
                            Vec2::splat(16.0),
                        ),
                        p.muted,
                    );
                }
                if response.has_focus() {
                    paint::corners(
                        ui.painter(),
                        response.rect.shrink(2.0),
                        Stroke::new(1.5, p.brass),
                    );
                }
            }
            if let Some((message, icon, color)) = status {
                ui.horizontal_top(|ui| {
                    let (rect, _) = ui.allocate_exact_size(Vec2::splat(16.0), egui::Sense::hover());
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
