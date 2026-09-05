use egui::{Color32, Painter, Pos2, Rect, Response, Shape, Stroke, pos2, vec2};

use crate::{Palette, Theme};

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

pub(crate) fn corners(p: &Painter, rect: Rect, stroke: Stroke) {
    let length = 10.0_f32.min(rect.width() * 0.2).min(rect.height() * 0.3);
    for (corner, dx, dy) in [
        (rect.left_top(), 1.0, 1.0),
        (rect.right_top(), -1.0, 1.0),
        (rect.right_bottom(), -1.0, -1.0),
        (rect.left_bottom(), 1.0, -1.0),
    ] {
        p.add(Shape::line(
            vec![
                corner + vec2(dx * length, 0.0),
                corner + vec2(dx * 3.0, 0.0),
                corner + vec2(0.0, dy * 3.0),
                corner + vec2(0.0, dy * length),
            ],
            stroke,
        ));
    }
}

pub(crate) fn control(
    p: &Painter,
    rect: Rect,
    response: &Response,
    theme: &Theme,
    selected: bool,
    accent: Option<Color32>,
) {
    let c = theme.palette;
    let pressed = response.is_pointer_button_down_on();
    let focused = response.has_focus();
    let fill = if !response.enabled() || pressed {
        c.background
    } else if selected {
        c.moss.gamma_multiply(0.55)
    } else if response.hovered() {
        c.raised
    } else {
        c.panel
    };
    let border = if response.enabled() && (focused || response.hovered()) {
        c.brass
    } else {
        accent.unwrap_or(c.border)
    };
    p.add(chamfer(
        rect.shrink(0.5),
        theme.metrics.cut,
        fill,
        Stroke::new(1.0, border),
    ));
    if accent.is_some() {
        p.add(chamfer(
            rect.shrink(3.0),
            theme.metrics.cut - 1.0,
            Color32::TRANSPARENT,
            Stroke::new(0.5, border.gamma_multiply(0.5)),
        ));
    }
    if focused && response.enabled() {
        corners(p, rect.shrink(2.0), Stroke::new(2.0, c.brass));
    }
    if pressed {
        p.line_segment(
            [
                rect.left_top() + vec2(6.0, 3.0),
                rect.right_top() + vec2(-6.0, 3.0),
            ],
            Stroke::new(2.0, c.background),
        );
    }
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

pub(crate) fn text_color(palette: Palette, response: &Response) -> Color32 {
    if response.enabled() {
        palette.text
    } else {
        palette.muted
    }
}
