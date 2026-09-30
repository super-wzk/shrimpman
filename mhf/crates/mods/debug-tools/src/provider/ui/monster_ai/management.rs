use super::Editor;
use crate::provider::{AiOperation, DebugControl, DebugSnapshot, monsters};
use egui_hunter::{
    Button, ButtonKind, Panel, Popup, Property, ResponsiveColumns, Tokens, properties,
};

impl Editor {
    pub(in super::super) fn show_management(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        self.sync_target(snapshot, control);
        if self.selected.is_none()
            && snapshot.ready
            && let Some(target) = snapshot.ai_targets.first().copied()
        {
            self.select_target(Some(target), snapshot, control);
        }
        let width = ui.available_width().min(360.0);
        ui.horizontal_wrapped(|ui| {
            self.target_picker(ui, snapshot, control, width);
            if self.replacement.is_some()
                || self
                    .pending
                    .as_ref()
                    .is_some_and(|pending| pending.replacement.is_some())
            {
                ui.weak("等待替换后的怪物加载…");
            } else if self.pending.is_some() {
                ui.weak("处理中…");
            }
        });
        let active = self.active(snapshot);
        let status = if active {
            snapshot
                .monster_statuses
                .iter()
                .find(|status| Some(status.target) == self.selected)
        } else {
            None
        };
        egui::ScrollArea::vertical()
            .id_salt("monster-management-body")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ResponsiveColumns::new(egui::Id::new("monster-management-sections"))
                    .min_column_width(360.0)
                    .show(ui, 2, |ui, section| {
                        if section == 0 {
                            Panel::new("实例属性").show(ui, |ui| {
                                if let Some(status) = status {
                                    let values = [
                                        ("AI 主状态", status.ai_state.to_string()),
                                        (
                                            "当前动作",
                                            format!("{}:{}", status.action_group, status.action_id),
                                        ),
                                        ("动作阶段", status.action_stage.to_string()),
                                        ("动画编号", status.animation.to_string()),
                                        ("动画帧", format!("{:.1}", status.frame)),
                                        (
                                            "位置",
                                            format!(
                                                "X {:.1} · Y {:.1} · Z {:.1}",
                                                status.position[0],
                                                status.position[1],
                                                status.position[2]
                                            ),
                                        ),
                                    ];
                                    let rows = values
                                        .each_ref()
                                        .map(|(label, value)| Property::new(label, value));
                                    properties(ui, &rows);
                                } else {
                                    ui.weak(if !snapshot.ready {
                                        "任务未就绪"
                                    } else if self.selected.is_none() {
                                        "请选择怪物实例"
                                    } else {
                                        "实例状态暂不可用"
                                    });
                                }
                            });
                        } else {
                            Panel::new("种类替换").show(ui, |ui| {
                                ui.add_enabled_ui(
                                    active && self.pending.is_none() && self.replacement.is_none(),
                                    |ui| {
                                        self.species_picker(ui, snapshot, control);
                                    },
                                );
                            });
                        }
                    });
            });
        self.show_notifications(ui);
    }

    fn species_picker(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        control: &DebugControl,
    ) {
        ui.horizontal_wrapped(|ui| {
            ui.label("种类");
            let selected = self
                .replacement_species
                .or(self.selected.map(|target| target.species));
            let name = selected
                .and_then(|species| monsters::NAMES.get(usize::from(species)))
                .copied()
                .unwrap_or("选择种类");
            let picker = ui.add_sized(
                [
                    180.0_f32.min(ui.available_width()),
                    ui.spacing().interact_size.y,
                ],
                Button::new(&format!("{name} ▾")).id(egui::Id::new("replacement-species")),
            );
            let enabled = ui.is_enabled();
            // Use viewport space, not the popup's previous measured height:
            // a short search result must not cap the next unfiltered list.
            let screen = ui.ctx().content_rect();
            let space =
                (screen.bottom() - picker.rect.bottom()).max(picker.rect.top() - screen.top());
            let list_height = (space - ui.spacing().interact_size.y - 64.0).clamp(60.0, 240.0);
            let mut popup = Popup::new(&picker)
                .style(ui.style().clone())
                .tokens(Tokens::get(ui));
            popup.native = popup.native.width(picker.rect.width().max(220.0));
            popup.show(|ui| {
                if !enabled {
                    ui.close();
                    return;
                }
                let search = ui.add(
                    egui::TextEdit::singleline(&mut self.species_filter)
                        .hint_text("搜索名称或编号")
                        .desired_width(f32::INFINITY),
                );
                ui.separator();
                let mut list = egui::ScrollArea::vertical()
                    .id_salt("replacement-species-list")
                    .max_height(list_height)
                    .min_scrolled_height(list_height)
                    .auto_shrink([false, true]);
                if search.changed() {
                    list = list.vertical_scroll_offset(0.0);
                }
                list.show(ui, |ui| {
                    let filter = self.species_filter.trim();
                    let mut found = false;
                    for monster in &snapshot.catalog.monsters {
                        if filter.is_empty()
                            || monster.name.contains(filter)
                            || monster.id.to_string().contains(filter)
                        {
                            found = true;
                            if ui
                                .add(
                                    Button::new(&format!("{} · {}", monster.name, monster.id))
                                        .kind(ButtonKind::Quiet)
                                        .selected(self.replacement_species == Some(monster.id))
                                        .full_width(),
                                )
                                .clicked()
                            {
                                self.replacement_species = Some(monster.id);
                                ui.close();
                            }
                        }
                    }
                    if !found {
                        ui.weak("没有匹配的种类");
                    }
                });
            });
            let selected = self.replacement_species.or(selected);
            if ui
                .add_enabled(
                    selected.is_some_and(|species| {
                        self.selected
                            .is_some_and(|target| target.species != species)
                    }),
                    Button::new("替换并重载任务"),
                )
                .on_hover_text(
                    "仅替换选中目标的种类。重载任务以加载模型和 AI，任务进度会重置，目标条件不变。",
                )
                .clicked()
            {
                self.submit(control, AiOperation::ReplaceSpecies(selected.unwrap()));
            }
        });
    }
}
