use std::sync::Arc;

use egui::{
    Align, Color32, Galley, Id, Label, Layout, Rect, Response, RichText, TextStyle, TextWrapMode,
    Ui, UiBuilder, UiStackInfo, Vec2, WidgetText, pos2, vec2,
};

use super::{VALIDATION_COLOR, Validation};
use crate::{Icon, Tokens};

/// A label, one control, and optional feedback. The host owns the value and validation.
/// Give each field a stable ID; the returned response belongs to the control.
#[derive(Clone, Copy, Debug)]
pub struct Field<'a> {
    pub(super) id: Id,
    label: Option<&'a str>,
    help: Option<&'a str>,
    validation: Validation<'a>,
    required: bool,
    label_vertical_align: Align,
}

pub(super) enum FieldLayout {
    Above {
        label_height: f32,
        align: Align,
    },
    Left {
        label_width: f32,
        control_offset: f32,
        align: Align,
    },
}

impl<'a> Field<'a> {
    pub fn new(id: Id) -> Self {
        Self {
            id,
            label: None,
            help: None,
            validation: Validation::None,
            required: false,
            label_vertical_align: Align::Center,
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.label = Some(label);
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

    /// Display a required marker. This does not validate the value.
    pub fn required(mut self, required: bool) -> Self {
        self.required = required;
        self
    }

    /// Align a left-hand label to the control itself, excluding help and errors.
    /// Use `Align::Min` for a multiline or otherwise tall control.
    pub fn label_vertical_align(mut self, align: Align) -> Self {
        self.label_vertical_align = align;
        self
    }

    pub(crate) fn has_details(&self) -> bool {
        self.label.is_some()
            || self.help.is_some()
            || self.validation != Validation::None
            || self.required
    }

    pub(super) fn has_label(&self) -> bool {
        self.label.is_some()
    }

    pub fn show(self, ui: &mut Ui, control: impl FnOnce(&mut Ui) -> Response) -> Response {
        let label = self.label_galley(ui, ui.available_width());
        let label_height = label.as_ref().map_or(0.0, |label| label.size().y);
        self.show_with_layout(
            ui,
            self.id,
            label,
            FieldLayout::Above {
                label_height,
                align: Align::Min,
            },
            control,
        )
    }

    pub(super) fn label_galley(self, ui: &Ui, width: f32) -> Option<Arc<Galley>> {
        self.label.map(|label| {
            let text = if self.required {
                format!("{label} *")
            } else {
                label.to_owned()
            };
            WidgetText::from(text).into_galley(
                ui,
                Some(TextWrapMode::Wrap),
                width.max(0.0),
                TextStyle::Body,
            )
        })
    }

    pub(super) fn control_offset(self, label_height: f32, min_control_height: f32) -> f32 {
        aligned_offset(
            self.label_vertical_align,
            (label_height - min_control_height).max(0.0),
        )
    }

    pub(super) fn show_with_layout(
        self,
        ui: &mut Ui,
        id: Id,
        label: Option<Arc<Galley>>,
        layout: FieldLayout,
        control: impl FnOnce(&mut Ui) -> Response,
    ) -> Response {
        ui.scope_builder(
            UiBuilder::new()
                .id(id.with("field"))
                .layout(Layout::top_down(Align::Min)),
            |ui| match layout {
                FieldLayout::Above {
                    label_height,
                    align,
                } => {
                    let label_response = if label_height > 0.0 {
                        let width = ui.available_width().max(0.0);
                        ui.allocate_ui_with_layout(
                            vec2(width, label_height),
                            Layout::top_down(align),
                            |ui| {
                                ui.set_width(width);
                                ui.set_min_height(label_height);
                                label.map(|label| ui.add(Label::new(label)))
                            },
                        )
                        .inner
                    } else {
                        None
                    };
                    let response = self.control(ui, id, control);
                    if let Some(label) = label_response {
                        response.labelled_by(label.id)
                    } else {
                        response
                    }
                }
                FieldLayout::Left {
                    label_width,
                    control_offset,
                    align,
                } => {
                    let start = ui.next_widget_position();
                    let gap = if label_width > 0.0 {
                        ui.spacing().item_spacing.x
                    } else {
                        0.0
                    };
                    let width = (ui.available_width() - label_width - gap).max(0.0);
                    let control_rect = Rect::from_min_max(
                        start + vec2(label_width + gap, control_offset),
                        pos2(start.x + label_width + gap + width, ui.max_rect().bottom()),
                    );
                    let mut response = ui
                        .scope_builder(
                            UiBuilder::new()
                                .max_rect(control_rect)
                                .layout(Layout::top_down(Align::Min)),
                            |ui| {
                                ui.set_width(width);
                                self.control(ui, id, control)
                            },
                        )
                        .inner;
                    if let Some(label) = label {
                        let size = label.size();
                        let x = start.x + aligned_offset(align, label_width - size.x);
                        let y = response.rect.top()
                            + aligned_offset(
                                self.label_vertical_align,
                                response.rect.height() - size.y,
                            );
                        let mut label_ui = ui.new_child(
                            UiBuilder::new()
                                .id(id.with("label"))
                                .max_rect(Rect::from_min_size(pos2(x, y.max(start.y)), size))
                                .layout(Layout::top_down(Align::Min)),
                        );
                        let label_response = label_ui.add(Label::new(label));
                        ui.expand_to_include_rect(label_response.rect);
                        response = response.labelled_by(label_response.id);
                    }
                    response
                }
            },
        )
        .inner
    }

    fn status(self, ui: &Ui) -> Option<(&'a str, Icon, Color32)> {
        match self.validation {
            Validation::None => None,
            Validation::Warning(message) => {
                Some((message, Icon::Warning, ui.visuals().warn_fg_color))
            }
            Validation::Error(message) => {
                Some((message, Icon::Warning, ui.visuals().error_fg_color))
            }
            Validation::Success(message) => Some((message, Icon::Check, Tokens::get(ui).success)),
        }
    }

    fn control(self, ui: &mut Ui, id: Id, control: impl FnOnce(&mut Ui) -> Response) -> Response {
        let status = self.status(ui);
        ui.scope_builder(
            UiBuilder::new().id(id.with("control")).ui_stack_info(
                UiStackInfo::default()
                    .with_tag_value(VALIDATION_COLOR, status.map(|(_, _, color)| color)),
            ),
            |ui| {
                ui.spacing_mut().interact_size.y = ui.spacing().interact_size.y.max(40.0);
                let response = control(ui);
                if let Some((message, icon, color)) = status {
                    ui.horizontal_top(|ui| {
                        let (rect, _) = ui.allocate_exact_size(
                            Vec2::splat(ui.spacing().icon_width_inner),
                            egui::Sense::hover(),
                        );
                        icon.paint(ui.painter(), rect, color);
                        ui.add(Label::new(RichText::new(message).small().color(color)).wrap());
                    });
                } else if let Some(help) = self.help {
                    ui.add(Label::new(RichText::new(help).small().weak()).wrap());
                }
                response
            },
        )
        .inner
    }
}

fn aligned_offset(align: Align, available: f32) -> f32 {
    match align {
        Align::Min => 0.0,
        Align::Center => available * 0.5,
        Align::Max => available,
    }
}
