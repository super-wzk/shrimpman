use super::{DebugSnapshot, NATIVE_WEAPON_NAMES};

pub(super) fn show(context: &egui::Context, snapshot: &DebugSnapshot) {
    if !snapshot.ready {
        return;
    }
    egui::Area::new(egui::Id::new("debug-runtime-hud"))
        .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(12.0, -12.0))
        .order(egui::Order::Middle)
        .movable(false)
        .interactable(false)
        .show(context, |ui| {
            ui.set_max_width((context.content_rect().width() - 24.0).max(80.0));
            egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(170))
                .corner_radius(6)
                .inner_margin(egui::Margin::symmetric(10, 8))
                .show(ui, |ui| {
                    ui.visuals_mut().override_text_color = Some(egui::Color32::from_gray(230));
                    ui.label(format!(
                        "装备：{} · 招式：{}",
                        NATIVE_WEAPON_NAMES
                            .get(snapshot.equipped_weapon as usize)
                            .unwrap_or(&"未知武器"),
                        NATIVE_WEAPON_NAMES
                            .get(snapshot.weapon as usize)
                            .unwrap_or(&"未知武器")
                    ));
                    if let Some(species) = snapshot.monster {
                        let variant = snapshot
                            .catalog
                            .monsters
                            .iter()
                            .find(|monster| monster.id == species)
                            .and_then(|monster| monster.variant(snapshot.monster_variant));
                        ui.strong(format!(
                            "变身：{} · {}{}",
                            super::super::monsters::NAMES
                                .get(usize::from(species))
                                .unwrap_or(&"未知怪物"),
                            variant.map_or("未知变种", |variant| variant.name),
                            if snapshot.controlling_monster {
                                ""
                            } else {
                                "（等待初始化）"
                            }
                        ));
                    }
                    ui.label(format!(
                        "状态 {}:{} / {} · 动画 {} · 帧 {:.1}",
                        snapshot.action_group,
                        snapshot.action_id,
                        snapshot.action_stage,
                        snapshot.animation,
                        snapshot.frame
                    ));
                    ui.small(format!(
                        "位置  X {:.1}  Y {:.1}  Z {:.1}",
                        snapshot.position[0], snapshot.position[1], snapshot.position[2]
                    ));
                });
        });
}
