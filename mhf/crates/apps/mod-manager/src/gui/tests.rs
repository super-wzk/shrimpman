use super::*;
use eframe::App as _;
use egui::{Event, Id, Modifiers, PointerButton, Pos2, RawInput, Rect, Vec2};
use mhf_mod_package::{BuiltinCatalog, RuntimeConfig};

fn app(context: &egui::Context) -> App {
    mhf_font::install(context);
    egui_hunter::Theme::default().apply(context);
    let mut app = App {
        manager: Manager::new("unused-test-config.toml".into(), None).unwrap(),
        snapshot: Some(Snapshot {
            config: RuntimeConfig::default(),
            mods_dir: "mods".into(),
            candidates: BuiltinCatalog {
                login: true,
                debug: true,
            }
            .candidates()
            .unwrap(),
        }),
        draft: BTreeMap::new(),
        preview: Err(String::new()),
        selected: None,
        filter: String::new(),
        pending: None,
        feedback: None,
        archive_action: ArchiveAction::Import,
        archive_path: String::new(),
        archive_error: None,
        archive_dialog: DialogState::default(),
        close_dialog: DialogState::default(),
        close_after_save: false,
        allow_close: false,
    };
    app.reset_draft();
    app.selected = app.draft.keys().next().cloned();
    app
}

fn frame(
    context: &egui::Context,
    app: &mut App,
    size: Vec2,
    events: Vec<Event>,
) -> (Rect, egui::FullOutput) {
    let mut bounds = Rect::NOTHING;
    let mut output = context.run_ui(
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, size)),
            events,
            ..Default::default()
        },
        |ui| {
            app.ui(ui, &mut eframe::Frame::_new_kittest());
            bounds = ui.min_rect();
        },
    );
    output.textures_delta.clear();
    (bounds, output)
}

#[test]
fn desktop_narrow_and_zoomed_layouts_keep_controls_inside_the_viewport() {
    for size in [
        egui::vec2(960.0, 640.0),
        egui::vec2(720.0, 500.0),
        egui::vec2(420.0, 440.0),
        // 960 × 640 at 200% UI scale.
        egui::vec2(480.0, 320.0),
    ] {
        let context = egui::Context::default();
        let mut app = app(&context);
        frame(&context, &mut app, size, Vec::new());
        let (_, output) = frame(&context, &mut app, size, Vec::new());
        if size.x >= 720.0 {
            let labels = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text)
                        if matches!(text.galley.job.text.as_str(), "版本要求" | "已安装版本") =>
                    {
                        Some((text.galley.job.text.as_str(), text.pos))
                    }
                    _ => None,
                })
                .collect::<BTreeMap<_, _>>();
            let requirement = labels["版本要求"];
            let installed = labels["已安装版本"];
            if size.x == 960.0 {
                assert_eq!(requirement.y, installed.y);
                assert!(installed.x > requirement.x);
            } else {
                assert!(installed.y > requirement.y + 40.0);
            }
        }
        let id = app.selected.clone().unwrap();
        let candidate = app
            .snapshot
            .as_mut()
            .unwrap()
            .candidates
            .iter_mut()
            .find(|candidate| candidate.manifest.id == id)
            .unwrap();
        candidate.manifest.name = candidate.manifest.name.repeat(20);
        let candidate = candidate.clone();
        app.snapshot
            .as_mut()
            .unwrap()
            .candidates
            .extend((1..12).map(|minor| {
                let mut candidate = candidate.clone();
                candidate.manifest.version = format!("1.{minor}.0").parse().unwrap();
                candidate
            }));
        app.draft.get_mut(&id).unwrap().enabled = Some(true);
        app.update_preview();
        for _ in 0..2 {
            let (bounds, _) = frame(&context, &mut app, size, Vec::new());
            assert!(bounds.right() <= size.x + 1.0, "{size:?}: {bounds:?}");
            assert!(bounds.bottom() <= size.y + 1.0, "{size:?}: {bounds:?}");
            let viewport = Rect::from_min_size(Pos2::ZERO, size);
            for action in [
                "refresh_mods",
                "import_mods",
                "export_mods",
                "save_mod_settings",
                "revert_mod_settings",
            ] {
                let response = context.read_response(Id::new(action)).unwrap();
                assert!(
                    viewport.contains_rect(response.rect),
                    "{size:?}: {action} at {:?}",
                    response.rect
                );
            }
            for control in [Id::new("mod_filter"), Id::new(("version", &id))] {
                let response = context.read_response(control).unwrap();
                assert!(
                    response.rect.right() <= size.x + 1.0,
                    "{size:?}: {:?}",
                    response.rect
                );
            }
        }
        let version = Id::new(("version", &id));
        for _ in 0..40 {
            frame(
                &context,
                &mut app,
                size,
                vec![Event::Key {
                    key: egui::Key::Tab,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Modifiers::NONE,
                }],
            );
            if context.memory(|memory| memory.has_focus(version)) {
                break;
            }
        }
        assert!(context.memory(|memory| memory.has_focus(version)));
        for _ in 0..20 {
            frame(&context, &mut app, size, Vec::new());
        }
        let field = context.read_response(version).unwrap();
        assert!(
            field.interact_rect.contains_rect(field.rect.shrink(1.0)),
            "{size:?}: version editor remains clipped: {field:?}"
        );
    }
}

#[test]
fn mod_metadata_is_part_of_the_selectable_row_and_keeps_keyboard_focus() {
    let context = egui::Context::default();
    let mut app = app(&context);
    let size = egui::vec2(960.0, 640.0);
    for _ in 0..2 {
        frame(&context, &mut app, size, Vec::new());
    }
    let id = app.draft.keys().nth(1).unwrap().clone();
    let row_id = Id::new(("mod_row", &id));
    let row = context.read_response(row_id).unwrap().rect;
    assert_eq!(row.height(), 64.0);
    let pos = egui::pos2(row.left() + 18.0, row.bottom() - 18.0);
    for pressed in [true, false] {
        frame(
            &context,
            &mut app,
            size,
            vec![
                Event::PointerMoved(pos),
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
        );
    }
    assert_eq!(app.selected.as_ref(), Some(&id));
    assert!(context.memory(|memory| memory.has_focus(row_id)));
    app.selected = None;
    frame(
        &context,
        &mut app,
        size,
        vec![Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }],
    );
    assert_eq!(app.selected.as_ref(), Some(&id));
}
