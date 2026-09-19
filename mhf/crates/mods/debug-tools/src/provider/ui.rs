use super::{
    Action, AppearanceChange, DebugCommand, DebugControl, DebugSnapshot, NATIVE_WEAPON_NAMES,
    input::InputController,
};
use egui::{Context, Key, Modifiers};
use egui_hunter::{
    Button, ButtonKind, Field, FormLayout, Icon, LabelPlacement, NavigationState, SelectField, Tab,
    Tabs, Tokens,
};
use std::{borrow::Cow, sync::Arc};

mod action_definition;
mod hud;
mod monster_ai;

pub(crate) struct DebugWindow {
    control: Arc<DebugControl>,
    open: bool,
    page: usize,
    weapon: u8,
    slot: u8,
    filter: String,
    transmog_slot: u8,
    transmog_filter: String,
    action_filter: String,
    action_weapon: Option<u8>,
    monster_filter: String,
    focused: bool,
    monster_action_filter: String,
    definition_action: Option<Action>,
    ai: monster_ai::Editor,
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
            transmog_slot: 2,
            transmog_filter: String::new(),
            action_filter: String::new(),
            action_weapon: None,
            monster_filter: String::new(),
            focused: true,
            monster_action_filter: String::new(),
            definition_action: None,
            ai: monster_ai::Editor::default(),
        }
    }
    fn send(&self, command: DebugCommand) {
        let _ = self.control.send(command);
    }
    pub(crate) fn show(
        &mut self,
        context: &Context,
        snapshot: &DebugSnapshot,
        input: &mut InputController,
    ) -> bool {
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
        hud::show(context, snapshot);
        let mut open = self.open;
        let viewport = context.content_rect();
        let max_width = (viewport.width() - 32.0).max(80.0);
        let max_height = (viewport.height() - 72.0).max(48.0);
        let window = egui::Window::new(egui::RichText::new("任务调试 · F7").size(16.0))
            .id(egui::Id::new("quest-debugger"))
            .title_frame(
                egui::Frame::window(&context.global_style())
                    .inner_margin(egui::Margin::symmetric(12, 6)),
            )
            .open(&mut open)
            .default_pos(viewport.min + egui::vec2(16.0, 16.0))
            .default_width(430.0_f32.min(max_width))
            .min_width(280.0_f32.min(max_width))
            .max_width(max_width)
            .default_height(620.0_f32.min(max_height))
            .min_height(280.0_f32.min(max_height))
            .max_height(max_height)
            .constrain_to(viewport.shrink(8.0))
            .vscroll(max_height < 280.0)
            .show(context, |ui| {
                ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
                self.summary(ui, snapshot);
                self.session_controls(ui, snapshot);
                self.area_controls(ui, snapshot);
                let tabs = [
                    Tab::new(egui::Id::new("appearance"), "外观"),
                    Tab::new(egui::Id::new("equipment"), "装备"),
                    Tab::new(egui::Id::new("transmog"), "幻化"),
                    Tab::new(egui::Id::new("actions"), "招式"),
                    Tab::new(egui::Id::new("monsters"), "怪物变身"),
                    Tab::new(egui::Id::new("monster-ai"), "怪物"),
                ];
                let mut navigation = NavigationState::default();
                navigation.select(tabs[self.page].id);
                Tabs::new(egui::Id::new("debug-pages")).show(
                    ui,
                    &mut navigation,
                    &tabs,
                    |ui, page| {
                        self.page = tabs.iter().position(|tab| tab.id == page).unwrap_or(0);
                        let body_height = ui.available_height().max(48.0);
                        // Keep navigation fixed. The body scroll remains available when
                        // the game viewport is short or a diagnostic section is expanded.
                        egui::ScrollArea::vertical()
                            .id_salt("debug-page-body")
                            .content_margin(egui::Margin {
                                right: 12,
                                ..egui::Margin::ZERO
                            })
                            .auto_shrink([false, false])
                            .max_height(body_height)
                            .show(ui, |ui| {
                                let list_height = (body_height - 128.0).clamp(120.0, 360.0);
                                match self.page {
                                    0 => self.appearance(ui, snapshot),
                                    1 => self.equipment(ui, snapshot, list_height),
                                    2 => self.transmog(ui, snapshot, list_height),
                                    3 => self.actions(ui, snapshot, list_height),
                                    5 => self.ai.show(ui, snapshot, &self.control),
                                    _ => self.monsters(ui, snapshot, input, list_height),
                                }
                                ui.add_space(4.0);
                                self.details(ui, snapshot, input);
                            });
                    },
                );
            });
        self.open = open;
        let definition_rect = if self.open {
            self.definition_action.and_then(|action| {
                let mut open = true;
                let position = window.as_ref().map_or(viewport.min, |window| {
                    window.response.rect.right_top() + egui::vec2(8.0, 0.0)
                });
                let rect = action_definition::show(context, snapshot, action, &mut open, position);
                if !open {
                    self.definition_action = None;
                }
                rect
            })
        } else {
            None
        };
        let ai_rect = if self.open {
            self.ai.show_window(context, snapshot, &self.control)
        } else {
            None
        };
        self.ai.show_hud(context, snapshot);
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
                    || definition_rect.is_some_and(|rect| rect.contains(position))
                    || ai_rect.is_some_and(|rect| rect.contains(position))
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
        self.focused
    }

    fn summary(&self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.scope(|ui| {
            ui.spacing_mut().interact_size.y = 24.0;
            ui.horizontal_wrapped(|ui| {
                ui.strong(format!("任务 {}", snapshot.quest_id));
                let help = ui.add(Button::new("使用说明"));
                let mut popup = egui_hunter::Popup::new(&help)
                    .title("使用说明")
                    .style(ui.style().clone())
                    .tokens(Tokens::get(ui));
                popup.native = popup
                    .native
                    .width(360.0_f32.min(ui.ctx().content_rect().width() - 32.0));
                popup.show(|ui| self.usage_help(ui));
                let tokens = Tokens::get(ui);
                egui::Frame::new()
                    .fill(if snapshot.ready {
                        tokens.success_fill
                    } else {
                        ui.visuals().extreme_bg_color
                    })
                    .corner_radius(6)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(if snapshot.ready {
                                "可调试"
                            } else {
                                "加载 / 结算中"
                            })
                            .small()
                            .color(if snapshot.ready {
                                tokens.success
                            } else {
                                ui.visuals().weak_text_color()
                            }),
                        );
                    });
            });
        });
    }

    fn details(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputController,
    ) {
        if snapshot.controlling_monster {
            disclosure(ui, "交战详情", |ui| {
                ui.label(format!(
                    "命中检查 {} · 确认命中 {}",
                    snapshot.combat.checks, snapshot.combat.hits
                ));
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
                if !snapshot.combat.last_damage.is_empty() {
                    ui.add(egui::Label::new(&snapshot.combat.last_damage).wrap());
                }
            });
        }
        if self.page == 4 {
            self.monster_controls(ui, snapshot, input);
        }
    }

    fn usage_help(&self, ui: &mut egui::Ui) {
        match self.page {
            0 => {
                ui.label("选择性别、脸型或发型后原地热替换；切换性别会同步全身装备模型。");
                ui.label("换装与换区会保留当前外观；头盔可能遮挡发型。");
            }
            1 => {
                ui.label("选择装备后原地热替换，刷新模型、技能与招式资源。");
            }
            2 => {
                ui.label("应用幻化会回到待机并替换防具外观，保留装备属性、技能与招式来源。");
                ui.label("换装、换区与切换性别会保留幻化选择；恢复原样可清除当前部位的幻化。");
                ui.label("头部需要先装备防具；卸下头盔或性别不兼容时，对应幻化暂不显示。");
            }
            3 => {
                ui.label("调用游戏招式状态机；编号来自当前客户端，未确认的名称保留编号。");
                ui.label("跨武器触发保留当前装备，重载任务后使用所选武器的招式资源。");
                ui.label("触发后可使用 F7 隐藏窗口观察。");
            }
            5 => {
                ui.label("修改种类：选择新种类后重载任务，更新选中目标的出生记录、模型和 AI；任务进度会重置，目标条件不变。");
                ui.label(
                    "仅支持能对应到任务目标出生记录的实例；动态召唤、机关和变身实例暂不支持。",
                );
                ui.label("选择任务中已加载的怪物实例后，会自动反编译其当前 AI。");
                ui.label("悬浮状态：跟随选中实例，显示 AI 主状态、动作、动画帧和位置；隐藏 F7 面板后仍显示，不拦截游戏输入。");
                ui.label("重新反编译：读取游戏内存，覆盖当前草稿。");
                ui.label("加载工程：读取磁盘上的地图专用或默认工程，覆盖草稿，不会立即应用。");
                ui.label("应用热替换：仅修改选中实例，并从状态 0 重新开始。");
                ui.label("恢复替换前 AI：恢复首次热替换前的 AI。");
                ui.label("草稿不会自动执行或保存到文件；复制 DSL 仅复制当前文件。");
                ui.label("独立窗口可调整大小；切换文件或实例会保留各自草稿。");
                ui.label("反编译仍为部分导出，未导出的表项沿用原生。");
                ui.label("变身操控对象的自动选招会暂停，可用「原生选招」执行 AI。");
            }
            _ => {
                ui.label("变种仍受当前任务设定影响；任务内同种怪物共用所选变种。");
                ui.label("从完整种类列表选择；重载当前地图并自动变身，无需场上已有该怪物。");
                ui.label("保留原任务目标，额外生成受控怪物；其他怪物会将你作为敌方目标。");
                ui.label("直接选择招式并触发；尚未变身时会自动变身后执行。");
                ui.label("点击游戏区域即可操控，调试窗口可以保持打开。");
                ui.label("W/S 跟随视角前后移动，A/D 左右移动，Q/E 升降，Shift 加速。");
                ui.label("进入出口的水平范围即可换区；站在跳崖入口上方也会触发，无需继续移动。");
                ui.label("1–4 触发绑定招式，R 原生选招，Backspace 恢复猎人。");
                ui.label("巨型怪物、场景机关和特殊形态可能依赖专用地图；未确认的对象保留编号。");
            }
        }
    }

    fn session_controls(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.horizontal_wrapped(|ui| {
            let restart = ui.add_enabled(
                snapshot.ready || snapshot.scene == 5,
                Button::new("重开任务")
                    .id(egui::Id::new("debug-restart"))
                    .kind(ButtonKind::Quiet),
            );
            if restart.clicked() {
                self.send(DebugCommand::Restart);
            }
            let exit = ui.add(
                Button::new("结束调试")
                    .id(egui::Id::new("debug-exit"))
                    .kind(ButtonKind::DangerQuiet),
            );
            if exit.clicked() {
                self.send(DebugCommand::Exit);
            }
            ui.small("F7 显示 / 隐藏");
        });
    }

    fn area_controls(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        let label = |area| match (snapshot.map, area) {
            (44, 245) => "营地 · 245".to_owned(),
            (44, 246) => "树海顶部 · 246".to_owned(),
            (97, 460) => "营地 · 460".to_owned(),
            (97, 461) => "古迹 · 461".to_owned(),
            _ => format!("区域 {area}"),
        };
        ui.add_enabled_ui(snapshot.ready && !snapshot.areas.is_empty(), |ui| {
            let mut field = SelectField::new(egui::Id::new("debug-area"), label(snapshot.area));
            field.native = field
                .native
                .width(ui.available_width())
                .height(menu_height(ui));
            field.show_ui(ui, |ui| {
                for &area in &snapshot.areas {
                    if ui
                        .selectable_label(area == snapshot.area, label(area))
                        .clicked()
                    {
                        if area != snapshot.area {
                            self.send(DebugCommand::ChangeArea(area));
                        }
                        ui.close();
                    }
                }
            });
        });
    }

    fn appearance(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        let appearance = snapshot.appearance;
        let options = &snapshot.catalog.appearances[usize::from(appearance.female)];
        let ids = ["debug-gender", "debug-face", "debug-hair"].map(egui::Id::new);
        let fields = [
            Field::new(ids[0]).label("性别"),
            Field::new(ids[1]).label("脸型"),
            Field::new(ids[2]).label("发型"),
        ];
        ui.add_enabled_ui(snapshot.ready, |ui| {
            FormLayout::new(egui::Id::new("debug-appearance-form"))
                .label_placement(LabelPlacement::Left)
                .label_width(64.0)
                .show(ui, &fields, |ui, index| {
                    let (mut selected, count) = match index {
                        0 => (u8::from(appearance.female), 2),
                        1 => (appearance.face, options.faces.len()),
                        _ => (appearance.hair, options.hair.len()),
                    };
                    let label = |value| match (index, value) {
                        (0, 0) => "男".into(),
                        (0, _) => "女".into(),
                        (1, _) => {
                            let model = options
                                .faces
                                .iter()
                                .find(|face| face.id == value)
                                .map_or_else(|| "未知".into(), |face| face.model_id.to_string());
                            format!("编号 {value} · 模型编号 {model}")
                        }
                        _ => format!("编号 {value} · 模型编号 {value}"),
                    };
                    ui.add_enabled_ui(count != 0, |ui| {
                        let mut field = SelectField::new(ids[index], label(selected));
                        field.native = field.native.height(menu_height(ui));
                        field
                            .show_ui(ui, |ui| {
                                for choice in 0..count {
                                    let value = match index {
                                        0 => choice as u8,
                                        1 => options.faces[choice].id,
                                        _ => options.hair[choice],
                                    };
                                    let option =
                                        ui.selectable_value(&mut selected, value, label(value));
                                    if option.changed() {
                                        self.send(DebugCommand::Appearance(match index {
                                            0 => AppearanceChange::Gender(value != 0),
                                            1 => AppearanceChange::Face(value),
                                            _ => AppearanceChange::Hair(value),
                                        }));
                                    }
                                    if option.clicked() {
                                        ui.close();
                                    }
                                }
                            })
                            .response
                    })
                    .inner
                });
        });
        ui.add_space(4.0);
    }

    fn equipment(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot, list_height: f32) {
        filter_field(ui, "筛选装备", &mut self.filter, "装备名称或编号");
        let id = egui::Id::new("debug-slot");
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                let mut field = SelectField::new(id, slot_name(self.slot));
                field.native = field.native.height(menu_height(ui)).width(80.0);
                let slot = field.show_ui(ui, |ui| {
                    for kind in [6, 2, 3, 4, 5, 0] {
                        if ui
                            .selectable_value(&mut self.slot, kind, slot_name(kind))
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                if self.slot == 6 {
                    weapon_selector(ui, "debug-weapon", &mut self.weapon);
                }
                slot.response
            })
            .inner
        });
        self.equipment_list(ui, snapshot, list_height, false);
    }

    fn transmog(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot, list_height: f32) {
        filter_field(ui, "筛选幻化", &mut self.transmog_filter, "防具名称或编号");
        let id = egui::Id::new("debug-transmog-slot");
        ui.vertical(|ui| {
            let mut field = SelectField::new(id, slot_name(self.transmog_slot));
            field.native = field.native.height(menu_height(ui)).width(80.0);
            field
                .show_ui(ui, |ui| {
                    for kind in [2, 3, 4, 5, 0] {
                        if ui
                            .selectable_value(&mut self.transmog_slot, kind, slot_name(kind))
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                })
                .response
        });
        let current = snapshot.transmogs.selected(self.transmog_slot);
        ui.horizontal_wrapped(|ui| {
            let label = match current {
                None => "当前幻化：原装备外观".to_owned(),
                Some(id) => match snapshot
                    .catalog
                    .equipment
                    .iter()
                    .find(|item| item.kind == self.transmog_slot && item.id == id)
                {
                    Some(item) => format!(
                        "当前幻化：{} · 编号 {id} · 模型编号 {}",
                        item.name,
                        item.model_ids[usize::from(snapshot.appearance.female)]
                    ),
                    None => format!("当前幻化：编号 {id}"),
                },
            };
            ui.add(egui::Label::new(label).wrap());
            if ui
                .add_enabled(
                    snapshot.ready && current.is_some(),
                    Button::new("恢复原样")
                        .id(egui::Id::new("debug-transmog-clear"))
                        .kind(ButtonKind::Quiet),
                )
                .clicked()
            {
                self.send(DebugCommand::Transmog {
                    kind: self.transmog_slot,
                    id: None,
                });
            }
        });
        self.equipment_list(ui, snapshot, list_height, true);
    }

    fn equipment_list(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        list_height: f32,
        transmog: bool,
    ) {
        let (slot, filter, list_id) = if transmog {
            (
                self.transmog_slot,
                &mut self.transmog_filter,
                "debug-transmog-list",
            )
        } else {
            (self.slot, &mut self.filter, "debug-equipment-list")
        };
        let query = filter.trim().to_lowercase();
        let items = snapshot
            .catalog
            .equipment
            .iter()
            .filter(|item| {
                (!transmog || item.id != 0)
                    && (if slot == 6 {
                        item.weapon == Some(self.weapon)
                    } else {
                        item.kind == slot
                    })
                    && (query.is_empty()
                        || item.name.to_lowercase().contains(&query)
                        || item.id.to_string().contains(&query))
            })
            .collect::<Vec<_>>();
        ui.label(
            egui::RichText::new(format!("{} 件装备", items.len()))
                .small()
                .weak(),
        );
        if items.is_empty() {
            empty_results(ui, "没有匹配的装备", filter);
            return;
        }
        let row_height = result_row_height(ui);
        let previous_offset = list_offset(ui, list_id);
        let mut focused_row = None;
        let list = egui::ScrollArea::vertical()
            .id_salt(list_id)
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .max_height(list_height)
            .animated(false)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, items.len(), |ui, rows| {
                for row in rows {
                    let item = items[row];
                    let equipped = if transmog {
                        snapshot.transmogs.selected(item.kind) == Some(item.id)
                    } else {
                        snapshot.equipment.contains(&Some((item.kind, item.id)))
                    };
                    ui.push_id((item.kind, item.id), |ui| {
                        result_row(ui, row_height, equipped, |ui| {
                            if equipped && transmog {
                                ui.label(
                                    egui::RichText::new("已幻化")
                                        .small()
                                        .color(Tokens::get(ui).primary),
                                );
                            } else {
                                let label = if transmog {
                                    "幻化"
                                } else if equipped {
                                    "重新加载"
                                } else {
                                    "换装"
                                };
                                let equip = ui.add_enabled(
                                    snapshot.ready,
                                    Button::new(label)
                                        .id(egui::Id::new(list_id).with((item.kind, item.id))),
                                );
                                if equip.gained_focus() {
                                    focused_row = Some(equip.rect);
                                }
                                if equip.clicked() {
                                    self.send(if transmog {
                                        DebugCommand::Transmog {
                                            kind: item.kind,
                                            id: Some(item.id),
                                        }
                                    } else {
                                        DebugCommand::Equip {
                                            kind: item.kind,
                                            id: item.id,
                                        }
                                    });
                                }
                            }
                            ui.allocate_ui_with_layout(
                                egui::vec2(ui.available_width(), row_height - 4.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.add(egui::Label::new(&item.name).truncate())
                                        .on_hover_text(&item.name);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "编号 {} · 模型编号 {}",
                                            item.id,
                                            item.model_ids[usize::from(snapshot.appearance.female)]
                                        ))
                                        .small()
                                        .weak(),
                                    );
                                },
                            );
                        });
                    });
                }
            });
        reveal_result(ui, focused_row, previous_offset - list.state.offset.y);
    }

    fn actions(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot, list_height: f32) {
        if snapshot.monster.is_some() {
            ui.label("当前处于怪物形态，请在“怪物变身”页使用怪物招式，或先恢复猎人。");
            return;
        }
        filter_field(ui, "筛选招式", &mut self.action_filter, "招式编号");
        let mut source = self.action_weapon.unwrap_or(snapshot.weapon).min(13);
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                let selection = weapon_selector(ui, "debug-action-source", &mut source);
                if selection.changed() {
                    self.action_weapon = Some(source);
                }
                if ui
                    .add_enabled(
                        snapshot.ready,
                        Button::new("回到待机").kind(ButtonKind::Quiet),
                    )
                    .clicked()
                {
                    self.send(DebugCommand::Action(Action {
                        group: 0,
                        id: 0,
                        weapon: snapshot.weapon,
                    }));
                }
                if ui
                    .add_enabled(
                        snapshot.ready,
                        Button::new("跟随装备").kind(ButtonKind::Quiet),
                    )
                    .clicked()
                {
                    self.action_weapon = None;
                    self.send(DebugCommand::FollowEquipment);
                }
                selection
            })
            .inner
        });
        let filter = self.action_filter.trim();
        let actions = snapshot
            .catalog
            .actions
            .get(source as usize)
            .map_or(&[][..], Vec::as_slice);
        let actions = if filter.is_empty() {
            Cow::Borrowed(actions)
        } else {
            Cow::Owned(
                actions
                    .iter()
                    .copied()
                    .filter(|action| action.id.to_string().contains(filter))
                    .collect::<Vec<_>>(),
            )
        };
        ui.label(
            egui::RichText::new(format!("{} 个招式", actions.len()))
                .small()
                .weak(),
        );
        if actions.is_empty() {
            empty_results(ui, "没有匹配的招式", &mut self.action_filter);
            return;
        }
        let row_height = result_row_height(ui);
        let previous_offset = list_offset(ui, "debug-actions-list");
        let mut focused_row = None;
        let list = egui::ScrollArea::vertical()
            .id_salt("debug-actions-list")
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .max_height(list_height)
            .animated(false)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, actions.len(), |ui, rows| {
                for row in rows {
                    let action = actions[row];
                    let current = action.weapon == snapshot.weapon
                        && (action.group, action.id) == (snapshot.action_group, snapshot.action_id);
                    ui.push_id((action.weapon, action.group, action.id), |ui| {
                        result_row(ui, row_height, current, |ui| {
                            let definition = ui.add_enabled(
                                snapshot.ready,
                                Button::new("定义").kind(ButtonKind::Quiet),
                            );
                            if definition.gained_focus() {
                                focused_row = Some(definition.rect);
                            }
                            if definition.clicked() {
                                self.definition_action = Some(action);
                                self.send(DebugCommand::InspectAction(action));
                            }
                            let trigger = ui.add_enabled(snapshot.ready, Button::new("触发"));
                            if trigger.gained_focus() {
                                focused_row = Some(trigger.rect);
                            }
                            if trigger.clicked() {
                                self.send(DebugCommand::Action(action));
                            }
                            if current {
                                ui.label(
                                    egui::RichText::new("当前")
                                        .small()
                                        .color(Tokens::get(ui).primary),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let label = action.label();
                                    ui.add(egui::Label::new(&label).truncate())
                                        .on_hover_text(label);
                                },
                            );
                        });
                    });
                }
            });
        reveal_result(ui, focused_row, previous_offset - list.state.offset.y);
    }

    fn monsters(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputController,
        list_height: f32,
    ) {
        filter_field(ui, "筛选怪物", &mut self.monster_filter, "怪物中文名或编号");
        let mut species = input.species();
        let ids = ["debug-monster-species", "debug-monster-variant"].map(egui::Id::new);
        ui.vertical(|ui| {
            let width = ui.available_width();
            let gap = ui.spacing().item_spacing.x;
            let species_width = ((width - gap) * 0.4).clamp(100.0, 180.0);
            let variant_width = (width - gap - species_width).clamp(150.0, 250.0);
            ui.horizontal_wrapped(|ui| {
                let filter = self.monster_filter.trim();
                let mut field = SelectField::new(ids[0], super::monsters::NAMES[species as usize]);
                field.native = field.native.width(species_width).height(menu_height(ui));
                field.show_ui(ui, |ui| {
                    for monster in &snapshot.catalog.monsters {
                        if !filter.is_empty()
                            && !monster.name.contains(filter)
                            && !monster.id.to_string().contains(filter)
                        {
                            continue;
                        }
                        if ui
                            .selectable_value(&mut species, monster.id, monster.name)
                            .clicked()
                        {
                            ui.close();
                        }
                    }
                });
                input.select_species(species);
                let mut variant = input.variant();
                let monster = snapshot
                    .catalog
                    .monsters
                    .iter()
                    .find(|monster| monster.id == species);
                let label = |variant: super::monsters::Variant| {
                    format!(
                        "{} · 变种{} · 模型{species:03}{}",
                        variant.name, variant.id, variant.model_suffix,
                    )
                };
                let selected = monster.and_then(|monster| monster.variant(variant));
                let response = ui
                    .add_enabled_ui(
                        monster.is_some_and(|monster| !monster.variants.is_empty()),
                        |ui| {
                            let mut field = SelectField::new(
                                ids[1],
                                selected.map_or_else(|| "无可用变种".into(), label),
                            );
                            field.native =
                                field.native.width(variant_width).height(menu_height(ui));
                            field
                                .show_ui(ui, |ui| {
                                    if let Some(monster) = monster {
                                        for &choice in &monster.variants {
                                            if ui
                                                .selectable_value(
                                                    &mut variant,
                                                    choice.id,
                                                    label(choice),
                                                )
                                                .clicked()
                                            {
                                                ui.close();
                                            }
                                        }
                                    }
                                })
                                .response
                        },
                    )
                    .inner;
                input.select_variant(variant);
                if ui
                    .add_enabled(
                        snapshot.ready
                            && monster
                                .and_then(|monster| monster.variant(variant))
                                .is_some(),
                        Button::new("变身并操控"),
                    )
                    .clicked()
                {
                    self.send(DebugCommand::Transform { species, variant });
                }
                if ui
                    .add_enabled(
                        snapshot.monster.is_some() && (snapshot.ready || snapshot.scene == 5),
                        Button::new("恢复猎人").kind(ButtonKind::Quiet),
                    )
                    .clicked()
                {
                    self.send(DebugCommand::RestoreHunter);
                }
                response
            })
            .inner
        });
        let variant = input.variant();
        filter_field(
            ui,
            "招式筛选",
            &mut self.monster_action_filter,
            "招式编号，如 3:12",
        );
        let selected = snapshot.monster == Some(species) && snapshot.monster_variant == variant;
        let actions = if selected {
            snapshot
                .monster_actions
                .as_deref()
                .map_or(&[][..], Vec::as_slice)
        } else {
            snapshot
                .catalog
                .monsters
                .iter()
                .find(|monster| monster.id == species)
                .map(|monster| monster.actions.as_slice())
                .unwrap_or_default()
        };
        let filter = self.monster_action_filter.trim();
        let actions = if filter.is_empty() {
            Cow::Borrowed(actions)
        } else {
            Cow::Owned(
                actions
                    .iter()
                    .copied()
                    .filter(|action| format!("{}:{}", action.group, action.id).contains(filter))
                    .collect::<Vec<_>>(),
            )
        };
        ui.label(
            egui::RichText::new(format!("{} 个招式", actions.len()))
                .small()
                .weak(),
        );
        if actions.is_empty() {
            empty_results(ui, "没有匹配的怪物招式", &mut self.monster_action_filter);
            return;
        }
        let row_height = result_row_height(ui);
        let previous_offset = list_offset(ui, "debug-monster-actions");
        let mut focused_row = None;
        let list = egui::ScrollArea::vertical()
            .id_salt("debug-monster-actions")
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .max_height(list_height)
            .animated(false)
            .auto_shrink([false, true])
            .show_rows(ui, row_height, actions.len(), |ui, rows| {
                for row in rows {
                    let action = actions[row];
                    let current = selected
                        && (action.group, action.id) == (snapshot.action_group, snapshot.action_id);
                    ui.push_id((species, variant, action.group, action.id), |ui| {
                        result_row(ui, row_height, current, |ui| {
                            let trigger = ui.add_enabled(snapshot.ready, Button::new("触发"));
                            if trigger.gained_focus() {
                                focused_row = Some(trigger.rect);
                            }
                            if trigger.clicked() {
                                self.send(DebugCommand::TransformAction {
                                    species,
                                    variant,
                                    action,
                                });
                            }
                            let binding = ui.menu_button("绑定", |ui| {
                                for slot in 0..4 {
                                    if ui.button(format!("快捷键 {}", slot + 1)).clicked() {
                                        input.shortcuts[slot] = Some(action);
                                        ui.close();
                                    }
                                }
                            });
                            egui_hunter::scroll_on_focus(&binding.response);
                            if binding.response.gained_focus() {
                                focused_row = Some(binding.response.rect);
                            }
                            if current {
                                ui.label(
                                    egui::RichText::new("当前")
                                        .small()
                                        .color(Tokens::get(ui).primary),
                                );
                            }
                            ui.with_layout(
                                egui::Layout::left_to_right(egui::Align::Center),
                                |ui| {
                                    let label = action.label();
                                    ui.add(egui::Label::new(&label).truncate())
                                        .on_hover_text(label);
                                },
                            );
                        });
                    });
                }
            });
        reveal_result(ui, focused_row, previous_offset - list.state.offset.y);
    }

    fn monster_controls(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputController,
    ) {
        disclosure(ui, "镜头与移动", |ui| {
            ui.scope(|ui| {
                ui.spacing_mut().interact_size.y = 24.0;
                let widgets = &mut ui.visuals_mut().widgets;
                for visuals in [
                    &mut widgets.inactive,
                    &mut widgets.hovered,
                    &mut widgets.active,
                    &mut widgets.open,
                ] {
                    visuals.corner_radius = egui::CornerRadius::same(4);
                }
                ui.add(egui::Slider::new(&mut input.speed, 50.0..=800.0).text("移动速度"));
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
            });
            ui.small("垂直角度：正值俯视，0° 平视，负值仰视。");
        });
        disclosure(ui, "快捷招式", |ui| {
            ui.horizontal_wrapped(|ui| {
                for slot in 0..4 {
                    let label = input.shortcuts[slot].map_or_else(
                        || "未绑定".into(),
                        |action| format!("{}:{}", action.group, action.id),
                    );
                    ui.label(format!("{} = {label}", slot + 1));
                }
            });
            if ui
                .add_enabled(
                    snapshot.controlling_monster,
                    Button::new("原生选招一次 · R"),
                )
                .clicked()
            {
                self.send(DebugCommand::NextMonsterAction);
            }
        });
    }
}

fn filter_field(ui: &mut egui::Ui, id: &str, value: &mut String, placeholder: &str) {
    ui.add(
        egui_hunter::TextField::new(ui.make_persistent_id(id), value)
            .hint(placeholder)
            .icon(Icon::Search),
    );
}

fn disclosure(ui: &mut egui::Ui, label: &str, content: impl FnOnce(&mut egui::Ui)) {
    let response = ui.collapsing(label, content);
    egui_hunter::scroll_on_focus(&response.header_response);
}

fn list_offset(ui: &egui::Ui, id: &str) -> f32 {
    egui::scroll_area::State::load(ui.ctx(), ui.make_persistent_id(egui::IdSalt::new(id)))
        .map_or(0.0, |state| state.offset.y)
}

fn reveal_result(ui: &mut egui::Ui, focused: Option<egui::Rect>, applied_scroll: f32) {
    if let Some(rect) = focused
        && !ui.input(|input| input.pointer.any_pressed() || input.pointer.any_click())
    {
        // The inner list consumed the focus request. Reveal the same control
        // in the body, accounting for the scroll the inner list just applied.
        ui.scroll_to_rect(rect.translate(egui::vec2(0.0, applied_scroll)), None);
    }
}

fn empty_results(ui: &mut egui::Ui, message: &str, filter: &mut String) {
    ui.add_space(12.0);
    ui.label(message);
    if !filter.is_empty()
        && ui
            .add(Button::new("清除筛选").kind(ButtonKind::Quiet))
            .clicked()
    {
        filter.clear();
    }
    ui.add_space(12.0);
}

fn result_row_height(ui: &egui::Ui) -> f32 {
    // Virtual row estimates must include both the real control height and
    // the two-line equipment label, with the same padding as result_row.
    ui.spacing().interact_size.y.max(
        ui.text_style_height(&egui::TextStyle::Body)
            + ui.text_style_height(&egui::TextStyle::Small),
    ) + 4.0
}

fn result_row(
    ui: &mut egui::Ui,
    height: f32,
    selected: bool,
    content: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    egui::Frame::new()
        .fill(if selected {
            ui.visuals().selection.bg_fill
        } else {
            egui::Color32::TRANSPARENT
        })
        .corner_radius(ui.visuals().widgets.inactive.corner_radius)
        .inner_margin(egui::Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), height - 4.0),
                egui::Layout::right_to_left(egui::Align::Center),
                content,
            );
        })
        .response
}

fn weapon_selector(ui: &mut egui::Ui, id: &str, weapon: &mut u8) -> egui::Response {
    let selected = *weapon;
    let mut changed = false;
    let mut field = SelectField::new(egui::Id::new(id), weapon_label(ui, selected));
    field.native = field.native.width(138.0).height(menu_height(ui));
    let mut selector = field.show_ui(ui, |ui| {
        for index in 0..NATIVE_WEAPON_NAMES.len() {
            let row = ui.selectable_value(weapon, index as u8, weapon_label(ui, index as u8));
            paint_weapon_label_icon(ui, &row, index as u8);
            changed |= row.changed();
            if row.clicked() {
                ui.close();
            }
        }
    });
    paint_weapon_label_icon(ui, &selector.response, selected);
    if changed {
        selector.response.mark_changed();
    }
    selector.response
}

fn weapon_label(ui: &egui::Ui, weapon: u8) -> egui::text::LayoutJob {
    let mut label = egui::text::LayoutJob::default();
    label.append(
        NATIVE_WEAPON_NAMES[weapon as usize],
        26.0,
        egui::TextFormat {
            font_id: egui::TextStyle::Button.resolve(ui.style()),
            color: egui::Color32::PLACEHOLDER,
            ..Default::default()
        },
    );
    label
}

fn paint_weapon_label_icon(ui: &egui::Ui, response: &egui::Response, weapon: u8) {
    let rect = egui::Rect::from_center_size(
        egui::pos2(
            response.rect.left() + ui.spacing().button_padding.x + 10.0,
            response.rect.center().y,
        ),
        egui::Vec2::splat(20.0),
    );
    native_weapon_icon(weapon).paint(&ui.painter_at(response.rect), rect, egui::Color32::WHITE);
}

fn native_weapon_icon(weapon: u8) -> Icon {
    // Native DAT class order differs from the server's WeaponType order.
    match weapon {
        0 => Icon::GreatSword,
        1 => Icon::HeavyBowgun,
        2 => Icon::Hammer,
        3 => Icon::Lance,
        4 => Icon::SwordAndShield,
        5 => Icon::LightBowgun,
        6 => Icon::DualBlades,
        7 => Icon::LongSword,
        8 => Icon::HuntingHorn,
        9 => Icon::Gunlance,
        10 => Icon::Bow,
        11 => Icon::Tonfa,
        12 => Icon::SwitchAxe,
        _ => Icon::MagnetSpike,
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
        2 => "头部",
        3 => "胸部",
        4 => "腕部",
        5 => "腰部",
        0 => "腿部",
        _ => "武器",
    }
}

#[cfg(test)]
mod tests;
