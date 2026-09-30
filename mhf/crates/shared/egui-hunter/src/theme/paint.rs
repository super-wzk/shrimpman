use egui::{Color32, CornerRadius, Rect, Response, Shape, Stroke, StrokeKind, Ui};

use super::Tokens;
use crate::components::ButtonKind;

const ACTIVE_PART: &str = "hunter-active-part";

pub(crate) fn focused(ui: &Ui, response: &Response) -> bool {
    response.enabled()
        && (response.has_focus()
            || ui
                .stack()
                .iter()
                .find_map(|node| node.tags().get_downcast::<bool>(ACTIVE_PART).copied())
                .unwrap_or(false))
}

/// A composite supplies its active part without registering another focus ID.
/// Native descendants and hunter widgets emphasize their existing border.
pub(crate) fn active_part<R>(
    ui: &mut Ui,
    active: bool,
    content: impl FnOnce(&mut Ui) -> R,
) -> egui::InnerResponse<R> {
    ui.scope_builder(
        egui::UiBuilder::new()
            .ui_stack_info(egui::UiStackInfo::default().with_tag_value(ACTIVE_PART, active)),
        |ui| {
            if active {
                let stroke = focus_stroke(ui, false);
                let visuals = ui.visuals_mut();
                visuals.widgets.inactive.bg_stroke = stroke;
                visuals.widgets.hovered.bg_stroke = stroke;
            }
            content(ui)
        },
    )
}

pub(crate) fn rounded(rect: Rect, radius: CornerRadius, fill: Color32, stroke: Stroke) -> Shape {
    Shape::Rect(egui::epaint::RectShape::new(
        rect,
        radius,
        fill,
        stroke,
        StrokeKind::Inside,
    ))
}

/// Persistent selection and semantic fills survive keyboard/controller focus.
pub(crate) fn visuals(ui: &Ui, response: &Response, selected: bool) -> egui::style::WidgetVisuals {
    let mut visuals = if response.enabled() && response.is_pointer_button_down_on() {
        ui.visuals().widgets.active
    } else if response.enabled() && (response.hovered() || response.highlighted()) {
        ui.visuals().widgets.hovered
    } else {
        ui.visuals().widgets.inactive
    };
    if selected {
        visuals.bg_fill = semantic_fill(response, ui.visuals().selection.bg_fill);
        visuals.weak_bg_fill = visuals.bg_fill;
        visuals.fg_stroke.color = ui.visuals().selection.stroke.color;
    }
    visuals
}

fn semantic_fill(response: &Response, fill: Color32) -> Color32 {
    if response.enabled() && response.is_pointer_button_down_on() {
        fill.lerp_to_gamma(Color32::BLACK, 0.12)
    } else if response.enabled() && (response.hovered() || response.highlighted()) {
        fill.lerp_to_gamma(Color32::WHITE, 0.06)
    } else {
        fill
    }
}

pub(crate) fn button_visuals(
    ui: &Ui,
    response: &Response,
    selected: bool,
    kind: ButtonKind,
) -> egui::style::WidgetVisuals {
    let mut visuals = visuals(ui, response, selected);
    visuals.bg_fill = visuals.weak_bg_fill;
    if matches!(kind, ButtonKind::Danger | ButtonKind::DangerQuiet) {
        visuals.fg_stroke.color = ui.visuals().error_fg_color;
    }
    if selected {
        return visuals;
    }
    let fill = match kind {
        ButtonKind::Default => return visuals,
        ButtonKind::Primary => {
            let tokens = Tokens::get(ui);
            visuals.fg_stroke.color = tokens.on_primary;
            visuals.bg_stroke = Stroke::NONE;
            tokens.primary
        }
        ButtonKind::Danger => {
            visuals.bg_stroke = Stroke::NONE;
            Tokens::get(ui).danger_fill
        }
        ButtonKind::Quiet | ButtonKind::DangerQuiet => {
            visuals.bg_stroke = Stroke::NONE;
            if response.enabled()
                && (response.hovered()
                    || response.highlighted()
                    || response.is_pointer_button_down_on())
            {
                visuals.bg_fill
            } else {
                Color32::TRANSPARENT
            }
        }
    };
    visuals.bg_fill = semantic_fill(response, fill);
    visuals.weak_bg_fill = visuals.bg_fill;
    visuals
}

pub(crate) fn field_visuals(ui: &Ui, response: &Response) -> egui::style::WidgetVisuals {
    let mut visuals = visuals(ui, response, false);
    visuals.bg_fill = ui.visuals().text_edit_bg_color();
    visuals.bg_stroke = ui.visuals().widgets.inactive.bg_stroke;
    visuals
}

/// Shared semantic surface and content color for text and icon buttons.
pub(crate) fn button(ui: &Ui, response: &Response, selected: bool, kind: ButtonKind) -> Color32 {
    let mut visuals = button_visuals(ui, response, selected, kind);
    if focused(ui, response) {
        visuals.bg_stroke = focus_stroke(ui, kind == ButtonKind::Primary && !selected);
    }
    control(ui, response.rect, &visuals);
    ui.visuals()
        .override_text_color
        .unwrap_or_else(|| visuals.text_color())
}

pub(crate) fn control(ui: &Ui, rect: Rect, visuals: &egui::style::WidgetVisuals) {
    ui.painter_at(rect).add(rounded(
        rect,
        visuals.corner_radius,
        visuals.bg_fill,
        visuals.bg_stroke,
    ));
}

pub(crate) fn focus_stroke(ui: &Ui, on_primary: bool) -> Stroke {
    let tokens = Tokens::get(ui);
    Stroke::new(
        2.0,
        if on_primary {
            tokens.on_primary
        } else {
            tokens.focus
        },
    )
}

/// Internal focus boundary for tab headers and keyboard-scroll viewports.
pub(crate) fn focus_border(ui: &Ui, rect: Rect, radius: CornerRadius) {
    ui.painter_at(rect)
        .rect_stroke(rect, radius, focus_stroke(ui, false), StrokeKind::Inside);
}

pub(crate) fn selection_mark(ui: &Ui, rect: Rect) {
    super::Icon::Check.paint(
        &ui.painter_at(rect),
        rect,
        ui.visuals().selection.stroke.color,
    );
}
