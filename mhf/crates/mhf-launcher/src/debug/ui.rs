use super::{
    Action, DebugCommand, DebugControl, DebugSnapshot, MonsterAction, MonsterInput,
    NATIVE_WEAPON_NAMES,
};
use egui::{Context, Key, Modifiers};
use std::sync::Arc;

pub(crate) struct DebugWindow {
    control: Arc<DebugControl>,
    open: bool,
    page: usize,
    weapon: u8,
    slot: u8,
    filter: String,
    action_filter: String,
    action_weapon: Option<u8>,
    error: String,
    monster_filter: String,
    monster_species: u8,
    monster_speed: f32,
    monster_shortcuts: [Option<MonsterAction>; 4],
    focused: bool,
    monster_action_filter: String,
    selected_area: Option<u16>,
}

impl DebugWindow {
    pub(crate) fn new(control: Arc<DebugControl>) -> Self {
        Self {
            control,
            open: true,
            page: 0,
            weapon: 0,
            slot: 6,
            filter: String::new(),
            action_filter: String::new(),
            action_weapon: None,
            error: String::new(),
            monster_filter: String::new(),
            monster_species: 94,
            monster_speed: 300.0,
            monster_shortcuts: [None; 4],
            focused: true,
            monster_action_filter: String::new(),
            selected_area: None,
        }
    }
    fn send(&mut self, command: DebugCommand) {
        self.error = self.control.send(command).err().unwrap_or_default();
    }
    pub(crate) fn show(&mut self, context: &Context) {
        if context.input_mut(|input| {
            let pressed = input.events.iter().any(|event| {
                matches!(event,
                egui::Event::Key { key: Key::F7, pressed: true, repeat: false, modifiers, .. }
                    if *modifiers == Modifiers::NONE)
            });
            input.consume_key(Modifiers::NONE, Key::F7);
            pressed
        }) {
            self.open = !self.open;
            self.focused = self.open;
        }
        let snapshot = self.control.snapshot();
        let mut open = self.open;
        let viewport = context.content_rect();
        let max_width = (viewport.width() - 32.0).max(80.0);
        let max_height = (viewport.height() - 72.0).max(48.0);
        let window = egui::Window::new("任务调试 · F7")
            .id(egui::Id::new("quest-debugger"))
            .open(&mut open)
            .default_pos(viewport.min + egui::vec2(16.0, 16.0))
            .default_width(430.0_f32.min(max_width))
            .min_width(160.0_f32.min(max_width))
            .max_width(max_width)
            .default_height(460.0_f32.min(max_height))
            .min_height(96.0_f32.min(max_height))
            .max_height(max_height)
            .constrain_to(viewport.shrink(8.0))
            .vscroll(true)
            .show(context, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "任务 {} · 区域 {}",
                        snapshot.quest_id, snapshot.area
                    ));
                    ui.label(if snapshot.ready {
                        "可调试"
                    } else {
                        "加载 / 结算中"
                    });
                });
                self.area_controls(ui, &snapshot);
                if snapshot.controlling_monster {
                    egui::CollapsingHeader::new("交战状态")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.small(format!(
                                "命中检查 {} · 确认命中 {}",
                                snapshot.combat.checks, snapshot.combat.hits
                            ));
                            egui::ScrollArea::vertical()
                                .id_salt("debug-combat-health")
                                .max_height(100.0)
                                .show(ui, |ui| {
                                    for monster in &snapshot.combat.health {
                                        let name = super::monsters::NAMES
                                            .get(usize::from(monster.species))
                                            .unwrap_or(&"未知怪物");
                                        ui.label(format!(
                                            "{} {name} #{} · HP {}",
                                            if monster.controlled {
                                                "受控"
                                            } else {
                                                "敌方"
                                            },
                                            monster.slot,
                                            monster.hp
                                        ));
                                    }
                                });
                            if !snapshot.combat.last_damage.is_empty() {
                                ui.label(&snapshot.combat.last_damage);
                            }
                        });
                }
                ui.collapsing("运行状态", |ui| {
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
                        ui.strong(format!(
                            "变身：{}{}",
                            super::monsters::NAMES[species as usize],
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
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add_enabled(
                            snapshot.ready || snapshot.scene == 5,
                            egui::Button::new("重开任务"),
                        )
                        .clicked()
                    {
                        self.send(DebugCommand::Restart);
                    }
                    if ui.button("结束调试").clicked() {
                        self.send(DebugCommand::Exit);
                    }
                    ui.weak("F7 显示 / 隐藏");
                });
                if !snapshot.message.is_empty() {
                    ui.label(&snapshot.message);
                }
                if !self.error.is_empty() {
                    ui.colored_label(egui::Color32::LIGHT_RED, &self.error);
                }
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.selectable_value(&mut self.page, 0, "装备");
                    ui.selectable_value(&mut self.page, 1, "实际招式");
                    ui.selectable_value(&mut self.page, 2, "怪物变身");
                });
                match self.page {
                    0 => self.equipment(ui, &snapshot),
                    1 => self.actions(ui, &snapshot),
                    _ => self.monsters(ui, &snapshot),
                }
            });
        self.open = open;
        if !self.open {
            self.focused = false;
        } else if let Some(window) = window {
            let pressed = context.input(|input| {
                input
                    .pointer
                    .any_pressed()
                    .then(|| input.pointer.interact_pos())
                    .flatten()
            });
            if let Some(position) = pressed {
                self.focused = window.response.rect.contains(position)
                    || context
                        .layer_id_at(position)
                        .is_some_and(|layer| layer.order == egui::Order::Foreground);
                if !self.focused {
                    context.memory_mut(|memory| {
                        if let Some(id) = memory.focused() {
                            memory.surrender_focus(id);
                        }
                    });
                }
            }
        }
        self.keyboard(context, &snapshot);
    }

    fn area_controls(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        if snapshot.ready
            && self
                .selected_area
                .is_none_or(|area| !snapshot.areas.contains(&area))
        {
            self.selected_area = snapshot
                .areas
                .iter()
                .copied()
                .find(|area| *area != snapshot.area)
                .or_else(|| snapshot.areas.first().copied());
        }
        ui.add_enabled_ui(snapshot.ready, |ui| {
            ui.horizontal_wrapped(|ui| {
                let label = |area| match (snapshot.map, area) {
                    (44, 245) => "营地 · 245".to_owned(),
                    (44, 246) => "树海顶部 · 246".to_owned(),
                    (97, 460) => "营地 · 460".to_owned(),
                    (97, 461) => "古迹 · 461".to_owned(),
                    _ => format!("区域 {area}"),
                };
                egui::ComboBox::from_id_salt("debug-area")
                    .height(menu_height(ui))
                    .selected_text(
                        self.selected_area
                            .map(label)
                            .unwrap_or_else(|| "目标区域".into()),
                    )
                    .show_ui(ui, |ui| {
                        for area in &snapshot.areas {
                            ui.selectable_value(&mut self.selected_area, Some(*area), label(*area));
                        }
                    });
                if ui
                    .add_enabled(
                        self.selected_area.is_some_and(|area| area != snapshot.area),
                        egui::Button::new("一键换区"),
                    )
                    .clicked()
                {
                    self.send(DebugCommand::ChangeArea(self.selected_area.unwrap()));
                }
            });
        });
    }

    fn equipment(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("debug-slot")
                .height(menu_height(ui))
                .selected_text(slot_name(self.slot))
                .show_ui(ui, |ui| {
                    for kind in [6, 0, 2, 3, 4, 5] {
                        ui.selectable_value(&mut self.slot, kind, slot_name(kind));
                    }
                });
            if self.slot == 6 {
                egui::ComboBox::from_id_salt("debug-weapon")
                    .height(menu_height(ui))
                    .selected_text(NATIVE_WEAPON_NAMES[self.weapon as usize])
                    .show_ui(ui, |ui| {
                        for (index, name) in NATIVE_WEAPON_NAMES.iter().enumerate() {
                            ui.selectable_value(&mut self.weapon, index as u8, *name);
                        }
                    });
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.filter).hint_text("按装备名称或编号筛选"));
        ui.small("选择装备后重载当前任务，刷新模型、技能与招式资源。");
        let filter = self.filter.trim().to_lowercase();
        let items = snapshot
            .catalog
            .equipment
            .iter()
            .filter(|item| {
                (if self.slot == 6 {
                    item.weapon == Some(self.weapon)
                } else {
                    item.kind == self.slot
                }) && (filter.is_empty()
                    || item.name.to_lowercase().contains(&filter)
                    || item.id.to_string().contains(&filter))
            })
            .collect::<Vec<_>>();
        ui.small(format!("{} 件", items.len()));
        egui::ScrollArea::vertical()
            .id_salt("debug-equipment-list")
            .max_height(290.0)
            .show_rows(ui, 30.0, items.len(), |ui, rows| {
                for row in rows {
                    let item = items[row];
                    ui.horizontal(|ui| {
                        let equipped = snapshot.equipment.contains(&(item.kind, item.id));
                        let button =
                            egui::Button::new(if equipped { "已装备" } else { "换装" });
                        if ui
                            .add_enabled(snapshot.ready && !equipped, button)
                            .clicked()
                        {
                            self.send(DebugCommand::Equip {
                                kind: item.kind,
                                id: item.id,
                            });
                        }
                        ui.add(egui::Label::new(format!("{} · {}", item.id, item.name)).truncate())
                            .on_hover_text(&item.name);
                    });
                }
            });
    }

    fn actions(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        if snapshot.monster.is_some() {
            ui.label("当前处于怪物形态，请在“怪物变身”页使用怪物招式，或先恢复猎人。");
            return;
        }
        ui.small("调用游戏招式状态机；编号来自当前客户端，未确认的名称保留编号。");
        let mut source = self.action_weapon.unwrap_or(snapshot.weapon).min(13);
        egui::ComboBox::from_id_salt("debug-action-source")
            .height(menu_height(ui))
            .selected_text(format!(
                "招式来源：{}",
                NATIVE_WEAPON_NAMES[source as usize]
            ))
            .show_ui(ui, |ui| {
                for (index, name) in NATIVE_WEAPON_NAMES.iter().enumerate() {
                    if ui
                        .selectable_value(&mut source, index as u8, *name)
                        .changed()
                    {
                        self.action_weapon = Some(source);
                    }
                }
            });
        ui.small("跨武器触发保留当前装备，重载任务后使用所选武器的招式资源。");
        ui.horizontal(|ui| {
            if ui
                .add_enabled(snapshot.ready, egui::Button::new("回到待机"))
                .clicked()
            {
                self.send(DebugCommand::Action(Action {
                    group: 0,
                    id: 0,
                    weapon: snapshot.weapon,
                }));
            }
            if ui
                .add_enabled(snapshot.ready, egui::Button::new("招式跟随装备"))
                .clicked()
            {
                self.action_weapon = None;
                self.send(DebugCommand::FollowEquipment);
            }
        });
        ui.add(egui::TextEdit::singleline(&mut self.action_filter).hint_text("按招式编号筛选"));
        let filter = self.action_filter.trim();
        let actions = snapshot
            .catalog
            .actions
            .get(source as usize)
            .into_iter()
            .flatten()
            .filter(|action| filter.is_empty() || action.id.to_string().contains(filter))
            .copied()
            .collect::<Vec<_>>();
        ui.small(format!("{} 个招式 · 触发后可隐藏窗口观察", actions.len()));
        egui::ScrollArea::vertical()
            .id_salt("debug-actions-list")
            .max_height(290.0)
            .show_rows(ui, 30.0, actions.len(), |ui, rows| {
                for row in rows {
                    let action = actions[row];
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(snapshot.ready, egui::Button::new("触发"))
                            .clicked()
                        {
                            self.send(DebugCommand::Action(action));
                        }
                        ui.label(action.label());
                        if action.weapon == snapshot.weapon
                            && (action.group, action.id)
                                == (snapshot.action_group, snapshot.action_id)
                        {
                            ui.strong("当前");
                        }
                    });
                }
            });
    }

    fn keyboard(&mut self, context: &Context, snapshot: &DebugSnapshot) {
        let mut movement = MonsterInput::default();
        if snapshot.controlling_monster && !self.focused && !context.egui_wants_keyboard_input() {
            context.input(|input| {
                if !input.focused {
                    return;
                }
                movement = MonsterInput {
                    forward: f32::from(u8::from(input.key_down(Key::W)))
                        - f32::from(u8::from(input.key_down(Key::S))),
                    sideways: f32::from(u8::from(input.key_down(Key::D)))
                        - f32::from(u8::from(input.key_down(Key::A))),
                    vertical: f32::from(u8::from(input.key_down(Key::E)))
                        - f32::from(u8::from(input.key_down(Key::Q))),
                    speed: self.monster_speed * if input.modifiers.shift { 3.0 } else { 1.0 },
                };
                for (slot, key) in [Key::Num1, Key::Num2, Key::Num3, Key::Num4]
                    .into_iter()
                    .enumerate()
                {
                    if snapshot.monster == Some(self.monster_species)
                        && input.key_pressed(key)
                        && let Some(action) = self.monster_shortcuts[slot]
                    {
                        self.send(DebugCommand::MonsterAction(action));
                    }
                }
                if input.key_pressed(Key::R) {
                    self.send(DebugCommand::NextMonsterAction);
                }
                if input.key_pressed(Key::Backspace) {
                    self.send(DebugCommand::RestoreHunter);
                }
            });
        }
        self.control.set_monster_input(movement);
    }

    fn monsters(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.small("从完整种类列表选择；重载当前地图并自动变身，无需场上已有该怪物。");
        ui.small("保留原任务目标，额外生成受控怪物；其他怪物会将你作为敌方目标。");
        ui.add(
            egui::TextEdit::singleline(&mut self.monster_filter)
                .hint_text("按怪物中文名或编号筛选"),
        );
        let filter = self.monster_filter.trim();
        let previous_species = self.monster_species;
        egui::ComboBox::from_id_salt("debug-monster-species")
            .width(ui.available_width().min(360.0))
            .truncate()
            .selected_text(format!(
                "{} · {}",
                self.monster_species,
                super::monsters::NAMES[self.monster_species as usize]
            ))
            .height(menu_height(ui))
            .show_ui(ui, |ui| {
                for monster in &snapshot.catalog.monsters {
                    if filter.is_empty()
                        || monster.name.contains(filter)
                        || monster.id.to_string().contains(filter)
                    {
                        ui.selectable_value(
                            &mut self.monster_species,
                            monster.id,
                            format!("{} · {}", monster.id, monster.name),
                        );
                    }
                }
            });
        if previous_species != self.monster_species {
            self.monster_shortcuts = [None; 4];
        }
        ui.horizontal(|ui| {
            if ui
                .add_enabled(snapshot.ready, egui::Button::new("变身并操控"))
                .clicked()
            {
                self.send(DebugCommand::Transform(self.monster_species));
            }
            if ui
                .add_enabled(
                    snapshot.monster.is_some() && (snapshot.ready || snapshot.scene == 5),
                    egui::Button::new("恢复猎人"),
                )
                .clicked()
            {
                self.send(DebugCommand::RestoreHunter);
            }
        });
        ui.separator();
        ui.small("直接选择招式并触发；尚未变身时会自动变身后执行。");
        ui.add(
            egui::TextEdit::singleline(&mut self.monster_action_filter)
                .hint_text("筛选招式编号，如 3:12"),
        );
        let actions = if snapshot.monster == Some(self.monster_species) {
            snapshot.monster_actions.as_slice()
        } else {
            snapshot
                .catalog
                .monsters
                .iter()
                .find(|monster| monster.id == self.monster_species)
                .map(|monster| monster.actions.as_slice())
                .unwrap_or_default()
        };
        let filter = self.monster_action_filter.trim();
        let actions = actions
            .iter()
            .copied()
            .filter(|action| {
                filter.is_empty() || format!("{}:{}", action.group, action.id).contains(filter)
            })
            .collect::<Vec<_>>();
        ui.small(format!("{} 个招式", actions.len()));
        egui::ScrollArea::vertical()
            .id_salt("debug-monster-actions")
            .max_height(230.0)
            .show_rows(ui, 30.0, actions.len(), |ui, rows| {
                for row in rows {
                    let action = actions[row];
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(snapshot.ready, egui::Button::new("触发"))
                            .clicked()
                        {
                            self.send(DebugCommand::TransformAction {
                                species: self.monster_species,
                                action,
                            });
                        }
                        ui.label(action.label());
                        ui.menu_button("绑定", |ui| {
                            for slot in 0..4 {
                                if ui.button(format!("快捷键 {}", slot + 1)).clicked() {
                                    self.monster_shortcuts[slot] = Some(action);
                                    ui.close();
                                }
                            }
                        });
                        if snapshot.monster == Some(self.monster_species)
                            && (action.group, action.id)
                                == (snapshot.action_group, snapshot.action_id)
                        {
                            ui.strong("当前");
                        }
                    });
                }
            });
        ui.collapsing("操控设置与说明", |ui| {
            ui.add(egui::Slider::new(&mut self.monster_speed, 50.0..=800.0).text("移动速度"));
            let mut distance = snapshot.camera_distance.max(300.0);
            if ui
                .add(egui::Slider::new(&mut distance, 300.0..=5000.0).text("镜头距离"))
                .changed()
            {
                self.send(DebugCommand::CameraDistance(distance));
            }
            let mut pitch = snapshot.camera_pitch;
            if ui
                .add(
                    egui::Slider::new(&mut pitch, -60.0..=80.0)
                        .text("垂直角度")
                        .suffix("°")
                        .step_by(1.0),
                )
                .changed()
            {
                self.send(DebugCommand::CameraPitch(pitch));
            }
            ui.small("垂直角度：正值俯视，0° 平视，负值仰视。");
            ui.small("点击游戏区域即可操控，调试窗口可以保持打开。");
            ui.small("W/S 跟随视角前后移动，A/D 左右移动，Q/E 升降，Shift 加速。");
            ui.small("进入出口的水平范围即可换区；站在跳崖入口上方也会触发，无需继续移动。");
            ui.small("1–4 触发绑定招式，R 原生选招，Backspace 恢复猎人。");
            ui.horizontal_wrapped(|ui| {
                for slot in 0..4 {
                    let label = self.monster_shortcuts[slot].map_or_else(
                        || "未绑定".into(),
                        |action| format!("{}:{}", action.group, action.id),
                    );
                    ui.label(format!("{} = {label}", slot + 1));
                }
            });
            if ui
                .add_enabled(
                    snapshot.controlling_monster,
                    egui::Button::new("原生选招一次 · R"),
                )
                .clicked()
            {
                self.send(DebugCommand::NextMonsterAction);
            }
            ui.small("巨型怪物、场景机关和特殊形态可能依赖专用地图；未确认的对象保留编号。");
        });
    }
}

fn menu_height(ui: &egui::Ui) -> f32 {
    let viewport = ui.ctx().content_rect().shrink(8.0);
    let y = ui
        .next_widget_position()
        .y
        .clamp(viewport.top(), viewport.bottom());
    ((y - viewport.top()).max(viewport.bottom() - y) - 32.0).clamp(24.0, 240.0)
}

fn slot_name(kind: u8) -> &'static str {
    match kind {
        0 => "头部",
        2 => "胸部",
        3 => "腕部",
        4 => "腰部",
        5 => "腿部",
        _ => "武器",
    }
}
