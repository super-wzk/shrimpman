use egui::{Color32, Painter, Pos2, Rect, Response, Shape, Stroke, Ui, pos2, vec2};

use super::Tokens;
use crate::components::ButtonKind;

const ACTIVE_PART: &str = "hunter-active-part";

fn focused(ui: &Ui, response: &Response) -> bool {
    response.enabled()
        && (response.has_focus()
            || ui
                .stack()
                .iter()
                .find_map(|node| node.tags().get_downcast::<bool>(ACTIVE_PART).copied())
                .unwrap_or(false))
}

/// A composite supplies its active part before rendering. Native widgets read
/// the scoped style; hunter widgets also preserve focus priority over selection.
/// This scope carries visual state only, without registering another focus ID.
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
                let visuals = ui.visuals_mut();
                visuals.widgets.inactive = visuals.widgets.active;
                visuals.widgets.hovered = visuals.widgets.active;
                visuals.selection.bg_fill = visuals.widgets.active.bg_fill;
            }
            content(ui)
        },
    )
}

pub(crate) fn chamfer(rect: Rect, cut: f32, fill: Color32, stroke: Stroke) -> Shape {
    let c = cut
        .max(0.0)
        .min(rect.width().min(rect.height()).max(0.0) * 0.5);
    Shape::convex_polygon(
        vec![
            pos2(rect.left() + c, rect.top()),
            pos2(rect.right() - c, rect.top()),
            pos2(rect.right(), rect.top() + c),
            pos2(rect.right(), rect.bottom() - c),
            pos2(rect.right() - c, rect.bottom()),
            pos2(rect.left() + c, rect.bottom()),
            pos2(rect.left(), rect.bottom() - c),
            pos2(rect.left(), rect.top() + c),
        ],
        fill,
        stroke,
    )
}

pub(crate) fn diamond(p: &Painter, center: Pos2, radius: f32, color: Color32) {
    p.add(Shape::convex_polygon(
        vec![
            center - vec2(0.0, radius),
            center + vec2(radius, 0.0),
            center + vec2(0.0, radius),
            center - vec2(radius, 0.0),
        ],
        color,
        Stroke::NONE,
    ));
}

/// Use egui's native interaction palette. A selected value keeps its check,
/// while keyboard focus takes priority over the persistent selected fill.
pub(crate) fn visuals(ui: &Ui, response: &Response, selected: bool) -> egui::style::WidgetVisuals {
    let focused = focused(ui, response);
    let mut visuals = if focused {
        ui.visuals().widgets.active
    } else {
        *ui.style().interact(response)
    };
    if selected && !focused {
        let fill = semantic_fill(ui, response, ui.visuals().selection.bg_fill);
        visuals.bg_fill = fill;
        visuals.weak_bg_fill = fill;
    }
    visuals
}

fn semantic_fill(ui: &Ui, response: &Response, fill: Color32) -> Color32 {
    if response.enabled() && response.is_pointer_button_down_on() {
        fill.lerp_to_gamma(ui.visuals().widgets.active.bg_fill, 0.6)
    } else if response.enabled() && (response.hovered() || response.highlighted()) {
        fill.lerp_to_gamma(ui.visuals().widgets.hovered.bg_fill, 0.25)
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
    if !selected && !focused(ui, response) {
        let fill = match kind {
            ButtonKind::Default => return visuals,
            ButtonKind::Primary => Tokens::get(ui).primary,
            ButtonKind::Danger => ui
                .visuals()
                .widgets
                .inactive
                .weak_bg_fill
                .lerp_to_gamma(ui.visuals().error_fg_color, 0.2),
        };
        visuals.bg_fill = semantic_fill(ui, response, fill);
        visuals.weak_bg_fill = visuals.bg_fill;
    }
    visuals
}

pub(crate) fn field_visuals(ui: &Ui, response: &Response) -> egui::style::WidgetVisuals {
    let mut visuals = visuals(ui, response, false);
    if !focused(ui, response) {
        visuals.bg_fill = semantic_fill(ui, response, ui.visuals().text_edit_bg_color());
    }
    visuals
}

pub(crate) fn control(ui: &Ui, rect: Rect, visuals: &egui::style::WidgetVisuals) {
    ui.painter_at(rect).add(chamfer(
        rect.shrink(0.5),
        Tokens::get(ui).cut,
        visuals.bg_fill,
        visuals.bg_stroke,
    ));
}

pub(crate) fn selection_mark(ui: &Ui, rect: Rect) {
    super::Icon::Check.paint(
        &ui.painter_at(rect),
        rect,
        ui.visuals().selection.stroke.color,
    );
}

/// Bounded, deterministic grain. No textures, I/O, timers or random state.
pub(crate) fn grain(rect: Rect, light: bool) -> Shape {
    let mut mesh = egui::Mesh::default();
    let columns = ((rect.width() / 13.0) as usize).min(120);
    let rows = ((rect.height() / 13.0) as usize).min(90);
    let tint = if light {
        Color32::from_black_alpha(9)
    } else {
        Color32::from_white_alpha(5)
    };
    for y in 0..rows {
        for x in 0..columns {
            let hash = (x.wrapping_mul(73_856_093) ^ y.wrapping_mul(19_349_663)) as u32;
            let offset = vec2((hash % 7) as f32, ((hash >> 4) % 7) as f32);
            let at = rect.min + vec2(x as f32 * 13.0 + 4.0, y as f32 * 13.0 + 4.0) + offset;
            mesh.add_colored_rect(Rect::from_min_size(at, vec2(1.4, 0.7)), tint);
        }
    }
    Shape::mesh(mesh)
}
