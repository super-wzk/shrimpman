use super::*;
use crate::provider::{
    Appearance, AppearanceOptions, Catalog, Equipment, Face, Monster, MonsterAction,
};
use egui::{Event, Id, RawInput, Rect, pos2, vec2};

fn context() -> Context {
    let context = Context::default();
    egui_hunter::Theme::default().apply(&context);
    mhf_font::install(&context);
    context
}

#[test]
fn result_rows_fit_controls_and_two_lines_without_consuming_the_viewport() {
    for viewport_height in [120.0, 480.0, 1200.0] {
        let context = context();
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    pos2(0.0, 0.0),
                    vec2(430.0, viewport_height),
                )),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let height = result_row_height(ui);
                    let mut controls = Vec::new();
                    let mut labels = Vec::new();
                    let mut rows = Vec::new();
                    for selected in [false, true] {
                        let row = result_row(ui, height, selected, |ui| {
                            controls.push(ui.add(Button::new("换装")).rect);
                            ui.allocate_ui_with_layout(
                                vec2(ui.available_width(), height - 4.0),
                                egui::Layout::top_down(egui::Align::Min),
                                |ui| {
                                    ui.spacing_mut().item_spacing.y = 0.0;
                                    ui.label("猎人武器");
                                    labels.push(ui.small("编号 65535 · 模型编号 65535").rect);
                                },
                            );
                        });
                        assert!((row.rect.height() - height).abs() <= 1.0, "{row:?}");
                        rows.push(row.rect);
                    }
                    assert!(rows[0].bottom() <= rows[1].top());
                    for ((row, control), label) in rows.iter().zip(controls).zip(labels) {
                        assert!(control.height() >= ui.spacing().interact_size.y);
                        assert!(row.contains_rect(control));
                        assert!(row.contains_rect(label));
                    }
                });
            },
        );
        output.drop_without_applying_deltas();
    }
}

fn populated_snapshot(item_count: u16) -> DebugSnapshot {
    let mut catalog = Catalog {
        equipment: [6, 2]
            .into_iter()
            .flat_map(|kind| {
                (0..item_count).map(move |id| Equipment {
                    kind,
                    id,
                    model_ids: [id; 2],
                    weapon: (kind == 6).then_some(0),
                    name: format!("猎人装备 {id}"),
                })
            })
            .collect(),
        ..Default::default()
    };
    let action_count = item_count.min(200) as u8;
    catalog.actions[0] = (0..action_count)
        .map(|id| Action {
            group: 1,
            id,
            weapon: 0,
        })
        .collect();
    catalog.monsters = vec![Monster {
        id: 94,
        name: "测试怪物",
        variants: crate::provider::monsters::variants(94),
        actions: (0..action_count)
            .map(|id| MonsterAction { group: 1, id })
            .collect::<Vec<_>>()
            .into(),
    }];
    DebugSnapshot {
        quest_id: 1,
        ready: true,
        area: 245,
        map: 44,
        areas: vec![245, 246],
        equipment: [Some((6, 0)), None, None, None, None, None],
        catalog: Arc::new(catalog),
        ..Default::default()
    }
}

struct DebugUi {
    context: Context,
    window: DebugWindow,
    input: InputController,
    snapshot: DebugSnapshot,
    texts: Vec<(String, Rect)>,
}

#[test]
fn monster_species_picker_sends_only_the_selected_instance_and_preserves_failed_draft() {
    use crate::provider::{AiDocument, AiOperation, AiReply, AiTarget};
    let target = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 7,
        serial: 42,
        model: 0x2000,
        species: 4,
    };
    let mut snapshot = populated_snapshot(1);
    snapshot.ai_targets = vec![target];
    for id in 1..=15 {
        Arc::get_mut(&mut snapshot.catalog)
            .unwrap()
            .monsters
            .push(Monster {
                id,
                name: crate::provider::monsters::NAMES[usize::from(id)],
                variants: crate::provider::monsters::variants(id),
                actions: Arc::new(Vec::new()),
            });
    }
    let mut ui = DebugUi::new(snapshot);
    ui.window.page = 5;
    ui.frame(vec![]);
    let commands = ui.window.control.commands();
    let [DebugCommand::MonsterAi { request, .. }] = commands.as_slice() else {
        panic!("missing inspect")
    };
    let source = "mhf_ai 1; species 4; base native;";
    ui.snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target,
        result: Ok(AiDocument {
            descriptor: 0x3000,
            source: Some(mhf_monster::ai::dsl::Project::single(
                None,
                4,
                source.into(),
            )),
            message: "已反编译".into(),
        }),
    }));
    ui.frame(vec![]);
    ui.click("菌猪 ▾");
    ui.click("搜索名称或编号");
    let popup_id = Id::new("replacement-species").with("popup");
    let full_height = ui.context.read_response(popup_id).unwrap().rect.height();
    assert!(full_height > 230.0, "{full_height}");
    ui.frame(vec![Event::Text("94".into())]);
    for _ in 0..3 {
        ui.frame(vec![]);
    }
    for _ in 0..2 {
        ui.frame(vec![
            Event::Key {
                key: Key::Backspace,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
            Event::Key {
                key: Key::Backspace,
                physical_key: None,
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ]);
    }
    for _ in 0..3 {
        ui.frame(vec![]);
    }
    let restored_height = ui.context.read_response(popup_id).unwrap().rect.height();
    assert!(
        (restored_height - full_height).abs() < 1.0,
        "before={full_height}, after={restored_height}"
    );
    ui.frame(vec![Event::Text("94".into())]);
    ui.click("测试怪物");
    ui.click("替换并重载任务");
    let commands = ui.window.control.commands();
    let [
        DebugCommand::MonsterAi {
            request,
            target: selected,
            operation: AiOperation::ReplaceSpecies(94),
        },
    ] = commands.as_slice()
    else {
        panic!("missing species replacement")
    };
    assert_eq!(*selected, target);
    ui.snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target,
        result: Err("资源槽已满".into()),
    }));
    ui.frame(vec![]);
    ui.click("应用热替换");
    let commands = ui.window.control.commands();
    assert!(
        matches!(commands.as_slice(), [DebugCommand::MonsterAi { operation: AiOperation::Apply { source: actual, .. }, .. }]
        if actual.files[0].source == source)
    );
}

impl DebugUi {
    fn new(snapshot: DebugSnapshot) -> Self {
        Self {
            context: context(),
            window: DebugWindow::new(DebugControl::new()),
            input: InputController::default(),
            snapshot,
            texts: Vec::new(),
        }
    }

    fn frame(&mut self, events: Vec<Event>) {
        let output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(480.0, 600.0))),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show(ui, |ui| match self.window.page {
                    1 => self.window.equipment(ui, &self.snapshot, 360.0),
                    2 => self.window.transmog(ui, &self.snapshot, 360.0),
                    3 => self.window.actions(ui, &self.snapshot, 360.0),
                    5 => self
                        .window
                        .ai
                        .show(ui, &self.snapshot, &self.window.control),
                    4 => self
                        .window
                        .monsters(ui, &self.snapshot, &mut self.input, 360.0),
                    _ => self.window.appearance(ui, &self.snapshot),
                });
            },
        );
        self.texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    Rect::from_min_size(text.pos, text.galley.size()),
                )),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
    }

    fn position(&mut self, label: &str) -> egui::Pos2 {
        self.frame(vec![]);
        self.frame(vec![]);
        self.texts
            .iter()
            .find(|(text, _)| text.split(" · ").next() == Some(label))
            .unwrap_or_else(|| panic!("missing option {label:?}"))
            .1
            .center()
    }

    fn click_at(&mut self, position: egui::Pos2) {
        for pressed in [true, false] {
            self.frame(vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ]);
        }
    }

    fn click(&mut self, label: &str) {
        let position = self.position(label);
        self.click_at(position);
    }

    fn click_id(&mut self, id: Id) {
        self.frame(vec![]);
        self.frame(vec![]);
        let response = self.context.read_response(id).unwrap();
        self.click_at(response.rect.center());
    }
}

fn appearance_ui(female: bool) -> DebugUi {
    DebugUi::new(DebugSnapshot {
        ready: true,
        appearance: Appearance {
            female,
            face: if female { 4 } else { 2 },
            hair: if female { 5 } else { 3 },
        },
        catalog: Arc::new(Catalog {
            appearances: [
                AppearanceOptions {
                    faces: vec![
                        Face {
                            id: 2,
                            model_id: 70,
                        },
                        Face {
                            id: 11,
                            model_id: 80,
                        },
                    ],
                    hair: vec![3, 12],
                },
                AppearanceOptions {
                    faces: vec![
                        Face {
                            id: 4,
                            model_id: 90,
                        },
                        Face {
                            id: 21,
                            model_id: 100,
                        },
                    ],
                    hair: vec![5, 22],
                },
            ],
            ..Default::default()
        }),
        ..Default::default()
    })
}

#[test]
fn appearance_selections_send_independent_changes_from_the_snapshot_catalog() {
    for female in [false, true] {
        let mut ui = appearance_ui(female);
        let selections = if female {
            [("女", "男"), ("编号 4", "编号 21"), ("编号 5", "编号 22")]
        } else {
            [("男", "女"), ("编号 2", "编号 11"), ("编号 3", "编号 12")]
        };
        // All three selections use the same snapshot, including while the
        // gender command is still queued. Each command changes only its field.
        for (current, target) in selections {
            ui.click(current);
            ui.click(target);
        }
        assert!(matches!(
            ui.window.control.commands().as_slice(),
            [
                DebugCommand::Appearance(AppearanceChange::Gender(gender)),
                DebugCommand::Appearance(AppearanceChange::Face(face)),
                DebugCommand::Appearance(AppearanceChange::Hair(hair)),
            ] if *gender != female
                && *face == if female { 21 } else { 11 }
                && *hair == if female { 22 } else { 12 }
        ));
    }
}

#[test]
fn appearance_controls_ignore_clicks_while_not_ready_or_missing_options() {
    for (ready, fields) in [
        (false, &["男", "编号 2", "编号 3"][..]),
        (true, &["编号 2", "编号 3"][..]),
    ] {
        let mut ui = appearance_ui(false);
        ui.snapshot.ready = ready;
        if ready {
            Arc::get_mut(&mut ui.snapshot.catalog).unwrap().appearances[0] =
                AppearanceOptions::default();
        }
        for field in fields {
            ui.click(field);
            ui.frame(vec![]);
            assert!(!ui.texts.iter().any(|(text, _)| matches!(
                text.split(" · ").next(),
                Some("女" | "编号 11" | "编号 12")
            )));
        }
        assert!(ui.window.control.commands().is_empty());
    }
}

#[test]
fn appearance_menu_ignores_a_queued_selection_when_loading_starts() {
    let mut ui = appearance_ui(false);
    ui.click("编号 3");
    let position = ui.position("编号 12");
    ui.snapshot.ready = false;
    ui.click_at(position);
    assert!(ui.window.control.commands().is_empty());
}

fn transmog_ui() -> DebugUi {
    let mut ui = DebugUi::new(populated_snapshot(0));
    ui.window.page = 2;
    Arc::get_mut(&mut ui.snapshot.catalog).unwrap().equipment = [2, 3, 4, 5, 0]
        .into_iter()
        .flat_map(|kind| {
            [0, 11, 22].map(|id| Equipment {
                kind,
                id,
                model_ids: [id + 100, id + 200],
                weapon: None,
                name: format!("防具 {kind}:{id}"),
            })
        })
        .collect();
    ui
}

#[test]
fn transmog_selects_and_restores_each_armor_slot_independently() {
    for kind in [2, 3, 4, 5, 0] {
        let mut ui = transmog_ui();
        ui.window.transmog_slot = kind;
        ui.window.transmog_filter = "11".into();
        ui.snapshot.equipment[0] = Some((kind, 22));
        ui.snapshot.transmogs.armor[if kind == 2 { 3 } else { 2 }] = 90;
        ui.click_id(Id::new("debug-transmog-list").with((kind, 11_u16)));
        assert!(matches!(
            ui.window.control.commands().as_slice(),
            [DebugCommand::Transmog { kind: selected, id: Some(11) }] if *selected == kind
        ));
        ui.snapshot.transmogs.armor[usize::from(kind)] = 11;
        ui.click_id(Id::new("debug-transmog-clear"));
        assert!(matches!(
            ui.window.control.commands().as_slice(),
            [DebugCommand::Transmog { kind: selected, id: None }] if *selected == kind
        ));
    }
}

#[test]
fn transmog_reserves_zero_for_restore_and_disables_changes_when_not_ready() {
    let mut ui = transmog_ui();
    ui.frame(vec![]);
    ui.frame(vec![]);
    assert!(
        ui.context
            .read_response(Id::new("debug-transmog-list").with((2_u8, 0_u16)))
            .is_none()
    );
    ui.click_id(Id::new("debug-transmog-clear"));
    assert!(ui.window.control.commands().is_empty());

    ui.snapshot.ready = false;
    ui.snapshot.transmogs.armor[2] = 22;
    ui.click_id(Id::new("debug-transmog-list").with((2_u8, 11_u16)));
    ui.click_id(Id::new("debug-transmog-clear"));
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn equipped_items_can_reload_and_loading_state_disables_the_action() {
    let mut ui = DebugUi::new(populated_snapshot(4));
    ui.window.page = 1;
    ui.click_id(Id::new("debug-equipment-list").with((6_u8, 0_u16)));
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::Equip { kind: 6, id: 0 }]
    ));
    ui.snapshot.ready = false;
    ui.click_id(Id::new("debug-equipment-list").with((6_u8, 0_u16)));
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn equipment_and_transmog_preserve_independent_filters_and_slots() {
    let mut ui = transmog_ui();
    ui.window.slot = 3;
    ui.window.filter = "22".into();
    ui.window.transmog_filter = "11".into();
    ui.click_id(Id::new("debug-transmog-list").with((2_u8, 11_u16)));
    ui.window.page = 1;
    ui.click_id(Id::new("debug-equipment-list").with((3_u8, 22_u16)));
    ui.window.page = 2;
    ui.click_id(Id::new("debug-transmog-list").with((2_u8, 11_u16)));
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [
            DebugCommand::Transmog {
                kind: 2,
                id: Some(11)
            },
            DebugCommand::Equip { kind: 3, id: 22 },
            DebugCommand::Transmog {
                kind: 2,
                id: Some(11)
            },
        ]
    ));
    assert_eq!(ui.window.filter, "22");
    assert_eq!(ui.window.transmog_filter, "11");
}

#[test]
fn filtered_hunter_actions_trigger_the_matching_catalog_action() {
    let mut ui = DebugUi::new(populated_snapshot(4));
    ui.window.page = 3;
    ui.window.action_filter = " 2 ".into();
    ui.click("触发");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::Action(Action {
            group: 1,
            id: 2,
            weapon: 0,
        })]
    ));
}

#[test]
fn inspecting_a_filtered_move_only_requests_its_definition() {
    let mut ui = DebugUi::new(populated_snapshot(4));
    ui.window.page = 3;
    ui.window.action_filter = "2".into();
    ui.click("定义");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::InspectAction(Action {
            weapon: 0,
            group: 1,
            id: 2
        })]
    ));
}

#[test]
fn runtime_hud_stays_at_bottom_left_without_capturing_input() {
    let context = context();
    let mut window = DebugWindow::new(DebugControl::new());
    window.open = false;
    let mut input = InputController::default();
    let mut snapshot = populated_snapshot(1);
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let mut frame = |snapshot: &DebugSnapshot, events| {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                assert!(!window.show(ui.ctx(), snapshot, &mut input));
            },
        );
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    Rect::from_min_size(text.pos, text.galley.size()),
                )),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        texts
    };
    for _ in 0..3 {
        frame(&snapshot, vec![]);
    }
    let texts = frame(&snapshot, vec![]);
    let equipment = texts
        .iter()
        .find(|(text, _)| text.starts_with("装备："))
        .unwrap()
        .1;
    let position = texts
        .iter()
        .find(|(text, _)| text.starts_with("位置 "))
        .unwrap()
        .1;
    assert!(equipment.left() < 30.0 && equipment.top() > 400.0);
    assert!(screen.contains_rect(position) && position.bottom() > 550.0);
    let pointer = equipment.center();
    frame(
        &snapshot,
        vec![
            Event::PointerMoved(pointer),
            Event::PointerButton {
                pos: pointer,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(!context.egui_wants_pointer_input());
    snapshot.ready = false;
    assert!(
        !frame(&snapshot, vec![])
            .iter()
            .any(|(text, _)| text.starts_with("装备："))
    );
}

#[test]
fn definition_window_keeps_move_rows_in_place_and_captures_its_clicks() {
    use crate::provider::action_definition::{ActionDefinition, ActionStep, Definition};
    let context = context();
    let mut window = DebugWindow::new(DebugControl::new());
    window.page = 3;
    let mut input = InputController::default();
    let mut snapshot = populated_snapshot(8);
    let action = Action {
        weapon: 0,
        group: 1,
        id: 2,
    };
    let mut frame = |window: &mut DebugWindow, snapshot: &DebugSnapshot, events| {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 900.0))),
                events,
                ..Default::default()
            },
            |ui| {
                window.show(ui.ctx(), snapshot, &mut input);
            },
        );
        let texts: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some((text.galley.job.text.clone(), text.pos)),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        texts
    };
    for _ in 0..4 {
        frame(&mut window, &snapshot, vec![]);
    }
    let rows = |texts: Vec<(String, egui::Pos2)>| {
        texts
            .into_iter()
            .filter(|(text, _)| text.starts_with("武器招式 "))
            .collect::<Vec<_>>()
    };
    let before = rows(frame(&mut window, &snapshot, vec![]));
    assert!(!before.is_empty());
    snapshot.action_definition = Some(Arc::new(ActionDefinition {
        action,
        motion_style: Some(0),
        data: Ok(Definition {
            steps: (0..20).map(|_| ActionStep([3, 1405, 0, 4, 0, 1])).collect(),
            events: vec![],
        }),
    }));
    window.definition_action = Some(action);
    for _ in 0..4 {
        frame(&mut window, &snapshot, vec![]);
    }
    let texts = frame(&mut window, &snapshot, vec![]);
    let position = texts
        .iter()
        .find(|(text, _)| text.starts_with("步骤 0 ·"))
        .expect("definition is visible in its own window")
        .1
        + vec2(5.0, 5.0);
    assert_eq!(before, rows(texts));
    frame(
        &mut window,
        &snapshot,
        vec![
            Event::PointerMoved(position),
            Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::NONE,
            },
        ],
    );
    assert!(window.focused);
    window.open = false;
    let texts = frame(&mut window, &snapshot, vec![]);
    assert!(!texts.iter().any(|(text, _)| text.starts_with("步骤 0 ·")));
    assert!(!window.focused);
}

fn monster_ui() -> DebugUi {
    let mut ui = DebugUi::new(DebugSnapshot {
        ready: true,
        catalog: Arc::new(Catalog {
            monsters: [11, 21, 65, 94, 100, 146, 155]
                .into_iter()
                .map(|id| Monster {
                    id,
                    name: crate::provider::monsters::NAMES[usize::from(id)],
                    variants: crate::provider::monsters::variants(id),
                    actions: vec![MonsterAction { group: 1, id: 5 }].into(),
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    });
    ui.window.page = 4;
    ui.input.select_species(11);
    ui
}

fn select_field_id(name: &str) -> Id {
    let id = Id::new(name);
    id.with("select").with(egui::IdSalt::new(id))
}

#[test]
fn reinforced_form_selections_reach_transform_and_direct_action_commands() {
    for (species, label, variant) in [
        (11, "HC", 1),
        (11, "辿异种", 16),
        (65, "霸种", 9),
        (100, "至天", 13),
        (146, "极怪", 11),
    ] {
        let mut ui = monster_ui();
        ui.input.select_species(species);
        ui.click_id(select_field_id("debug-monster-variant"));
        ui.click(label);
        assert_eq!(ui.input.variant(), variant);
        ui.click("变身并操控");
        ui.click("触发");
        assert!(matches!(
            ui.window.control.commands().as_slice(),
            [
                DebugCommand::Transform { species: transformed_species, variant: selected },
                DebugCommand::TransformAction {
                    species: triggered_species,
                    variant: triggered,
                    action: MonsterAction { group: 1, id: 5 },
                },
            ] if *transformed_species == species && *triggered_species == species
                && *selected == variant && *triggered == variant
        ));
    }
}

#[test]
fn variant_menu_uses_the_current_species_snapshot_descriptors() {
    use crate::provider::monsters::Variant;

    let mut ui = monster_ui();
    let monsters = &mut Arc::get_mut(&mut ui.snapshot.catalog).unwrap().monsters;
    monsters.retain(|monster| matches!(monster.id, 21 | 155));
    monsters[1].variants = vec![
        Variant {
            id: 0,
            name: "普通",
            model_suffix: "",
        },
        Variant {
            id: 12,
            name: "目录专用形态",
            model_suffix: "",
        },
    ];
    ui.input.select_species(21);
    ui.click_id(select_field_id("debug-monster-variant"));
    ui.click("彼岸岛联动");
    assert_eq!(ui.input.variant(), 12);
    ui.click("变身并操控");

    ui.click_id(select_field_id("debug-monster-species"));
    ui.click(crate::provider::monsters::NAMES[155]);
    assert_eq!(ui.input.species(), 155);
    assert_eq!(ui.input.variant(), 0);
    ui.click_id(select_field_id("debug-monster-variant"));
    let option = ui.position("目录专用形态");
    assert!(
        !ui.texts
            .iter()
            .any(|(text, _)| text.starts_with("彼岸岛联动"))
    );
    ui.click_at(option);
    assert_eq!(ui.input.variant(), 12);
    ui.click("触发");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [
            DebugCommand::Transform {
                species: 21,
                variant: 12
            },
            DebugCommand::TransformAction {
                species: 155,
                variant: 12,
                action: MonsterAction { group: 1, id: 5 },
            },
        ]
    ));
}

#[test]
fn changing_form_or_species_clears_bindings_and_restores_the_default_form() {
    let mut ui = monster_ui();
    ui.input.shortcuts[0] = Some(MonsterAction { group: 1, id: 5 });
    ui.click_id(select_field_id("debug-monster-variant"));
    ui.click("HC");
    assert!(ui.input.shortcuts.iter().all(Option::is_none));
    ui.click("绑定");
    ui.click("快捷键 1");
    assert!(ui.input.shortcuts[0] == Some(MonsterAction { group: 1, id: 5 }));
    ui.click_id(select_field_id("debug-monster-species"));
    ui.click(crate::provider::monsters::NAMES[94]);
    assert_eq!(ui.input.species(), 94);
    assert_eq!(ui.input.variant(), 0);
    assert!(ui.input.shortcuts.iter().all(Option::is_none));
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn runtime_actions_are_used_only_for_the_matching_species_and_form() {
    let mut ui = monster_ui();
    ui.snapshot.monster = Some(11);
    ui.snapshot.monster_actions = Some(vec![MonsterAction { group: 3, id: 77 }].into());
    ui.click_id(select_field_id("debug-monster-variant"));
    ui.click("HC");
    ui.click("触发");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::TransformAction {
            species: 11,
            variant: 1,
            action: MonsterAction { group: 1, id: 5 },
        }]
    ));
    ui.snapshot.monster_variant = 1;
    ui.click("触发");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::TransformAction {
            species: 11,
            variant: 1,
            action: MonsterAction { group: 3, id: 77 },
        }]
    ));
}

#[test]
fn filtered_runtime_monster_actions_trigger_and_bind_the_matching_action() {
    let mut ui = monster_ui();
    let action = MonsterAction { group: 3, id: 77 };
    ui.input.select_variant(1);
    ui.snapshot.monster = Some(11);
    ui.snapshot.monster_variant = 1;
    ui.snapshot.controlling_monster = true;
    ui.snapshot.monster_actions = Some(vec![MonsterAction { group: 1, id: 77 }, action].into());
    ui.window.monster_action_filter = " 3:77 ".into();
    ui.click("触发");
    ui.click("绑定");
    ui.click("快捷键 2");
    assert!(matches!(
        ui.window.control.commands().as_slice(),
        [DebugCommand::TransformAction {
            species: 11,
            variant: 1,
            action: selected,
        }] if *selected == action
    ));
    assert!(ui.input.shortcuts == [None, Some(action), None, None]);
}

#[test]
fn each_page_keeps_session_controls_above_tabs_in_a_short_window() {
    verify_session_controls_accessibility(false);
}

#[test]
fn tab_navigation_reaches_and_activates_session_controls_on_every_page() {
    verify_session_controls_accessibility(true);
}

fn verify_session_controls_accessibility(navigate_with_tabs: bool) {
    for height in [380.0, 900.0] {
        for page in 0..6 {
            let context = context();
            let control = DebugControl::new();
            let mut window = DebugWindow::new(control.clone());
            window.page = page;
            let mut input = InputController::default();
            // Keep a large catalog for programmatic focus and geometry. For
            // the full Tab route, use a list longer than the visible viewport.
            let snapshot = populated_snapshot(if navigate_with_tabs { 8 } else { 2000 });
            let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(480.0, height));
            let mut time = 0.0;
            let mut frame = |events: Vec<Event>, focus_session_controls: bool| {
                let output = context.run_ui(
                    RawInput {
                        screen_rect: Some(screen),
                        time: Some(time),
                        events,
                        focused: true,
                        ..Default::default()
                    },
                    |ui| {
                        if focus_session_controls {
                            // Focus requests belong inside the egui pass so
                            // gained_focus can observe the transition.
                            ui.memory_mut(|memory| memory.request_focus(Id::new("debug-exit")));
                        }
                        window.show(ui.ctx(), &snapshot, &mut input);
                    },
                );
                let visible_equipment = output.shapes.iter().filter(|clipped| {
                    matches!(&clipped.shape, egui::Shape::Text(text)
                        if text.galley.job.text.starts_with("猎人装备 ")
                            && clipped.clip_rect.contains_rect(clipped.shape.visual_bounding_rect()))
                }).count();
                output.drop_without_applying_deltas();
                time += 0.2;
                visible_equipment
            };
            frame(vec![], false);
            let visible_equipment = frame(vec![], false);
            let tab = context
                .read_response(Id::new("debug-pages").with(("header", Id::new("equipment"))))
                .unwrap();
            let controls = context.read_response(Id::new("debug-exit")).unwrap();
            assert!(
                controls.rect.bottom() < tab.rect.top(),
                "session controls must stay above tabs: {controls:?}, {tab:?}"
            );
            assert!(
                tab.rect.top() < 210.0,
                "compact header is too tall: {tab:?}"
            );
            if height == 900.0 && matches!(page, 1 | 2) {
                assert!(
                    visible_equipment >= 6,
                    "only {visible_equipment} equipment rows are fully visible"
                );
            }
            if navigate_with_tabs {
                for _ in 0..80 {
                    frame(
                        [true, false]
                            .map(|pressed| Event::Key {
                                key: Key::Tab,
                                physical_key: None,
                                pressed,
                                repeat: false,
                                modifiers: Modifiers::NONE,
                            })
                            .into(),
                        false,
                    );
                    for _ in 0..4 {
                        frame(vec![], false);
                    }
                    if let Some(focused) = context
                        .memory(|memory| memory.focused())
                        .and_then(|id| context.read_response(id))
                    {
                        assert!(
                            screen.contains_rect(focused.rect)
                                && focused.interact_rect.height() >= focused.rect.height() - 1.0
                                && focused.interact_rect.width() >= focused.rect.width() - 1.0,
                            "Tab focus is clipped on page {page}, height {height}: {focused:?}"
                        );
                    }
                    if context.memory(|memory| memory.focused()) == Some(Id::new("debug-exit")) {
                        break;
                    }
                }
            } else {
                frame(vec![], true);
            }
            for _ in 0..8 {
                frame(vec![], false);
            }
            let exit = context.read_response(Id::new("debug-exit")).unwrap();
            assert!(exit.has_focus(), "page {page}, height {height}");
            assert!(
                screen.contains_rect(exit.rect),
                "page {page}, height {height}: {exit:?}"
            );
            assert!(
                exit.interact_rect.height() >= exit.rect.height() - 1.0
                    && exit.interact_rect.width() >= exit.rect.width() - 1.0,
                "session controls are clipped on page {page}, height {height}: {exit:?}"
            );
            frame(
                vec![Event::Key {
                    key: Key::Enter,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
                false,
            );
            assert!(matches!(
                control.commands().as_slice(),
                [DebugCommand::Exit]
            ));
        }
    }
}

#[test]
fn monster_ai_auto_inspects_preserves_failed_draft_and_rejects_reused_instance() {
    use crate::provider::{AiDocument, AiOperation, AiReply, AiTarget};
    let target = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 7,
        serial: 12,
        model: 0x2000,
        species: 6,
    };
    let source =
        "mhf_ai 1; species 6; base native; states { idle { self.action(3:6, 0); restart; } }";
    let mut ui = DebugUi::new(DebugSnapshot {
        ready: true,
        ai_targets: vec![target],
        ..Default::default()
    });
    ui.window.page = 5;
    ui.frame(vec![]);
    let commands = ui.window.control.commands();
    let [
        DebugCommand::MonsterAi {
            request,
            target: selected,
            operation: AiOperation::Inspect,
        },
    ] = commands.as_slice()
    else {
        panic!("missing inspection");
    };
    assert_eq!(*selected, target);
    ui.click("应用热替换");
    assert!(ui.window.control.commands().is_empty());
    ui.snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target,
        result: Ok(AiDocument {
            descriptor: 0x3000,
            source: Some(mhf_monster::ai::dsl::Project::single(
                Some(0),
                target.species,
                source.into(),
            )),
            message: "已反编译".into(),
        }),
    }));
    ui.frame(vec![]);
    ui.click("应用热替换");
    let commands = ui.window.control.commands();
    let [
        DebugCommand::MonsterAi {
            request,
            operation:
                AiOperation::Apply {
                    descriptor,
                    source: actual,
                },
            ..
        },
    ] = commands.as_slice()
    else {
        panic!("missing apply");
    };
    assert_eq!(*descriptor, 0x3000);
    assert_eq!(actual.files[0].source, source);
    ui.snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target,
        result: Err("测试编译失败".into()),
    }));
    ui.frame(vec![]);
    ui.click("应用热替换");
    let commands = ui.window.control.commands();
    assert!(
        matches!(commands.as_slice(), [DebugCommand::MonsterAi { operation: AiOperation::Apply { source: actual, .. }, .. }] if actual.files[0].source == source)
    );
    ui.snapshot.ai_targets[0].serial += 1;
    ui.frame(vec![]);
    ui.click("应用热替换");
    assert!(ui.window.control.commands().is_empty());
}
