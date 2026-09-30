use super::*;
use crate::provider::{
    Appearance, AppearanceOptions, Catalog, Equipment, Face, Monster, MonsterAction,
};
use egui::{Context, Event, Id, Key, Modifiers, RawInput, Rect, pos2, vec2};

fn context() -> Context {
    let context = Context::default();
    egui_hunter::Theme::default()
        .density(egui_hunter::Density::Compact)
        .apply(&context);
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
    window: DebugPanel,
    input: InputSettings,
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
    ui.window.page = Page::MonsterAi;
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
        }),
    }));
    ui.frame(vec![]);
    ui.window.page = Page::MonsterManagement;
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
    ui.window.page = Page::MonsterAi;
    ui.click_id(Id::new("ai-apply"));
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
            window: DebugPanel {
                page: Page::Appearance,
                ..DebugPanel::new(DebugControl::new())
            },
            input: InputSettings::default(),
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
                    Page::Equipment => self.window.equipment(ui, &self.snapshot),
                    Page::Transmog => self.window.transmog(ui, &self.snapshot),
                    Page::Actions => self.window.actions(ui, &self.snapshot),
                    Page::MonsterAi => {
                        self.window
                            .ai
                            .show(ui, &self.snapshot, &self.window.control)
                    }
                    Page::MonsterManagement => {
                        self.window
                            .ai
                            .show_management(ui, &self.snapshot, &self.window.control)
                    }
                    Page::Monsters => self.window.monsters(ui, &self.snapshot, &mut self.input),
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

struct PanelUi {
    context: Context,
    window: DebugPanel,
    input: InputSettings,
    snapshot: DebugSnapshot,
    size: egui::Vec2,
    time: f64,
    texts: Vec<(String, Rect, Rect)>,
    copied: Vec<String>,
}

impl PanelUi {
    fn new(snapshot: DebugSnapshot, size: egui::Vec2, page: Page) -> Self {
        Self {
            context: context(),
            window: DebugPanel {
                page,
                ..DebugPanel::new(DebugControl::new())
            },
            input: InputSettings::default(),
            snapshot,
            size,
            time: 0.0,
            texts: Vec::new(),
            copied: Vec::new(),
        }
    }

    fn frame(&mut self, events: Vec<Event>) {
        let output = self.context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
                time: Some(self.time),
                focused: true,
                events,
                ..Default::default()
            },
            |ui| self.window.show(ui, &self.snapshot, &mut self.input),
        );
        self.copied
            .extend(
                output
                    .platform_output
                    .commands
                    .iter()
                    .filter_map(|command| match command {
                        egui::OutputCommand::CopyText(text) => Some(text.clone()),
                        _ => None,
                    }),
            );
        self.texts = output
            .shapes
            .iter()
            .filter_map(|clipped| match &clipped.shape {
                egui::Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    Rect::from_min_size(text.pos, text.galley.size()),
                    clipped.clip_rect,
                )),
                _ => None,
            })
            .collect();
        output.drop_without_applying_deltas();
        self.time += 0.1;
    }

    fn settle(&mut self) {
        for _ in 0..4 {
            self.frame(Vec::new());
        }
    }

    fn visible_text(&self, label: &str) -> Rect {
        self.texts
            .iter()
            .find(|(text, rect, clip)| text == label && clip.contains_rect(*rect))
            .unwrap_or_else(|| {
                panic!(
                    "missing fully visible text {label:?} at {:?}: {:?}",
                    self.size,
                    self.texts
                        .iter()
                        .filter(|(text, _, _)| text == label)
                        .collect::<Vec<_>>()
                )
            })
            .1
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
        self.settle();
    }

    fn select_page(&mut self, page: Page) {
        self.settle();
        let id = Id::new("debug-pages").with(("header", page.tab().id));
        let response = self.context.read_response(id).unwrap();
        assert!(Rect::from_min_size(pos2(0.0, 0.0), self.size).contains_rect(response.rect));
        self.click_at(response.rect.center());
        assert_eq!(self.window.page, page);
    }

    fn drag(&mut self, from: egui::Pos2, to: egui::Pos2) {
        self.frame(vec![Event::PointerMoved(from)]);
        self.frame(vec![Event::PointerButton {
            pos: from,
            button: egui::PointerButton::Primary,
            pressed: true,
            modifiers: Modifiers::NONE,
        }]);
        self.frame(vec![Event::PointerMoved(from.lerp(to, 0.5))]);
        self.frame(vec![Event::PointerMoved(to)]);
        self.frame(vec![Event::PointerButton {
            pos: to,
            button: egui::PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::NONE,
        }]);
        self.settle();
    }
}

#[test]
fn responsive_navigation_reaches_every_page_without_queuing_game_operations() {
    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0), vec2(1280.0, 900.0)] {
        let mut ui = PanelUi::new(populated_snapshot(20), size, Page::Task);
        for page in Page::ALL {
            ui.select_page(page);
            assert!(
                ui.window.control.commands().is_empty(),
                "navigation is local"
            );
            if page != Page::Task {
                assert!(ui.context.read_response(Id::new("debug-restart")).is_none());
            }
        }
        ui.select_page(Page::Task);
        let restart = ui.context.read_response(Id::new("debug-restart")).unwrap();
        assert!(Rect::from_min_size(pos2(0.0, 0.0), size).contains_rect(restart.rect));
        ui.click_at(restart.rect.center());
        assert!(matches!(
            ui.window.control.commands().as_slice(),
            [DebugCommand::Restart]
        ));
    }
}

#[test]
fn scrolling_lists_keeps_tools_fixed_and_keyboard_can_activate_a_bottom_row() {
    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0), vec2(1280.0, 900.0)] {
        let mut ui = PanelUi::new(populated_snapshot(1000), size, Page::Equipment);
        ui.settle();
        let filter = ui.visible_text("装备名称或编号");
        let slot = ui
            .context
            .read_response(select_field_id("debug-slot"))
            .unwrap()
            .rect;
        let first = ui.visible_text("猎人装备 0");
        ui.frame(vec![
            Event::PointerMoved(first.center()),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, -100_000.0),
                modifiers: Modifiers::NONE,
            },
        ]);
        ui.settle();
        assert_eq!(ui.visible_text("装备名称或编号"), filter);
        assert_eq!(
            ui.context
                .read_response(select_field_id("debug-slot"))
                .unwrap()
                .rect,
            slot
        );
        ui.visible_text("猎人装备 999");

        ui.click_at(filter.center());
        let mut row_focused = false;
        for _ in 0..16 {
            ui.frame(
                [true, false]
                    .map(|pressed| Event::Key {
                        key: Key::Tab,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers: Modifiers::NONE,
                    })
                    .into(),
            );
            ui.settle();
            if let Some(focused) = ui
                .context
                .memory(|memory| memory.focused())
                .and_then(|id| ui.context.read_response(id))
                && ui.texts.iter().any(|(text, rect, clip)| {
                    text == "换装" && clip.contains_rect(*rect) && focused.rect.contains_rect(*rect)
                })
            {
                assert!(Rect::from_min_size(pos2(0.0, 0.0), size).contains_rect(focused.rect));
                row_focused = true;
                break;
            }
        }
        assert!(
            row_focused,
            "Tab did not reach the visible bottom rows at {size:?}"
        );
        ui.frame(vec![Event::Key {
            key: Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        assert!(
            matches!(ui.window.control.commands().as_slice(), [DebugCommand::Equip { kind: 6, id }] if *id >= 950),
            "keyboard activation must use a visible row at the end of the list"
        );
    }
}

#[test]
fn wide_navigation_and_monster_controls_resize_with_native_handles() {
    let mut ui = PanelUi::new(populated_snapshot(20), vec2(1280.0, 900.0), Page::Equipment);
    ui.settle();
    let navigation = Id::new("debug-navigation");
    let before = egui::containers::panel::PanelState::load(&ui.context, navigation)
        .unwrap()
        .size()
        .x;
    let handle = ui
        .context
        .read_response(navigation.with("__resize"))
        .unwrap()
        .rect
        .center();
    ui.drag(handle, handle + vec2(48.0, 0.0));
    let after = egui::containers::panel::PanelState::load(&ui.context, navigation)
        .unwrap()
        .size()
        .x;
    assert!(
        after > before + 20.0,
        "navigation resize did not persist: {before} -> {after}"
    );
    ui.select_page(Page::Monsters);
    let controls = Id::new("debug-monster-control-panel");
    let before = egui::containers::panel::PanelState::load(&ui.context, controls)
        .unwrap()
        .size()
        .x;
    let handle = ui
        .context
        .read_response(controls.with("__resize"))
        .unwrap()
        .rect
        .center();
    ui.drag(handle, handle - vec2(48.0, 0.0));
    let after = egui::containers::panel::PanelState::load(&ui.context, controls)
        .unwrap()
        .size()
        .x;
    assert!(
        after > before + 20.0,
        "monster control resize did not persist: {before} -> {after}"
    );
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn definition_window_resizes_constrains_and_scrolls_while_keeping_its_summary() {
    use crate::provider::action_definition::ActionDefinition;
    use mhf_resource::action_definition::{ActionEvent, ActionStep, Definition};
    let action = Action {
        weapon: 11,
        group: 1,
        id: 2,
    };
    let mut snapshot = populated_snapshot(20);
    snapshot.action_definition = Some(Arc::new(ActionDefinition {
        action,
        motion_style: Some(3),
        attacks: Some(Arc::new(Err("mhfsdt.bin 测试目标尚未解析".into()))),
        data: Ok(Definition {
            weapon: action.weapon,
            action: u16::from(action.id),
            offset: 0,
            steps_range: 24..24 + 60 * 12,
            events_range: 24 + 60 * 12..24 + 61 * 12,
            steps: (0..60)
                .map(|_| ActionStep([4, 1405, 65535, 4, 20, 1]))
                .collect(),
            events: vec![ActionEvent {
                step: 59,
                timing: 2,
                phase: 0,
                frame: 20,
                count: 1,
                operation: 4,
                argument: 12345,
            }],
        }),
    }));
    let mut ui = PanelUi::new(snapshot, vec2(1280.0, 900.0), Page::Actions);
    ui.window.definition_action = Some(action);
    ui.context
        .all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    ui.settle();
    let id = Id::new("debug-action-definition");
    let before = egui::AreaState::load(&ui.context, id).unwrap().rect();
    ui.drag(
        before.right_bottom() - vec2(2.0, 2.0),
        before.right_bottom() + vec2(100.0, 80.0),
    );
    let resized = egui::AreaState::load(&ui.context, id).unwrap().rect();
    assert!(resized.width() > before.width() + 50.0 && resized.height() > before.height() + 30.0);
    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0)] {
        ui.size = size;
        ui.settle();
        let rect = egui::AreaState::load(&ui.context, id).unwrap().rect();
        assert!(
            Rect::from_min_size(pos2(0.0, 0.0), size)
                .shrink(7.0)
                .contains_rect(rect),
            "{rect:?} at {size:?}"
        );
        let summary = ui.visible_text("60 个步骤 · 1 个事件");
        ui.frame(vec![
            Event::PointerMoved(rect.center()),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, -100_000.0),
                modifiers: Modifiers::NONE,
            },
        ]);
        ui.settle();
        assert_eq!(ui.visible_text("60 个步骤 · 1 个事件"), summary);
        ui.visible_text("motion/w11goku.mot#4/5");
        for (text, rect, clip) in &ui.texts {
            if text.starts_with("motion/w11goku.mot") || text.contains("12345") {
                assert!(
                    rect.right() <= clip.right() + 1.0 && rect.left() >= clip.left() - 1.0,
                    "detail is horizontally clipped: {text}: {rect:?}, {clip:?}"
                );
            }
        }
        let event = ui.visible_text("步骤结束后 → 生成攻击");
        ui.frame(vec![Event::PointerMoved(event.center())]);
        ui.settle();
        for detail in [
            "mhfdat.bin#389/11/2/events/0",
            "原始操作 4 · 参数 12345",
            "阶段 0 · 帧条件 20 · 计数 1",
            "数据层偏移 0x2e8..0x2f4 · 12 字节",
        ] {
            assert!(
                ui.texts.iter().any(|(text, _, _)| text == detail),
                "missing event detail {detail}"
            );
        }
        // The narrow stacked fields can make one complete step taller than
        // the viewport. Its final event and header remain separately reachable.
        ui.frame(vec![Event::PointerGone]);
        ui.settle();
        ui.frame(vec![
            Event::PointerMoved(rect.center()),
            Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                phase: egui::TouchPhase::Move,
                delta: vec2(0.0, 100.0),
                modifiers: Modifiers::NONE,
            },
        ]);
        ui.settle();
        ui.visible_text("步骤 59 · 播放动画");
        assert_eq!(ui.visible_text("60 个步骤 · 1 个事件"), summary);
    }
}

#[test]
fn definition_attack_and_motion_share_canonical_rows_and_copy_exact_targets() {
    use crate::provider::action_definition::ActionDefinition;
    use mhf_resource::{
        action_definition::{ActionEvent, ActionStep, AttackDirectory, Definition},
        sdt,
    };
    let mut bytes = vec![0; 0x100 + 24 * sdt::ATTACK_STRIDE];
    for (index, kind) in [999_u16, 100, 0].into_iter().enumerate() {
        let offset = index * sdt::DIRECTORY_STRIDE;
        bytes[offset + 2..offset + 4].copy_from_slice(&kind.to_le_bytes());
        if kind == 0 {
            bytes[offset + 4..offset + 6].copy_from_slice(&24_u16.to_le_bytes());
            bytes[offset + 8..offset + 12].copy_from_slice(&0x100_u32.to_le_bytes());
        }
    }
    let end = 3 * sdt::DIRECTORY_STRIDE;
    bytes[end + 2..end + 4].copy_from_slice(&u16::MAX.to_le_bytes());
    let file = sdt::Sdt::parse(&bytes).unwrap();
    let directory = AttackDirectory::from_sdt("mhfsdt.bin", &file).unwrap();
    let action = Action {
        weapon: 0,
        group: 1,
        id: 2,
    };
    let event = ActionEvent {
        step: 0,
        timing: 1,
        phase: 0,
        frame: 0,
        count: 0,
        operation: 16,
        argument: 23,
    };
    let mut snapshot = populated_snapshot(4);
    snapshot.action_definition = Some(Arc::new(ActionDefinition {
        action,
        motion_style: Some(0),
        attacks: Some(Arc::new(Ok(directory))),
        data: Ok(Definition {
            weapon: action.weapon,
            action: u16::from(action.id),
            offset: 0,
            steps_range: 24..36,
            events_range: 36..48,
            steps: vec![ActionStep([3, 1405, 0, 4, 0, 1])],
            events: vec![event],
        }),
    }));
    let mut ui = PanelUi::new(snapshot, vec2(1280.0, 900.0), Page::Actions);
    ui.window.definition_action = Some(action);
    for zoom in [1.0, 1.5] {
        ui.context.set_zoom_factor(zoom);
        ui.settle();
        ui.visible_text("生成攻击");
        for (label, value) in [
            ("动画资源", "motion/w00.mot#4/5"),
            ("动画参数", "0, 4"),
            ("等待条件", "等待计数 1"),
            ("攻击资源", "mhfsdt.bin#2/attacks/23"),
        ] {
            let label = ui.visible_text(label);
            let value = ui.visible_text(value);
            assert!(
                (label.center().y - value.center().y).abs() <= 1.0,
                "{zoom}: field label/value are misaligned: {label:?}, {value:?}"
            );
            assert!(value.left() >= label.right() + 8.0);
        }
        for (kind, expected) in [
            ("motion", "motion/w00.mot#4/5"),
            ("attack", "mhfsdt.bin#2/attacks/23"),
        ] {
            let id = Id::new((
                format!("debug-action-{kind}-resource"),
                action.group,
                action.weapon,
                action.id,
                0_usize,
            ))
            .with("copy");
            let copy = ui.context.read_response(id).unwrap();
            assert!(copy.enabled());
            assert!(ui.visible_text(expected).right() <= copy.rect.left());
            ui.click_at(copy.rect.center());
            assert_eq!(ui.copied.last().map(String::as_str), Some(expected));
        }
    }
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn definition_references_keep_unknown_targets_and_original_event_sources() {
    use crate::provider::action_definition::ActionDefinition;
    use mhf_resource::action_definition::{ActionEvent, ActionStep, Definition};
    let action = Action {
        weapon: 11,
        group: 1,
        id: 2,
    };
    let mut snapshot = populated_snapshot(4);
    snapshot.action_definition = Some(Arc::new(ActionDefinition {
        action,
        motion_style: None,
        attacks: Some(Arc::new(Err("mhfsdt.bin 测试文件缺失".into()))),
        data: Ok(Definition {
            weapon: action.weapon,
            action: u16::from(action.id),
            offset: 0x100,
            steps_range: 0x180..0x18c,
            events_range: 0x200..0x218,
            steps: vec![ActionStep([4, 1405, 0xffff, 0x8000, 12, 2])],
            // Display grouping puts event 1 before event 0, without changing
            // their original indices or the byte spans in their source paths.
            events: vec![
                ActionEvent {
                    step: 59,
                    timing: 255,
                    phase: -128,
                    frame: 65533,
                    count: 65532,
                    operation: 65535,
                    argument: 65534,
                },
                ActionEvent {
                    step: 0,
                    timing: 2,
                    phase: -2,
                    frame: 17,
                    count: 9,
                    operation: 4,
                    argument: 12345,
                },
            ],
        }),
    }));
    let mut ui = PanelUi::new(snapshot, vec2(1280.0, 900.0), Page::Actions);
    ui.window.definition_action = Some(action);
    ui.context
        .all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    ui.settle();
    assert!(
        ui.texts
            .iter()
            .any(|(text, _, _)| text.contains("未解析") && text.contains("1405")),
        "an unknown weapon style retains the raw motion selector"
    );
    assert!(
        ui.texts
            .iter()
            .any(|(text, _, _)| text.contains("未解析") && text.contains("12345")),
        "an attack query without SDT data remains unresolved"
    );
    assert!(
        !ui.texts
            .iter()
            .any(|(text, _, _)| { text.contains("motion/w11") || text.contains("mhfsdt.bin#") })
    );
    let (_, target, clip) = ui
        .texts
        .iter()
        .find(|(text, _, _)| text.contains("12345") && text.contains("未解析"))
        .unwrap();
    ui.frame(vec![Event::PointerMoved(target.intersect(*clip).center())]);
    ui.settle();
    assert!(
        ui.texts
            .iter()
            .any(|(text, _, _)| text == "mhfsdt.bin 测试文件缺失"),
        "an unresolved resource exposes its concrete source error in hover"
    );
    for (label, details) in [
        (
            "1 个步骤 · 2 个事件",
            &["mhfdat.bin#389/11/2", "数据层偏移 0x100..0x118 · 24 字节"][..],
        ),
        (
            "步骤 0 · 播放动画",
            &[
                "mhfdat.bin#389/11/2/steps/0",
                "数据层偏移 0x180..0x18c · 12 字节",
            ][..],
        ),
        (
            "步骤结束后 → 生成攻击",
            &[
                "mhfdat.bin#389/11/2/events/1",
                "数据层偏移 0x20c..0x218 · 12 字节",
                "所属步骤 0 · 时机 2",
                "阶段 -2 · 帧条件 17 · 计数 9",
                "原始操作 4 · 参数 12345",
            ][..],
        ),
        (
            "步骤 59 · 帧条件 65533 · 计数 65532 → 操作 65535 · 参数 65534",
            &[
                "mhfdat.bin#389/11/2/events/0",
                "数据层偏移 0x200..0x20c · 12 字节",
                "所属步骤 59 · 时机 255",
                "阶段 -128 · 帧条件 65533 · 计数 65532",
                "原始操作 65535 · 参数 65534",
            ][..],
        ),
    ] {
        ui.frame(vec![Event::PointerGone]);
        ui.settle();
        let event = ui.visible_text(label);
        ui.frame(vec![Event::PointerMoved(event.center())]);
        ui.settle();
        for detail in details {
            assert!(
                ui.texts.iter().any(|(text, _, _)| text == *detail),
                "missing definition source or event detail {detail}"
            );
        }
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
    ui.window.page = Page::Transmog;
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
    ui.window.page = Page::Equipment;
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
    ui.window.page = Page::Equipment;
    ui.click_id(Id::new("debug-equipment-list").with((3_u8, 22_u16)));
    ui.window.page = Page::Transmog;
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
    ui.window.page = Page::Actions;
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
    ui.window.page = Page::Actions;
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
    let mut snapshot = populated_snapshot(1);
    let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0));
    let frame = |snapshot: &DebugSnapshot, events| {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            },
            |ui| {
                show_hud(ui.ctx(), snapshot, None);
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
fn definition_window_keeps_move_rows_in_place() {
    use crate::provider::action_definition::ActionDefinition;
    use mhf_resource::action_definition::{ActionStep, Definition};
    let mut ui = PanelUi::new(populated_snapshot(8), vec2(1200.0, 900.0), Page::Actions);
    let action = Action {
        weapon: 0,
        group: 1,
        id: 2,
    };
    ui.settle();
    let list_clip = ui
        .texts
        .iter()
        .find(|(text, _, _)| text.starts_with("武器招式 "))
        .unwrap()
        .2;
    let rows = |texts: &[(String, Rect, Rect)]| {
        texts
            .iter()
            .filter(|(text, _, clip)| text.starts_with("武器招式 ") && *clip == list_clip)
            .map(|(text, rect, _)| (text.clone(), rect.min))
            .collect::<Vec<_>>()
    };
    let before = rows(&ui.texts);
    assert!(!before.is_empty());
    ui.snapshot.action_definition = Some(Arc::new(ActionDefinition {
        action,
        motion_style: Some(0),
        attacks: None,
        data: Ok(Definition {
            weapon: action.weapon,
            action: u16::from(action.id),
            offset: 0,
            steps_range: 24..24 + 20 * 12,
            events_range: 0..0,
            steps: (0..20).map(|_| ActionStep([3, 1405, 0, 4, 0, 1])).collect(),
            events: vec![],
        }),
    }));
    ui.window.definition_action = Some(action);
    ui.settle();
    assert!(
        ui.texts
            .iter()
            .any(|(text, _, _)| text == "motion/w00.mot#4/5")
    );
    assert!(
        ui.texts
            .iter()
            .any(|(text, _, _)| text.starts_with("步骤 0 ·")),
        "definition is visible in its own window"
    );
    assert_eq!(before, rows(&ui.texts));
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
    ui.window.page = Page::Monsters;
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
fn task_tab_exposes_session_controls_from_each_page_in_a_short_window() {
    verify_session_controls_accessibility(false);
}

#[test]
fn task_tab_navigation_reaches_and_activates_session_controls_from_each_page() {
    verify_session_controls_accessibility(true);
}

fn verify_session_controls_accessibility(navigate_with_tabs: bool) {
    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0), vec2(1280.0, 900.0)] {
        let height = size.y;
        for page in Page::ALL {
            let context = context();
            let control = DebugControl::new();
            let mut window = DebugPanel::new(control.clone());
            window.page = page;
            let mut input = InputSettings::default();
            // The starting page has realistic scrollable content, while task
            // controls must remain reachable after selecting the task tab.
            let snapshot = populated_snapshot(if navigate_with_tabs { 8 } else { 2000 });
            let screen = Rect::from_min_size(pos2(0.0, 0.0), size);
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
                        window.show(ui, &snapshot, &mut input);
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
                .read_response(Id::new("debug-pages").with(("header", Id::new("task"))))
                .unwrap();
            assert!(
                screen.contains_rect(tab.rect) && (size.x >= 920.0 || tab.rect.top() < 40.0),
                "navigation must stay fully visible at {size:?}: {tab:?}"
            );
            if page != Page::Task {
                assert!(
                    context.read_response(Id::new("debug-exit")).is_none()
                        && context.read_response(Id::new("debug-restart")).is_none(),
                    "task controls must not allocate widgets on {page:?}"
                );
            }
            if height == 900.0 && matches!(page, Page::Equipment | Page::Transmog) {
                assert!(
                    visible_equipment >= 6,
                    "only {visible_equipment} equipment rows are fully visible"
                );
            }

            // Navigate through the actual task tab from every starting page.
            let position = tab.rect.center();
            for pressed in [true, false] {
                frame(
                    vec![
                        Event::PointerMoved(position),
                        Event::PointerButton {
                            pos: position,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: Modifiers::NONE,
                        },
                    ],
                    false,
                );
            }
            frame(vec![], false);
            let controls = context.read_response(Id::new("debug-exit")).unwrap();
            assert!(
                controls.rect.top() > tab.rect.bottom(),
                "task controls belong inside task content: {controls:?}, {tab:?}"
            );

            if navigate_with_tabs {
                for _ in 0..24 {
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
                            "Tab focus is clipped from page {page:?}, height {height}: {focused:?}"
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
            assert!(exit.has_focus(), "from page {page:?}, height {height}");
            assert!(
                screen.contains_rect(exit.rect),
                "from page {page:?}, height {height}: {exit:?}"
            );
            assert!(
                exit.interact_rect.height() >= exit.rect.height() - 1.0
                    && exit.interact_rect.width() >= exit.rect.width() - 1.0,
                "task controls are clipped from page {page:?}, height {height}: {exit:?}"
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
fn management_and_ai_share_the_instance_without_losing_edited_drafts() {
    use crate::provider::{AiDocument, AiOperation, AiReply, AiTarget};
    let first = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 1,
        serial: 1,
        model: 0x2000,
        species: 6,
    };
    let second = AiTarget { slot: 2, ..first };
    let source = "mhf_ai 1; species 6; base native;";
    let edited = format!("{source}\n// retained draft");
    let mut ui = PanelUi::new(
        DebugSnapshot {
            ready: true,
            ai_targets: vec![first, second],
            ..Default::default()
        },
        vec2(1280.0, 900.0),
        Page::MonsterAi,
    );
    let reply = |request, target, descriptor| {
        Arc::new(AiReply {
            request,
            target,
            result: Ok(AiDocument {
                descriptor,
                source: Some(mhf_monster::ai::dsl::Project::single(
                    None,
                    6,
                    source.into(),
                )),
            }),
        })
    };
    ui.settle();
    let initial = ui.window.control.commands();
    let [DebugCommand::MonsterAi { request, .. }] = initial.as_slice() else {
        panic!("missing initial inspect");
    };
    ui.snapshot.ai_reply = Some(reply(*request, first, 0x3000));
    ui.settle();
    ui.click_at(ui.visible_text(source).center());
    let modifiers = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    ui.frame(vec![
        Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers,
        },
        Event::Key {
            key: Key::A,
            physical_key: None,
            pressed: false,
            repeat: false,
            modifiers,
        },
        Event::Text(edited.clone()),
    ]);
    assert!(ui.window.control.commands().is_empty());
    ui.select_page(Page::MonsterManagement);
    assert!(
        ui.window.control.commands().is_empty(),
        "page changes keep the active instance"
    );

    for (label, target, descriptor) in [
        ("#2 大怪鸟 · 物种 6", second, 0x4000),
        ("#1 大怪鸟 · 物种 6", first, 0x3100),
    ] {
        let picker = ui
            .context
            .read_response(select_field_id("ai-target"))
            .unwrap();
        ui.click_at(picker.rect.center());
        ui.click_at(ui.visible_text(label).center());
        let commands = ui.window.control.commands();
        let [
            DebugCommand::MonsterAi {
                request,
                target: inspected,
                operation: AiOperation::Inspect,
            },
        ] = commands.as_slice()
        else {
            panic!("missing shared target inspect");
        };
        assert_eq!(*inspected, target);
        ui.snapshot.ai_reply = Some(reply(*request, target, descriptor));
        ui.settle();
    }
    ui.select_page(Page::MonsterAi);
    let apply = ui.context.read_response(Id::new("ai-apply")).unwrap();
    ui.click_at(apply.rect.center());
    assert!(
        matches!(ui.window.control.commands().as_slice(), [DebugCommand::MonsterAi {
        target,
        operation: AiOperation::Apply { descriptor: 0x3100, source },
        ..
    }] if *target == first && source.files[0].source == edited)
    );
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
    ui.window.page = Page::MonsterAi;
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
    ui.click_id(Id::new("ai-apply"));
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
        }),
    }));
    ui.frame(vec![]);
    ui.click_id(Id::new("ai-apply"));
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
    ui.click_id(Id::new("ai-apply"));
    let commands = ui.window.control.commands();
    assert!(
        matches!(commands.as_slice(), [DebugCommand::MonsterAi { operation: AiOperation::Apply { source: actual, .. }, .. }] if actual.files[0].source == source)
    );
    ui.snapshot.ai_targets[0].serial += 1;
    ui.frame(vec![]);
    ui.click_id(Id::new("ai-apply"));
    assert!(ui.window.control.commands().is_empty());
}

#[test]
fn monster_selector_refreshes_source_and_moves_the_attached_debugger() {
    use crate::provider::{
        AiDebugOperation, AiDebugSnapshot, AiDocument, AiOperation, AiReply, AiTarget,
    };
    let first = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 1,
        serial: 1,
        model: 0x2000,
        species: 6,
    };
    let second = AiTarget { slot: 2, ..first };
    let state = mhf_ai_debug::Snapshot::default();
    let mut ui = DebugUi::new(DebugSnapshot {
        ready: true,
        ai_targets: vec![first, second],
        ..Default::default()
    });
    ui.window.page = Page::MonsterAi;
    ui.frame(vec![]);
    let initial = ui.window.control.commands();
    let [DebugCommand::MonsterAi { request, .. }] = initial.as_slice() else {
        panic!("missing initial inspect");
    };
    let reply = |request, target, source: &str| {
        Arc::new(AiReply {
            request,
            target,
            result: Ok(AiDocument {
                descriptor: 0x3000,
                source: Some(mhf_monster::ai::dsl::Project::single(
                    None,
                    6,
                    source.into(),
                )),
            }),
        })
    };
    ui.snapshot.ai_reply = Some(reply(*request, first, "script_for_first"));
    ui.snapshot.ai_debug = Some(Arc::new(AiDebugSnapshot {
        target: first,
        attached: true,
        paused: true,
        state: state.clone(),
        recording: mhf_ai_debug::Recording::empty(state),
        reason: String::new(),
        breakpoints: vec![],
        debug_info: Default::default(),
    }));
    ui.frame(vec![]);
    ui.click("#1 大怪鸟");
    ui.click("#2 大怪鸟");
    let commands = ui.window.control.commands();
    let [
        DebugCommand::AiDebug {
            target: detached,
            operation: AiDebugOperation::Detach,
        },
        DebugCommand::MonsterAi {
            target: inspected,
            request,
            operation: AiOperation::Inspect,
        },
    ] = commands.as_slice()
    else {
        panic!("selection did not switch debugger and script");
    };
    assert_eq!(*detached, first);
    assert_eq!(*inspected, second);
    assert!(!ui.texts.iter().any(|(text, _)| text == "script_for_first"));
    ui.snapshot.ai_reply = Some(reply(*request, second, "script_for_second"));
    ui.frame(vec![]);
    assert!(ui.texts.iter().any(|(text, _)| text == "script_for_second"));
    assert!(
        matches!(ui.window.control.commands().as_slice(), [DebugCommand::AiDebug { target, operation: AiDebugOperation::Attach }] if *target == second)
    );
}
