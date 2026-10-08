use super::{
    Action, AppearanceChange, DebugCommand, DebugControl, DebugSnapshot, NATIVE_WEAPON_NAMES,
    WeaponStyle, input::InputSettings,
};

use egui_hunter::{
    Button, ButtonKind, Field, FormLayout, Icon, LabelPlacement, NavigationState, Panel, Segment,
    SegmentedControl, SelectField, Tab, Tabs, Tokens,
};
use std::{borrow::Cow, sync::Arc};

mod action_definition;
mod hud;
mod monster_ai;

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
enum Page {
    #[default]
    Task,
    Appearance,
    Equipment,
    Transmog,
    Actions,
    Monsters,
    MonsterManagement,
    MonsterAi,
}

impl Page {
    const ALL: [Self; 8] = [
        Self::Task,
        Self::Appearance,
        Self::Equipment,
        Self::Transmog,
        Self::Actions,
        Self::Monsters,
        Self::MonsterManagement,
        Self::MonsterAi,
    ];

    fn tab(self) -> Tab<'static> {
        let (id, label) = match self {
            Self::Task => ("task", "任务"),
            Self::Appearance => ("appearance", "外观"),
            Self::Equipment => ("equipment", "装备"),
            Self::Transmog => ("transmog", "幻化"),
            Self::Actions => ("actions", "招式"),
            Self::Monsters => ("monsters", "怪物变身"),
            Self::MonsterManagement => ("monster-management", "怪物管理"),
            Self::MonsterAi => ("monster-ai", "怪物 AI"),
        };
        Tab::new(egui::Id::new(id), label)
    }
}

pub(crate) fn show_hud(
    context: &egui::Context,
    snapshot: &DebugSnapshot,
    target: Option<super::AiTarget>,
) {
    hud::show(context, snapshot);
    if let Some(target) = target {
        monster_ai::show_hud(context, snapshot, target);
    }
}

pub(crate) struct DebugPanel {
    control: Arc<DebugControl>,
    page: Page,
    weapon: u8,
    equipment_filters: [String; 6],
    equipment_popup: Option<(bool, usize)>,
    transmog_filters: [String; 5],
    action_filter: String,
    action_weapon: Option<u8>,
    monster_filter: String,
    monster_action_filter: String,
    definition_action: Option<Action>,
    ai: monster_ai::Editor,
}

impl DebugPanel {
    pub(crate) fn new(control: Arc<DebugControl>) -> Self {
        Self {
            control,
            page: Page::default(),
            weapon: 0,
            equipment_filters: Default::default(),
            equipment_popup: None,
            transmog_filters: Default::default(),
            action_filter: String::new(),
            action_weapon: None,
            monster_filter: String::new(),
            monster_action_filter: String::new(),
            definition_action: None,
            ai: monster_ai::Editor::default(),
        }
    }
    fn send(&self, command: DebugCommand) {
        let _ = self.control.send(command);
    }
    pub(crate) fn take_recording_save(&mut self) -> Option<String> {
        self.ai.debugger.take_recording_save()
    }
    pub(crate) fn recording_save_finished(&mut self, result: Result<bool, String>) {
        self.ai.debugger.recording_save_finished(result);
    }
    pub(crate) fn show(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputSettings,
    ) {
        if ui.available_width() >= 920.0 {
            egui::Panel::left("debug-navigation")
                .default_size(152.0)
                .size_range(128.0..=224.0)
                .resizable(true)
                .frame(
                    egui::Frame::new()
                        .inner_margin(ui.spacing().window_margin)
                        .fill(ui.visuals().faint_bg_color),
                )
                .show(ui, |ui| self.sidebar(ui));
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE.inner_margin(ui.spacing().window_margin))
                .show(ui, |ui| self.page_content(ui, snapshot, input));
        } else {
            let tabs = Page::ALL.map(Page::tab);
            let mut navigation = NavigationState::default();
            navigation.select(self.page.tab().id);
            Tabs::new(egui::Id::new("debug-pages")).show(ui, &mut navigation, &tabs, |ui, page| {
                self.page = Page::ALL[tabs.iter().position(|tab| tab.id == page).unwrap()];
                self.page_content(ui, snapshot, input);
            });
        }
        if let Some(action) = self.definition_action {
            let mut open = true;
            action_definition::show(ui.ctx(), snapshot, action, &mut open);
            if !open {
                self.definition_action = None;
            }
        }
    }

    fn sidebar(&mut self, ui: &mut egui::Ui) {
        for page in Page::ALL {
            match page {
                Page::Appearance => {
                    ui.add_space(12.0);
                    ui.weak("猎人");
                }
                Page::Monsters => {
                    ui.add_space(12.0);
                    ui.weak("怪物");
                }
                _ => {}
            }
            let tab = page.tab();
            if ui
                .add(
                    Button::new(tab.label)
                        .id(egui::Id::new("debug-pages").with(("header", tab.id)))
                        .selected(self.page == page)
                        .kind(ButtonKind::Quiet)
                        .full_width(),
                )
                .clicked()
            {
                self.page = page;
            }
        }
    }

    fn page_content(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputSettings,
    ) {
        let compact_monsters = self.page == Page::Monsters && ui.available_width() < 720.0;
        ui.horizontal(|ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                self.help(ui);
                if compact_monsters {
                    let settings = ui.add(Button::new("操控设置"));
                    let mut popup = egui_hunter::Popup::new(&settings)
                        .style(ui.style().clone())
                        .tokens(Tokens::get(ui));
                    popup.native = popup.native.width(320.0);
                    popup.show(|ui| {
                        egui::ScrollArea::vertical()
                            .max_height((ui.ctx().content_rect().height() - 100.0).max(80.0))
                            .show(ui, |ui| self.monster_controls(ui, snapshot, input));
                    });
                }
            });
        });
        ui.separator();
        // Lists and editor panes each own their scrolling. Only content-sized
        // forms scroll as a page, so toolbars never disappear behind a list.
        if !matches!(self.page, Page::Equipment | Page::Transmog)
            || self
                .equipment_popup
                .is_some_and(|(transmog, _)| transmog != (self.page == Page::Transmog))
        {
            self.equipment_popup = None;
        }
        match self.page {
            Page::Task | Page::Appearance => {
                egui::ScrollArea::vertical()
                    .id_salt(("debug-page-body", self.page.tab().id))
                    .auto_shrink([false, false])
                    .show(ui, |ui| match self.page {
                        Page::Task => {
                            Panel::new("").show(ui, |ui| {
                                self.summary(ui, snapshot);
                                ui.weak(format!("地图 {} · 场景 {}", snapshot.map, snapshot.scene));
                                ui.add_space(8.0);
                                ui.label("所在区域");
                                self.area_controls(ui, snapshot);
                                ui.add_space(8.0);
                                self.session_controls(ui, snapshot);
                            });
                            self.details(ui, snapshot);
                        }
                        _ => {
                            ui.set_max_width(640.0);
                            Panel::new("").show(ui, |ui| self.appearance(ui, snapshot));
                        }
                    });
            }
            Page::Equipment | Page::Transmog => {
                self.equipment_form(ui, snapshot, self.page == Page::Transmog)
            }
            Page::Actions => self.actions(ui, snapshot),
            Page::MonsterAi => self.ai.show(ui, snapshot, &self.control),
            Page::MonsterManagement => self.ai.show_management(ui, snapshot, &self.control),
            Page::Monsters => {
                if ui.available_width() >= 720.0 {
                    egui::Panel::right("debug-monster-control-panel")
                        .default_size(256.0)
                        .size_range(224.0..=360.0)
                        .resizable(true)
                        .frame(
                            egui::Frame::new()
                                .inner_margin(ui.spacing().window_margin)
                                .fill(ui.visuals().faint_bg_color),
                        )
                        .show(ui, |ui| {
                            ui.strong("操控设置");
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.monster_controls(ui, snapshot, input));
                        });
                    egui::CentralPanel::default()
                        .frame(egui::Frame::NONE.inner_margin(egui::Margin {
                            right: 12,
                            ..egui::Margin::ZERO
                        }))
                        .show(ui, |ui| self.monsters(ui, snapshot, input));
                } else {
                    self.monsters(ui, snapshot, input);
                }
            }
        }
    }

    pub(crate) fn hud_target(&self) -> Option<super::AiTarget> {
        self.ai.hud_target()
    }

    fn summary(&self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        ui.horizontal_wrapped(|ui| {
            ui.strong(format!("任务 {}", snapshot.quest_id));
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
    }

    fn details(&self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
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
    }

    fn help(&self, ui: &mut egui::Ui) {
        let help = ui.add(Button::new("帮助").kind(ButtonKind::Quiet));
        let mut popup = egui_hunter::Popup::new(&help)
            .title("使用说明")
            .style(ui.style().clone())
            .tokens(Tokens::get(ui));
        popup.native = popup
            .native
            .width(360.0_f32.min(ui.ctx().content_rect().width() - 32.0));
        popup.show(|ui| self.usage_help(ui));
    }

    fn usage_help(&self, ui: &mut egui::Ui) {
        match self.page {
            Page::Task => {
                ui.label("选择区域立即换区；重开任务重新载入当前任务，结束调试关闭本次会话。");
                ui.label("F7 显示或隐藏调试窗口，隐藏后保留草稿和选择。");
            }
            Page::Appearance => {
                ui.label("选择性别、脸型或发型后原地热替换；切换性别会同步全身装备模型。");
                ui.label("换装与换区会保留当前外观；头盔可能遮挡发型。");
            }
            Page::Equipment => {
                ui.label("按部位展开选择器，搜索名称或编号并选中装备即可原地换装；重选当前装备可重新加载。");
                ui.label("秘传书原地切换，换装与换区保留；磁斩锤仅支持极型。");
            }
            Page::Transmog => {
                ui.label("应用幻化会回到待机并替换防具外观，保留装备属性、技能与招式来源。");
                ui.label("换装、换区与切换性别会保留幻化选择；恢复原样可清除当前部位的幻化。");
                ui.label("头部需要先装备防具；卸下头盔或性别不兼容时，对应幻化暂不显示。");
            }
            Page::Actions => {
                ui.label("调用游戏招式状态机；编号来自当前客户端，未确认的名称保留编号。");
                ui.label("跨武器触发保留当前装备，重载任务后使用所选武器的招式资源。");
                ui.label("触发后可使用 F7 隐藏窗口观察。");
            }
            Page::MonsterManagement => {
                ui.label("修改种类：选择新种类后重载任务，更新选中目标的出生记录、模型和 AI；任务进度会重置，目标条件不变。");
                ui.label(
                    "仅支持能对应到任务目标出生记录的实例；动态召唤、机关和变身实例暂不支持。",
                );
                ui.label("管理页与 AI 页共用选中的实例，切换页面会保留脚本草稿。");
                ui.label("属性信息来自当前任务快照；更换实例不会修改其他怪物。");
            }
            Page::MonsterAi => {
                ui.label("选择任务中已加载的怪物实例后，会自动反编译其当前 AI。");
                ui.label(
                    "工具栏图标按执行控制、源码操作和录制分组，悬停或键盘聚焦可查看操作名称。",
                );
                ui.label(
                    "监视页显示运行字段；字段旁的断点图标在字段变化时暂停 AI，再次点击移除断点。",
                );
                ui.label("悬浮状态：跟随选中实例，显示 AI 主状态、动作、动画帧和位置；隐藏 F7 面板后仍显示，不拦截游戏输入。");
                ui.label("重新反编译：读取游戏内存，覆盖当前草稿。");
                ui.label("加载工程：读取磁盘上的地图专用或默认工程，覆盖草稿，不会立即应用。");
                ui.label("应用更改：仅修改选中实例，并从状态 0 重新开始。");
                ui.label("恢复替换前 AI：恢复首次热替换前的 AI。");
                ui.label("草稿不会自动执行或保存到文件；复制 DSL 仅复制当前文件。");
                ui.label("调试窗口可调整大小；切换文件或实例会保留各自草稿。");
                ui.label("反编译仍为部分导出，未导出的表项沿用原生。");
                ui.label("变身操控对象的自动选招会暂停，可用「原生选招」执行 AI。");
            }
            Page::Monsters => {
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

    fn equipment_form(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot, transmog: bool) {
        let fields = [
            ("debug-equipped-weapon", "武器"),
            ("debug-weapon-style", "秘传书"),
            ("debug-equipped-head", "头部"),
            ("debug-equipped-chest", "胸部"),
            ("debug-equipped-arms", "腕部"),
            ("debug-equipped-waist", "腰部"),
            ("debug-equipped-legs", "腿部"),
        ]
        .map(|(id, label)| Field::new(egui::Id::new(id)).label(label));
        egui::ScrollArea::vertical()
            .id_salt(("debug-equipment-form-scroll", transmog))
            .show(ui, |ui| {
                ui.add_enabled_ui(snapshot.ready, |ui| {
                    FormLayout::new(egui::Id::new("debug-equipment-form"))
                        .label_placement(LabelPlacement::Left)
                        .label_width(64.0)
                        .show(
                            ui,
                            if transmog { &fields[2..] } else { &fields },
                            |ui, index| {
                                let index = if transmog { index + 2 } else { index };
                                if index == 1 {
                                    self.weapon_style(ui, snapshot)
                                } else {
                                    self.equipment_slot(
                                        ui,
                                        snapshot,
                                        index.saturating_sub(1),
                                        transmog,
                                    )
                                }
                            },
                        );
                });
            });
    }

    fn weapon_style(&self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) -> egui::Response {
        let segments = WeaponStyle::ALL.map(|style| {
            Segment::new(style, style.name())
                .enabled(snapshot.monster.is_none() && style.for_weapon(snapshot.weapon) == style)
        });
        let mut selected = snapshot.weapon_style;
        let response = SegmentedControl::new(egui::Id::new("debug-weapon-style")).show(
            ui,
            &mut selected,
            &segments,
        );
        if response.changed()
            && let Some(style) = selected
        {
            self.send(DebugCommand::WeaponStyle(style));
        }
        response
    }

    fn equipment_slot(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        slot: usize,
        transmog: bool,
    ) -> egui::Response {
        let kind = [6, 2, 3, 4, 5, 0][slot];
        let current = if transmog {
            snapshot.transmogs.selected(kind).map(|id| (kind, id))
        } else {
            snapshot.equipment[slot]
        };
        let label = match current {
            None => if transmog {
                "原装备外观"
            } else {
                "未装备"
            }
            .to_owned(),
            Some((kind, id)) => snapshot
                .catalog
                .equipment
                .iter()
                .find(|item| (item.kind, item.id) == (kind, id))
                .map_or_else(|| format!("编号 {id}"), |item| item.name.clone()),
        };
        let response = ui
            .push_id(("debug-equipment-slot", transmog, slot), |ui| {
                ui.add(
                    egui::Button::new(label)
                        .right_text("⏷")
                        .truncate()
                        .min_size(egui::vec2(
                            ui.available_width(),
                            ui.spacing().interact_size.y,
                        )),
                )
            })
            .inner;
        egui_hunter::scroll_on_focus(&response);
        if response.clicked() {
            self.equipment_popup =
                (self.equipment_popup != Some((transmog, slot))).then_some((transmog, slot));
        }
        let mut open = self.equipment_popup == Some((transmog, slot)) && ui.is_enabled();
        // Keep the parent independent of egui's single memory-popup slot, which
        // the weapon ComboBox owns while its menu is open.
        let child_open = egui::Popup::is_any_open(ui.ctx());
        let style = ui.style().clone();
        egui::Popup::from_response(&response)
            .id(response.id.with("equipment-popup"))
            .open_bool(&mut open)
            .width(response.rect.width())
            .close_behavior(if child_open {
                egui::PopupCloseBehavior::IgnoreClicks
            } else {
                egui::PopupCloseBehavior::CloseOnClickOutside
            })
            .show(|ui| {
                ui.set_style(style);
                ui.set_width(response.rect.width());
                let filter = if transmog {
                    &mut self.transmog_filters[slot - 1]
                } else {
                    &mut self.equipment_filters[slot]
                };
                filter_field(ui, "筛选装备", filter, "装备名称或编号");
                if slot == 0 {
                    weapon_selector(ui, "debug-weapon", &mut self.weapon);
                }
                if transmog
                    && ui
                        .add_enabled(
                            current.is_some(),
                            Button::new("恢复原装备外观")
                                .id(egui::Id::new("debug-transmog-clear").with(kind))
                                .full_width(),
                        )
                        .clicked()
                {
                    self.send(DebugCommand::Transmog { kind, id: None });
                    ui.close();
                }
                self.equipment_list(ui, snapshot, slot, transmog);
            });
        if self.equipment_popup == Some((transmog, slot)) && !open {
            self.equipment_popup = None;
            if !ui.ctx().input(|input| input.pointer.any_click()) && response.enabled() {
                response.request_focus();
            }
        }
        response
    }

    fn equipment_list(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        index: usize,
        transmog: bool,
    ) {
        let slot = [6, 2, 3, 4, 5, 0][index];
        let (filter, list_id) = if transmog {
            (&mut self.transmog_filters[index - 1], "debug-transmog-list")
        } else {
            (&mut self.equipment_filters[index], "debug-equipment-list")
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
        egui::ScrollArea::vertical()
            .id_salt((list_id, slot))
            .max_height(240.0)
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .animated(false)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, items.len(), |ui, rows| {
                for row in rows {
                    let item = items[row];
                    let equipped = if transmog {
                        snapshot.transmogs.selected(item.kind) == Some(item.id)
                    } else {
                        snapshot.equipment.contains(&Some((item.kind, item.id)))
                    };
                    let response = ui.add_enabled(
                        snapshot.ready,
                        Button::new("")
                            .id(egui::Id::new(list_id).with((item.kind, item.id)))
                            .selected(equipped)
                            .full_width()
                            .min_size(egui::vec2(0.0, row_height)),
                    );
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::SelectableLabel,
                            response.enabled(),
                            equipped,
                            &item.name,
                        )
                    });
                    let mut rect = response.rect.shrink2(ui.spacing().button_padding);
                    rect.max.x -= ui.spacing().icon_width;
                    let painter = ui.painter_at(rect);
                    painter.text(
                        rect.left_top(),
                        egui::Align2::LEFT_TOP,
                        &item.name,
                        egui::TextStyle::Body.resolve(ui.style()),
                        ui.visuals().text_color(),
                    );
                    painter.text(
                        rect.left_bottom(),
                        egui::Align2::LEFT_BOTTOM,
                        format!(
                            "编号 {} · 模型编号 {}",
                            item.id,
                            item.model_ids[usize::from(snapshot.appearance.female)]
                        ),
                        egui::TextStyle::Small.resolve(ui.style()),
                        ui.visuals().weak_text_color(),
                    );
                    if response.clicked() {
                        if !transmog || !equipped {
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
                        ui.close();
                    }
                }
            });
    }

    fn actions(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot) {
        if snapshot.monster.is_some() {
            ui.label("当前处于怪物形态，请在“怪物变身”页使用怪物招式，或先恢复猎人。");
            return;
        }
        filter_field(ui, "筛选招式", &mut self.action_filter, "招式编号");
        let mut source = self.action_weapon.unwrap_or(snapshot.weapon).min(13);
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
        egui::ScrollArea::vertical()
            .id_salt("debug-actions-list")
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .animated(false)
            .auto_shrink([false, false])
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
                            if definition.clicked() {
                                self.definition_action = Some(action);
                                self.send(DebugCommand::InspectAction(action));
                            }
                            let trigger = ui.add_enabled(snapshot.ready, Button::new("触发"));
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
    }

    fn monsters(&mut self, ui: &mut egui::Ui, snapshot: &DebugSnapshot, input: &mut InputSettings) {
        filter_field(ui, "筛选怪物", &mut self.monster_filter, "怪物中文名或编号");
        let mut species = input.species;
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
                let mut variant = input.variant;
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
        let variant = input.variant;
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
        egui::ScrollArea::vertical()
            .id_salt("debug-monster-actions")
            .content_margin(egui::Margin {
                right: 12,
                ..egui::Margin::ZERO
            })
            .animated(false)
            .auto_shrink([false, false])
            .show_rows(ui, row_height, actions.len(), |ui, rows| {
                for row in rows {
                    let action = actions[row];
                    let current = selected
                        && (action.group, action.id) == (snapshot.action_group, snapshot.action_id);
                    ui.push_id((species, variant, action.group, action.id), |ui| {
                        result_row(ui, row_height, current, |ui| {
                            let trigger = ui.add_enabled(snapshot.ready, Button::new("触发"));
                            if trigger.clicked() {
                                self.send(DebugCommand::TransformAction {
                                    species,
                                    variant,
                                    action,
                                });
                            }
                            let binding = ui.add(Button::new("绑定").kind(ButtonKind::Quiet));
                            egui_hunter::Popup::new(&binding)
                                .style(ui.style().clone())
                                .tokens(Tokens::get(ui))
                                .show(|ui| {
                                    for slot in 0..4 {
                                        if ui
                                            .add(
                                                Button::new(&format!("快捷键 {}", slot + 1))
                                                    .kind(ButtonKind::Quiet)
                                                    .full_width(),
                                            )
                                            .clicked()
                                        {
                                            input.shortcuts[slot] = Some(action);
                                            ui.close();
                                        }
                                    }
                                });
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
    }

    fn monster_controls(
        &mut self,
        ui: &mut egui::Ui,
        snapshot: &DebugSnapshot,
        input: &mut InputSettings,
    ) {
        disclosure(ui, "镜头与移动", |ui| {
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
                .on_hover_text("正值俯视，0° 平视，负值仰视。")
                .changed()
            {
                self.send(DebugCommand::CameraPitch(pitch));
            }
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
    let response = egui::CollapsingHeader::new(label)
        .default_open(true)
        .show(ui, content);
    egui_hunter::scroll_on_focus(&response.header_response);
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
    // Native DAT class order differs from the server's WeaponType order.
    let icon = match weapon {
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
    };
    icon.paint(&ui.painter_at(response.rect), rect, egui::Color32::WHITE);
}

fn menu_height(ui: &egui::Ui) -> f32 {
    let viewport = ui.ctx().content_rect().shrink(8.0);
    let y = ui
        .next_widget_position()
        .y
        .clamp(viewport.top(), viewport.bottom());
    ((y - viewport.top()).max(viewport.bottom() - y) - 32.0).clamp(24.0, 240.0)
}

#[cfg(test)]
mod tests;
