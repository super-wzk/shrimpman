use super::*;
use crate::provider::{
    AiDocument, AiOperation, AiReply, AiTarget, Catalog, DebugCommand, DebugSnapshot, Monster,
    MonsterAction, MonsterInput,
};
use eframe::App as _;
use egui::{Event, Key, Modifiers, Pos2, RawInput, Rect, ViewportEvent, ViewportId, pos2, vec2};

struct DesktopUi {
    context: egui::Context,
    app: DebugApp,
    frame: eframe::Frame,
    texts: Vec<(String, Rect)>,
    size: egui::Vec2,
}

impl DesktopUi {
    fn new(snapshot: DebugSnapshot) -> Self {
        let context = egui::Context::default();
        context.set_os(egui::os::OperatingSystem::Windows);
        mhf_font::install(&context);
        egui_hunter::Theme::default()
            .density(egui_hunter::Density::Compact)
            .apply(&context);
        let control = DebugControl::new();
        control.publish(snapshot);
        let window = Arc::new(WindowState::default());
        window.visible.store(true, Ordering::Release);
        window.context.set(context.clone()).unwrap();
        Self {
            app: DebugApp {
                settings: control.ui_settings(),
                panel: DebugPanel::new(control.clone()),
                control,
                window,
            },
            context,
            frame: eframe::Frame::_new_kittest(),
            texts: Vec::new(),
            size: vec2(1000.0, 760.0),
        }
    }

    fn run(&mut self, events: Vec<Event>, close: bool) -> Vec<ViewportCommand> {
        let mut raw = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), self.size)),
            focused: true,
            events,
            ..Default::default()
        };
        if close {
            raw.viewports
                .get_mut(&ViewportId::ROOT)
                .unwrap()
                .events
                .push(ViewportEvent::Close);
        }
        let output = self.context.run_ui(raw, |ui| {
            self.app.logic(ui.ctx(), &mut self.frame);
            self.app.ui(ui, &mut self.frame);
        });
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
        let commands = output.viewport_output[&ViewportId::ROOT].commands.clone();
        output.drop_without_applying_deltas();
        commands
    }

    fn position(&mut self, label: &str) -> Pos2 {
        self.run(Vec::new(), false);
        self.run(Vec::new(), false);
        self.texts
            .iter()
            .find(|(text, _)| text == label)
            .unwrap_or_else(|| panic!("missing UI label {label:?}: {:?}", self.texts))
            .1
            .center()
    }

    fn click_at(&mut self, position: Pos2) {
        for pressed in [true, false] {
            self.run(
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
    }

    fn click(&mut self, label: &str) {
        let position = self.position(label);
        self.click_at(position);
    }

    fn click_id(&mut self, id: egui::Id) {
        self.run(Vec::new(), false);
        let response = self.context.read_response(id).unwrap();
        self.click_at(response.rect.center());
    }

    fn f7(&mut self) -> Vec<ViewportCommand> {
        self.run(key(Key::F7, true, false, Modifiers::NONE), false)
    }
}

fn key(key: Key, pressed: bool, repeat: bool, modifiers: Modifiers) -> Vec<Event> {
    vec![
        Event::ModifiersChanged(modifiers),
        Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat,
            modifiers,
        },
    ]
}

fn target() -> AiTarget {
    AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 7,
        serial: 42,
        model: 0x2000,
        species: 4,
    }
}

fn snapshot() -> DebugSnapshot {
    DebugSnapshot {
        quest_id: 40001,
        ready: true,
        areas: vec![245],
        area: 245,
        map: 44,
        ai_targets: vec![target()],
        catalog: Arc::new(Catalog {
            monsters: [4, 94]
                .into_iter()
                .map(|id| Monster {
                    id,
                    name: crate::provider::monsters::NAMES[usize::from(id)],
                    variants: crate::provider::monsters::variants(id),
                    actions: Arc::new(Vec::new()),
                })
                .collect(),
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn load_ai_source(ui: &mut DesktopUi, source: &str) {
    ui.click("怪物 AI");
    let commands = ui.app.control.commands();
    let [
        DebugCommand::MonsterAi {
            request,
            target: inspected,
            operation: AiOperation::Inspect,
        },
    ] = commands.as_slice()
    else {
        panic!("opening AI page must request its source through the command queue")
    };
    assert_eq!(*inspected, target());
    let mut snapshot = ui.app.control.snapshot();
    snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target: target(),
        result: Ok(AiDocument {
            descriptor: 0x3000,
            source: Some(mhf_monster::ai::dsl::Project::single(
                None,
                4,
                source.into(),
            )),
        }),
    }));
    ui.app.control.publish(snapshot);
    ui.run(Vec::new(), false);
}

#[test]
fn close_and_f7_keep_the_panel_and_unapplied_ai_draft() {
    let mut ui = DesktopUi::new(snapshot());
    let source = "mhf_ai 1; species 4; base native;\n// initial draft";
    load_ai_source(&mut ui, source);
    let editor = ui.position(source);
    ui.click_at(editor);
    let edited = "mhf_ai 1; species 4; base native;\n// unapplied retained draft";
    let modifiers = Modifiers {
        ctrl: true,
        command: true,
        ..Modifiers::NONE
    };
    let mut events = key(Key::A, true, false, modifiers);
    events.extend(key(Key::A, false, false, modifiers));
    events.push(Event::Text(edited.into()));
    ui.run(events, false);
    assert!(ui.app.control.commands().is_empty(), "editing is local");

    let closed = ui.run(Vec::new(), true);
    assert!(!ui.app.window.visible.load(Ordering::Acquire));
    assert!(closed.contains(&ViewportCommand::CancelClose));
    assert!(closed.contains(&ViewportCommand::Visible(false)));
    assert!(!closed.contains(&ViewportCommand::Close));
    let shown = ui.f7();
    assert!(shown.contains(&ViewportCommand::Visible(true)));
    assert!(shown.contains(&ViewportCommand::Focus));
    assert!(ui.app.window.visible.load(Ordering::Acquire));
    ui.run(key(Key::F7, false, false, Modifiers::NONE), false);
    assert!(ui.f7().contains(&ViewportCommand::Visible(false)));
    ui.run(key(Key::F7, false, false, Modifiers::NONE), false);
    assert!(ui.f7().contains(&ViewportCommand::Visible(true)));

    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0), vec2(1280.0, 900.0)] {
        ui.size = size;
        for _ in 0..4 {
            ui.run(Vec::new(), false);
        }
        assert!(
            ui.app.control.commands().is_empty(),
            "resizing preserves the local draft"
        );
    }
    ui.click_id(egui::Id::new("ai-apply"));
    let commands = ui.app.control.commands();
    assert!(
        matches!(commands.as_slice(), [DebugCommand::MonsterAi {
            target: applied,
            operation: AiOperation::Apply { descriptor: 0x3000, source },
            ..
        }] if *applied == target() && source.files[0].source == edited),
        "the retained draft must be submitted to the original instance"
    );
}

fn select_id(name: &str) -> egui::Id {
    let id = egui::Id::new(name);
    id.with("select").with(egui::IdSalt::new(id))
}

#[test]
fn ai_toolbar_controls_align_and_keep_geometry_when_hovered_or_switching_mode() {
    let mut ui = DesktopUi::new(snapshot());
    load_ai_source(&mut ui, "mhf_ai 1; species 4; base native;");
    let ids = [
        select_id("ai-session-mode"),
        select_id("ai-target"),
        egui::Id::new("ai-attach"),
        egui::Id::new("ai-pause"),
        egui::Id::new("ai-step"),
        egui::Id::new("ai-run-yield"),
        egui::Id::new("ai-apply"),
        egui::Id::new("ai-source-refresh"),
        egui::Id::new("ai-source-load"),
        egui::Id::new("ai-source-restore"),
        egui::Id::new("ai-source-copy"),
        egui::Id::new("ai-recording-save"),
        egui::Id::new("ai-trace-clear"),
        egui::Id::new("ai-status-hud"),
    ];
    for size in [vec2(440.0, 360.0), vec2(720.0, 480.0), vec2(1280.0, 900.0)] {
        ui.size = size;
        for _ in 0..4 {
            ui.run(Vec::new(), false);
        }
        let rects = ids.map(|id| ui.context.read_response(id).unwrap().rect);
        let viewport = Rect::from_min_size(pos2(0.0, 0.0), size);
        for (index, rect) in rects.iter().enumerate() {
            assert!(
                viewport.contains_rect(*rect),
                "toolbar control {index} is outside {size:?}: {rect:?}; all controls {rects:?}"
            );
            assert!(
                (rect.height() - rects[0].height()).abs() < 1.0,
                "field and button heights differ: {rect:?}, {:?}",
                rects[0]
            );
        }
        for (index, rect) in rects.iter().enumerate() {
            if index >= 2 {
                assert!(
                    (rect.width() - rect.height()).abs() < 1.0,
                    "action icon is not square: {rect:?}"
                );
            }
            for other in &rects[..index] {
                if rect.y_range().intersects(other.y_range()) {
                    assert!(
                        (rect.center().y - other.center().y).abs() < 1.0,
                        "toolbar centers differ at {size:?}: {rect:?}, {other:?}"
                    );
                }
            }
        }
        assert!(ui.context.read_response(egui::Id::new("ai-more")).is_none());
        for hovered in rects {
            ui.run(vec![Event::PointerMoved(hovered.center())], false);
            let actual = ids.map(|id| ui.context.read_response(id).unwrap().rect);
            assert_eq!(actual, rects, "hover changed toolbar geometry at {size:?}");
        }
        assert!(ui.app.control.commands().is_empty(), "hover is local");
        ui.click_id(select_id("ai-session-mode"));
        ui.click("回放");
        ui.run(Vec::new(), false);
        assert!(
            ui.context
                .read_response(egui::Id::new("ai-apply"))
                .is_none()
        );
        assert_eq!(
            ui.context
                .read_response(select_id("ai-session-mode"))
                .unwrap()
                .rect,
            rects[0]
        );
        ui.click_id(select_id("ai-session-mode"));
        ui.click("现场");
        ui.run(Vec::new(), false);
        assert!(
            ui.context
                .read_response(egui::Id::new("ai-apply"))
                .unwrap()
                .enabled()
        );
        assert!(
            ui.app.control.commands().is_empty(),
            "mode selection is local"
        );
    }
}

#[test]
fn exposed_ai_icons_keep_native_operations_disabled_while_pending_or_loading() {
    use crate::provider::AiDebugSnapshot;
    let mut ui = DesktopUi::new(snapshot());
    load_ai_source(&mut ui, "mhf_ai 1; species 4; base native;");
    let mut snapshot = ui.app.control.snapshot();
    let state = mhf_ai_debug::Snapshot::default();
    snapshot.ai_debug = Some(Arc::new(AiDebugSnapshot {
        target: target(),
        attached: true,
        paused: true,
        reason: String::new(),
        recording: mhf_ai_debug::Recording::empty(state.clone()),
        state,
        breakpoints: Vec::new(),
        debug_info: Default::default(),
    }));
    ui.app.control.publish(snapshot);
    ui.run(Vec::new(), false);
    ui.click_id(egui::Id::new("ai-source-copy"));
    ui.click_id(egui::Id::new("ai-status-hud"));
    assert!(ui.app.control.commands().is_empty());
    assert_eq!(ui.app.control.ui_settings().hud_target, Some(target()));

    ui.click_id(egui::Id::new("ai-source-refresh"));
    let commands = ui.app.control.commands();
    let [
        DebugCommand::MonsterAi {
            target: inspected,
            request,
            operation: AiOperation::Inspect,
        },
    ] = commands.as_slice()
    else {
        panic!("refresh icon must request inspection of the selected instance")
    };
    assert_eq!(*inspected, target());
    ui.run(Vec::new(), false);
    ui.run(Vec::new(), false);
    let operations = [
        "ai-apply",
        "ai-source-refresh",
        "ai-source-load",
        "ai-source-restore",
        "ai-attach",
        "ai-pause",
        "ai-step",
        "ai-run-yield",
        "ai-trace-clear",
    ];
    for id in operations {
        let response = ui.context.read_response(egui::Id::new(id)).unwrap();
        assert!(!response.enabled(), "{id} must wait for the source reply");
        ui.click_id(egui::Id::new(id));
    }
    assert!(ui.app.control.commands().is_empty());

    let mut snapshot = ui.app.control.snapshot();
    snapshot.ready = false;
    snapshot.ai_reply = Some(Arc::new(AiReply {
        request: *request,
        target: target(),
        result: Ok(AiDocument {
            descriptor: 0x3000,
            source: None,
        }),
    }));
    ui.app.control.publish(snapshot);
    ui.run(Vec::new(), false);
    for id in operations {
        assert!(
            !ui.context
                .read_response(egui::Id::new(id))
                .unwrap()
                .enabled()
        );
        ui.click_id(egui::Id::new(id));
    }
    assert!(ui.app.control.commands().is_empty());
    assert!(
        ui.context
            .read_response(egui::Id::new("ai-recording-save"))
            .unwrap()
            .enabled(),
        "an archived recording remains exportable"
    );

    let mut snapshot = ui.app.control.snapshot();
    snapshot.ready = true;
    snapshot.ai_targets[0].serial += 1;
    ui.app.control.publish(snapshot);
    ui.run(Vec::new(), false);
    ui.click_id(egui::Id::new("ai-apply"));
    ui.click_id(egui::Id::new("ai-attach"));
    assert!(
        ui.app.control.commands().is_empty(),
        "icons must reject a reused instance slot"
    );
}

#[test]
fn shutdown_closes_instead_of_cancelling_the_root_close_request() {
    let mut ui = DesktopUi::new(DebugSnapshot::default());
    ui.app.window.stop();
    let commands = ui.run(key(Key::F7, true, false, Modifiers::NONE), true);
    assert!(commands.contains(&ViewportCommand::Close));
    assert!(!commands.contains(&ViewportCommand::CancelClose));
    assert!(!commands.contains(&ViewportCommand::Visible(false)));
    assert!(ui.app.window.stopping.load(Ordering::Acquire));
}

#[test]
fn desktop_controls_publish_settings_and_queue_game_operations() {
    let mut ui = DesktopUi::new(snapshot());
    ui.app.settings.input.speed = 475.0;
    ui.app.settings.input.shortcuts[0] = Some(MonsterAction { group: 1, id: 11 });
    ui.click("怪物变身");
    ui.click(crate::provider::monsters::NAMES[94]);
    ui.click("菌猪");
    let settings = ui.app.control.ui_settings();
    assert_eq!(settings.input.species(), 4);
    assert_eq!(settings.input.variant(), 0);
    assert_eq!(settings.input.speed, 475.0);
    assert!(settings.input.shortcuts.iter().all(Option::is_none));
    assert!(ui.app.control.commands().is_empty());

    ui.click("变身并操控");
    let commands = ui.app.control.commands();
    assert!(matches!(
        commands.as_slice(),
        [DebugCommand::Transform {
            species: 4,
            variant: 0,
        }]
    ));
    ui.click("任务");
    ui.click("重开任务");
    assert!(matches!(
        ui.app.control.commands().as_slice(),
        [DebugCommand::Restart]
    ));
    assert_eq!(
        ui.app.control.snapshot().monster,
        None,
        "UI cannot execute game commands"
    );
}

#[test]
fn desktop_keyboard_does_not_sample_or_replace_game_control_input() {
    let mut snapshot = snapshot();
    snapshot.monster = Some(94);
    snapshot.controlling_monster = true;
    let mut ui = DesktopUi::new(snapshot);
    let game_input = MonsterInput {
        forward: 0.5,
        sideways: -0.25,
        vertical: 0.75,
        speed: 475.0,
    };
    ui.app.control.set_monster_input(game_input);
    let events = [Key::W, Key::D, Key::E, Key::R, Key::Backspace]
        .into_iter()
        .flat_map(|pressed| key(pressed, true, false, Modifiers::NONE))
        .collect();
    ui.run(events, false);
    assert!(ui.app.control.commands().is_empty());
    let current = ui.app.control.shared.lock().unwrap().monster_input;
    assert_eq!(
        [
            current.forward,
            current.sideways,
            current.vertical,
            current.speed
        ],
        [
            game_input.forward,
            game_input.sideways,
            game_input.vertical,
            game_input.speed
        ]
    );
}

#[test]
#[ignore = "opens a real eframe window; run alone on Windows/Wine with a display"]
fn native_desktop_can_hide_reopen_and_stop_without_a_game_dll() {
    let control = DebugControl::new();
    let mut snapshot = snapshot();
    let state = mhf_ai_debug::Snapshot::default();
    snapshot.ai_debug = Some(Arc::new(crate::provider::AiDebugSnapshot {
        target: target(),
        attached: false,
        paused: false,
        reason: String::new(),
        recording: mhf_ai_debug::Recording::empty(state.clone()),
        state,
        breakpoints: Vec::new(),
        debug_info: Default::default(),
    }));
    control.publish(snapshot);
    let mut desktop = DebugDesktop::start(control).expect("native window initialization");
    let window = desktop.state();
    let context = window.context.get().unwrap().clone();
    for size in [vec2(440.0, 360.0), vec2(1040.0, 700.0)] {
        context.send_viewport_cmd(ViewportCommand::InnerSize(size));
        wait_until(|| (context.content_rect().size() - size).abs().max_elem() < 2.0);
        assert!(!desktop.thread.as_ref().unwrap().is_finished());
    }
    // The winit integration translates this command into the same close event
    // as the OS title-bar button, on the independently running UI thread.
    context.send_viewport_cmd(ViewportCommand::Close);
    wait_until(|| !window.visible.load(Ordering::Acquire));
    assert!(!window.stopping.load(Ordering::Acquire));
    assert!(!desktop.thread.as_ref().unwrap().is_finished());
    let hidden_frame = context.cumulative_frame_nr();
    window.toggle();
    assert!(window.visible.load(Ordering::Acquire));
    wait_until(|| context.cumulative_frame_nr() > hidden_frame);
    let owner = HWND(window.native_window.load(Ordering::Acquire) as *mut _);
    click_native_widget(
        &context,
        owner,
        egui::Id::new("debug-pages").with(("header", egui::Id::new("monster-ai"))),
    );
    click_native_widget(&context, owner, egui::Id::new("ai-recording-save"));
    wait_until(|| file_dialog_is_open(owner));
    assert!(!desktop.thread.as_ref().unwrap().is_finished());
    // Teardown must dismiss the native picker instead of waiting for a user.
    desktop.stop().expect("native window shutdown");
    assert!(desktop.thread.is_none());
    desktop.stop().expect("shutdown is idempotent");
}

fn click_native_widget(context: &egui::Context, owner: HWND, id: egui::Id) {
    use windows::Win32::{
        Foundation::{LPARAM, WPARAM},
        UI::WindowsAndMessaging::{PostMessageW, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE},
    };
    wait_until(|| {
        context
            .read_response(id)
            .is_some_and(|response| response.enabled())
    });
    let point = context.read_response(id).unwrap().rect.center() * context.pixels_per_point();
    let position = LPARAM(
        ((point.x.round() as u16 as u32) | ((point.y.round() as u16 as u32) << 16)) as isize,
    );
    unsafe {
        PostMessageW(Some(owner), WM_MOUSEMOVE, WPARAM(0), position).unwrap();
        PostMessageW(Some(owner), WM_LBUTTONDOWN, WPARAM(1), position).unwrap();
        PostMessageW(Some(owner), WM_LBUTTONUP, WPARAM(0), position).unwrap();
    }
}

fn file_dialog_is_open(owner: HWND) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{
        GA_ROOTOWNER, GetAncestor, GetClassNameW, GetLastActivePopup,
    };
    let mut class = [0_u16; 16];
    unsafe {
        let popup = GetLastActivePopup(owner);
        let length = GetClassNameW(popup, &mut class) as usize;
        popup != owner
            && class[..length] == *windows::core::w!("#32770").as_wide()
            && GetAncestor(popup, GA_ROOTOWNER) == owner
    }
}

fn wait_until(mut condition: impl FnMut() -> bool) {
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(
            std::time::Instant::now() < deadline,
            "native UI transition timed out"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}
