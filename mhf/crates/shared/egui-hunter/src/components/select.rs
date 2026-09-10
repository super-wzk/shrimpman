use egui::{ComboBox, Id, InnerResponse, Stroke, Ui, UiBuilder, WidgetText};

use super::{
    form::{Field, Validation, validation_color},
    popup::interaction::PopupInteraction,
};
use crate::{Tokens, primitives::focus};

/// A native selection menu with the same label, help, and validation as other fields.
/// The menu owns its selection; inspect the responses of its native options for changes.
#[must_use = "Call show_ui to render the field"]
pub struct SelectField<'a> {
    id: Id,
    field: Field<'a>,
    /// Configure the native menu, including its height, icon, and closing behavior.
    pub native: ComboBox,
}

impl<'a> SelectField<'a> {
    pub fn new(id: Id, selected_text: impl Into<WidgetText>) -> Self {
        Self {
            id,
            field: Field::new(id),
            native: ComboBox::from_id_salt(id)
                .selected_text(selected_text)
                .truncate(),
        }
    }

    pub fn label(mut self, label: &'a str) -> Self {
        self.field = self.field.label(label);
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

    /// Returns the native button response and `None` while the menu is closed.
    pub fn show_ui<R>(
        self,
        ui: &mut Ui,
        menu_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        if self.field.has_details() {
            let mut inner = None;
            let response = self.field.show(ui, |ui| {
                let output = self.show_menu(ui, menu_contents);
                inner = output.inner;
                output.response
            });
            InnerResponse { inner, response }
        } else {
            self.show_menu(ui, menu_contents)
        }
    }

    fn show_menu<R>(
        self,
        ui: &mut Ui,
        menu_contents: impl FnOnce(&mut Ui) -> R,
    ) -> InnerResponse<Option<R>> {
        ui.scope_builder(UiBuilder::new().id(self.id.with("select")), |ui| {
            crate::input::discard_escape_repeats(ui.ctx());
            let enabled = ui.is_enabled();
            let original_widgets = ui.visuals().widgets.clone();
            let original_padding = ui.spacing().button_padding;
            let original_height = ui.spacing().interact_size.y;
            let original_width = ui.spacing().combo_width;
            let width = ui.available_width();
            let height = original_height.max(40.0);
            let content_height = ui
                .text_style_height(&egui::TextStyle::Button)
                .max(ui.spacing().icon_width);
            let padding = original_padding.y.max((height - content_height) / 2.0);
            ui.spacing_mut().interact_size.y = height;
            ui.spacing_mut().button_padding.y = padding;
            ui.spacing_mut().combo_width = width;

            let status = validation_color(ui);
            let focus_color = status.unwrap_or_else(|| Tokens::get(ui).focus);
            let widgets = &mut ui.visuals_mut().widgets;
            if let Some(color) = status {
                for visual in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                    &mut widgets.open,
                ] {
                    visual.bg_stroke.color = color;
                }
            }
            widgets.active.bg_stroke = Stroke::new(2.0, focus_color);
            widgets.open.bg_stroke = Stroke::new(2.0, focus_color);
            let field_widgets = widgets.clone();

            let output = self.native.show_ui(ui, |ui| {
                // Native popups live outside the disabled parent Ui. Close an
                // existing menu before its options can handle any queued input.
                if !enabled {
                    ui.close();
                    return None;
                }
                // The field's border and height belong to the closed control.
                // Restore inherited values in the menu, retaining native popup-style overrides.
                let widgets = &mut ui.visuals_mut().widgets;
                for (visual, field, original) in [
                    (
                        &mut widgets.inactive,
                        field_widgets.inactive,
                        original_widgets.inactive,
                    ),
                    (
                        &mut widgets.hovered,
                        field_widgets.hovered,
                        original_widgets.hovered,
                    ),
                    (
                        &mut widgets.active,
                        field_widgets.active,
                        original_widgets.active,
                    ),
                    (&mut widgets.open, field_widgets.open, original_widgets.open),
                ] {
                    if visual.bg_stroke == field.bg_stroke {
                        visual.bg_stroke = original.bg_stroke;
                    }
                }
                if ui.spacing().button_padding.y == padding {
                    ui.spacing_mut().button_padding.y = original_padding.y;
                }
                if ui.spacing().interact_size.y == height {
                    ui.spacing_mut().interact_size.y = original_height;
                }
                if ui.spacing().combo_width == width {
                    ui.spacing_mut().combo_width = original_width;
                }
                Some(menu_contents(ui))
            });
            if !output.response.enabled() {
                output.response.surrender_focus();
            }
            // egui 0.36.1 ComboBox::widget_to_popup_id uses this suffix.
            let popup_id = output.response.id.with("popup");
            let popup_response = output
                .inner
                .as_ref()
                .and_then(|_| ui.ctx().read_response(popup_id));
            PopupInteraction::after_show(&output.response, popup_id, popup_response.as_ref());
            focus::scroll_on_focus(&output.response);
            InnerResponse {
                inner: output.inner.flatten(),
                response: output.response,
            }
        })
        .inner
    }
}
