use egui::{Color32, CornerRadius, Id, Painter, Rect, Shape, Stroke, TextureHandle, Vec2, pos2};

/// Theme-colored utility icons and original, untinted MHF weapon artwork.
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
    ArrowRight,
    ArrowUpRight,
    Plus,
    LogOut,
    Trash,
    Eye,
    EyeOff,
    SwordAndShield,
    HeavyBowgun,
    Hammer,
    GreatSword,
    Lance,
    LightBowgun,
    LongSword,
    DualBlades,
    HuntingHorn,
    Gunlance,
    Bow,
    Tonfa,
    SwitchAxe,
    MagnetSpike,
}

impl Icon {
    pub fn paint(self, p: &Painter, rect: Rect, color: Color32) {
        let rect =
            Rect::from_center_size(rect.center(), Vec2::splat(rect.width().min(rect.height())));
        if let Some((name, bytes)) = self.weapon_png() {
            let id = Id::new(("hunter-weapon-texture", name));
            let texture = p.ctx().data(|data| data.get_temp::<TextureHandle>(id));
            let texture = texture.unwrap_or_else(|| {
                let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
                    .expect("embedded weapon icons are validated PNGs")
                    .into_rgba8();
                let image = egui::ColorImage::from_rgba_unmultiplied(
                    [image.width() as usize, image.height() as usize],
                    image.as_raw(),
                );
                let texture = p
                    .ctx()
                    .load_texture(name, image, egui::TextureOptions::LINEAR);
                p.ctx()
                    .data_mut(|data| data.insert_temp(id, texture.clone()));
                texture
            });
            p.image(
                texture.id(),
                rect,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                Color32::WHITE,
            );
            return;
        }
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
                p.add(super::paint::rounded(
                    body,
                    CornerRadius::same(3),
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
            Self::ArrowRight => {
                line(&[(0.15, 0.5), (0.85, 0.5)]);
                line(&[(0.55, 0.2), (0.85, 0.5), (0.55, 0.8)]);
            }
            Self::ArrowUpRight => {
                line(&[(0.2, 0.8), (0.8, 0.2)]);
                line(&[(0.25, 0.2), (0.8, 0.2), (0.8, 0.75)]);
            }
            Self::Plus => {
                line(&[(0.2, 0.5), (0.8, 0.5)]);
                line(&[(0.5, 0.2), (0.5, 0.8)]);
            }
            Self::LogOut => {
                line(&[(0.45, 0.15), (0.15, 0.15), (0.15, 0.85), (0.45, 0.85)]);
                line(&[(0.4, 0.5), (0.9, 0.5)]);
                line(&[(0.68, 0.28), (0.9, 0.5), (0.68, 0.72)]);
            }
            Self::Trash => {
                line(&[(0.15, 0.28), (0.85, 0.28)]);
                line(&[(0.35, 0.28), (0.35, 0.13), (0.65, 0.13), (0.65, 0.28)]);
                line(&[(0.25, 0.28), (0.3, 0.88), (0.7, 0.88), (0.75, 0.28)]);
                line(&[(0.42, 0.43), (0.42, 0.7)]);
                line(&[(0.58, 0.43), (0.58, 0.7)]);
            }
            Self::Eye | Self::EyeOff => {
                line(&[
                    (0.08, 0.5),
                    (0.25, 0.3),
                    (0.5, 0.22),
                    (0.75, 0.3),
                    (0.92, 0.5),
                    (0.75, 0.7),
                    (0.5, 0.78),
                    (0.25, 0.7),
                    (0.08, 0.5),
                ]);
                p.circle_stroke(point(0.5, 0.5), rect.width() * 0.14, stroke);
                if self == Self::EyeOff {
                    line(&[(0.1, 0.1), (0.9, 0.9)]);
                }
            }
            // Weapon variants returned above use the original PNG, never theme tint.
            _ => unreachable!("weapon icon handled before vector painting"),
        }
    }

    fn weapon_png(self) -> Option<(&'static str, &'static [u8])> {
        macro_rules! png {
            ($name:literal) => {
                Some((
                    concat!("hunter-weapon-", $name),
                    include_bytes!(concat!("../../assets/weapons/", $name, ".png")).as_slice(),
                ))
            };
        }
        match self {
            Self::SwordAndShield => png!("sword-and-shield"),
            Self::HeavyBowgun => png!("heavy-bowgun"),
            Self::Hammer => png!("hammer"),
            Self::GreatSword => png!("great-sword"),
            Self::Lance => png!("lance"),
            Self::LightBowgun => png!("light-bowgun"),
            Self::LongSword => png!("long-sword"),
            Self::DualBlades => png!("dual-blades"),
            Self::HuntingHorn => png!("hunting-horn"),
            Self::Gunlance => png!("gunlance"),
            Self::Bow => png!("bow"),
            Self::Tonfa => png!("tonfa"),
            Self::SwitchAxe => png!("switch-axe-f"),
            Self::MagnetSpike => png!("magnet-spike"),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const WEAPONS: [Icon; 14] = [
        Icon::SwordAndShield,
        Icon::HeavyBowgun,
        Icon::Hammer,
        Icon::GreatSword,
        Icon::Lance,
        Icon::LightBowgun,
        Icon::LongSword,
        Icon::DualBlades,
        Icon::HuntingHorn,
        Icon::Gunlance,
        Icon::Bow,
        Icon::Tonfa,
        Icon::SwitchAxe,
        Icon::MagnetSpike,
    ];

    #[test]
    fn every_weapon_has_distinct_original_64px_artwork() {
        let mut names = std::collections::HashSet::new();
        let mut images = std::collections::HashSet::new();
        for icon in WEAPONS {
            let (name, bytes) = icon.weapon_png().expect("weapon artwork");
            let image = image::load_from_memory(bytes).unwrap().into_rgba8();
            assert_eq!(image.dimensions(), (64, 64), "{name}");
            assert!(names.insert(name), "duplicate weapon mapping: {name}");
            assert!(images.insert(bytes), "duplicate weapon image: {name}");
            assert!(
                image.pixels().any(|pixel| pixel[3] > 0 && pixel[3] < 255),
                "{name}: original antialiased edges must be preserved",
            );
        }
    }

    #[test]
    fn weapon_textures_are_cached_and_ignore_theme_tint() {
        let context = egui::Context::default();
        let mut texture_ids = Vec::new();
        for frame in 0..2 {
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(
                        egui::Pos2::ZERO,
                        Vec2::new(500.0, 300.0),
                    )),
                    ..Default::default()
                },
                |ui| {
                    for (index, icon) in WEAPONS.into_iter().enumerate() {
                        icon.paint(
                            ui.painter(),
                            Rect::from_min_size(
                                pos2((index % 7) as f32 * 64.0, (index / 7) as f32 * 64.0),
                                Vec2::splat(64.0),
                            ),
                            if frame == 0 {
                                Color32::RED
                            } else {
                                Color32::GOLD
                            },
                        );
                        let (name, _) = icon.weapon_png().unwrap();
                        let texture = context
                            .data(|data| {
                                data.get_temp::<TextureHandle>(Id::new((
                                    "hunter-weapon-texture",
                                    name,
                                )))
                            })
                            .unwrap();
                        assert_eq!(texture.size(), [64, 64]);
                        if frame == 0 {
                            texture_ids.push(texture.id());
                        } else {
                            assert_eq!(texture.id(), texture_ids[index]);
                        }
                    }
                },
            );
            let meshes: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    Shape::Mesh(mesh) if texture_ids.contains(&mesh.texture_id) => Some(mesh),
                    _ => None,
                })
                .collect();
            assert_eq!(meshes.len(), 14);
            assert!(
                meshes
                    .iter()
                    .flat_map(|mesh| &mesh.vertices)
                    .all(|vertex| vertex.color == Color32::WHITE),
            );
            if frame == 1 {
                assert!(
                    output
                        .textures_delta
                        .set
                        .iter()
                        .all(|(id, _)| !texture_ids.contains(id)),
                    "cached weapon textures should not upload again",
                );
            }
            output.textures_delta.clear();
        }
    }
}
