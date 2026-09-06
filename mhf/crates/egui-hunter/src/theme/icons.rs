use egui::{Color32, Painter, Rect, Shape, Stroke, Vec2, pos2};

/// Small vector icons. Supply your own texture to an item slot for game artwork.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Quest,
    Sword,
    Potion,
    Herb,
    Bone,
    Ore,
    Trap,
    Check,
    Warning,
    Search,
}

impl Icon {
    pub fn paint(self, p: &Painter, rect: Rect, color: Color32) {
        let rect =
            Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())));
        let point = |x, y| {
            pos2(
                rect.left() + x * rect.width(),
                rect.top() + y * rect.height(),
            )
        };
        let stroke = Stroke::new((rect.width() / 20.0).clamp(1.2, 2.5), color);
        let line = |points: &[(f32, f32)]| {
            p.add(Shape::line(
                points.iter().map(|&(x, y)| point(x, y)).collect(),
                stroke,
            ));
        };
        match self {
            Self::Quest => {
                line(&[
                    (0.2, 0.2),
                    (0.72, 0.2),
                    (0.82, 0.3),
                    (0.82, 0.85),
                    (0.2, 0.85),
                    (0.2, 0.2),
                ]);
                line(&[(0.36, 0.4), (0.65, 0.4)]);
                line(&[(0.36, 0.56), (0.65, 0.56)]);
                line(&[(0.36, 0.72), (0.51, 0.72)]);
                line(&[(0.34, 0.08), (0.34, 0.26)]);
            }
            Self::Sword => {
                line(&[
                    (0.24, 0.76),
                    (0.7, 0.15),
                    (0.88, 0.08),
                    (0.87, 0.28),
                    (0.33, 0.83),
                ]);
                line(&[(0.17, 0.61), (0.44, 0.85)]);
                line(&[(0.25, 0.75), (0.12, 0.91)]);
            }
            Self::Potion => {
                let body = Rect::from_min_max(point(0.22, 0.38), point(0.78, 0.9));
                p.add(super::paint::chamfer(
                    body,
                    rect.width() * 0.1,
                    color.gamma_multiply(0.2),
                    stroke,
                ));
                line(&[(0.36, 0.4), (0.36, 0.17), (0.64, 0.17), (0.64, 0.4)]);
                line(&[(0.31, 0.13), (0.69, 0.13)]);
                line(&[(0.33, 0.66), (0.67, 0.66)]);
                p.circle_filled(point(0.5, 0.66), rect.width() * 0.07, color);
            }
            Self::Herb => {
                line(&[(0.48, 0.91), (0.55, 0.13)]);
                for (a, b, c) in [
                    ((0.51, 0.69), (0.14, 0.42), (0.17, 0.72)),
                    ((0.53, 0.49), (0.84, 0.24), (0.86, 0.56)),
                    ((0.53, 0.35), (0.27, 0.12), (0.24, 0.4)),
                ] {
                    p.add(Shape::convex_polygon(
                        vec![point(a.0, a.1), point(b.0, b.1), point(c.0, c.1)],
                        color.gamma_multiply(0.3),
                        stroke,
                    ));
                }
            }
            Self::Bone => {
                line(&[(0.27, 0.34), (0.67, 0.73)]);
                line(&[(0.35, 0.27), (0.75, 0.65)]);
                for (x, y) in [(0.22, 0.27), (0.3, 0.2), (0.73, 0.8), (0.82, 0.72)] {
                    p.circle_stroke(point(x, y), rect.width() * 0.1, stroke);
                }
            }
            Self::Ore => {
                p.add(Shape::convex_polygon(
                    vec![
                        point(0.21, 0.73),
                        point(0.15, 0.46),
                        point(0.49, 0.13),
                        point(0.8, 0.36),
                        point(0.87, 0.7),
                        point(0.52, 0.91),
                    ],
                    color.gamma_multiply(0.18),
                    stroke,
                ));
                line(&[(0.49, 0.13), (0.42, 0.54), (0.52, 0.91)]);
                line(&[(0.15, 0.46), (0.42, 0.54), (0.8, 0.36)]);
            }
            Self::Trap => {
                line(&[
                    (0.1, 0.72),
                    (0.9, 0.72),
                    (0.78, 0.88),
                    (0.22, 0.88),
                    (0.1, 0.72),
                ]);
                for x in [0.2, 0.4, 0.6, 0.8] {
                    line(&[(x - 0.07, 0.7), (x, 0.33), (x + 0.07, 0.7)]);
                }
            }
            Self::Check => line(&[(0.17, 0.51), (0.4, 0.75), (0.85, 0.22)]),
            Self::Warning => {
                line(&[(0.5, 0.09), (0.94, 0.88), (0.06, 0.88), (0.5, 0.09)]);
                line(&[(0.5, 0.35), (0.5, 0.58)]);
                p.circle_filled(point(0.5, 0.73), rect.width() * 0.035, color);
            }
            Self::Search => {
                p.circle_stroke(point(0.42, 0.4), rect.width() * 0.26, stroke);
                line(&[(0.62, 0.61), (0.9, 0.89)]);
            }
        }
    }
}
