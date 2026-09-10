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
        diagnostics: BTreeMap::new(),
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

fn error_text(
    output: &egui::FullOutput,
    rect: Rect,
    color: egui::Color32,
) -> impl Iterator<Item = &str> {
    output.shapes.iter().filter_map(move |shape| {
        let egui::Shape::Text(text) = &shape.shape else {
            return None;
        };
        (rect.contains(text.pos)
            && (text.fallback_color == color
                || text
                    .galley
                    .job
                    .sections
                    .iter()
                    .any(|section| section.format.color == color)))
        .then_some(text.galley.job.text.as_str())
    })
}

#[test]
fn dependency_errors_color_consumers_and_their_details_without_coloring_disabled_dependencies() {
    let context = egui::Context::default();
    let mut app = app(&context);
    app.draft.get_mut("mhf.debug").unwrap().enabled = Some(true);
    app.draft.get_mut("mhf.base").unwrap().enabled = Some(false);
    app.update_preview();
    assert!(app.preview.is_err());
    for id in ["mhf.debug", "mhf.login"] {
        assert!(app.diagnostics[id].issues.iter().any(|issue| {
            issue.dependency.as_deref() == Some("mhf.base")
                && issue.kind == DependencyIssueKind::Disabled
        }));
    }
    for id in ["mhf.base", "mhf.config"] {
        assert!(
            app.diagnostics
                .get(id)
                .is_none_or(|diagnostic| diagnostic.issues.is_empty())
        );
    }

    let size = egui::vec2(960.0, 640.0);
    let error_color = context.global_style().visuals.error_fg_color;
    let row = |id| {
        context
            .read_response(Id::new(("mod_row", id)))
            .unwrap()
            .rect
    };
    let details = |id| {
        let left = context
            .read_response(Id::new(("version", id)))
            .unwrap()
            .rect
            .left();
        let bottom = context
            .read_response(Id::new("save_mod_settings"))
            .unwrap()
            .rect
            .top();
        Rect::from_min_max(egui::pos2(left, 0.0), egui::pos2(size.x, bottom))
    };
    for (selected, has_dependency_error) in [
        ("mhf.config", false),
        ("mhf.base", false),
        ("mhf.login", true),
        ("mhf.debug", true),
    ] {
        app.selected = Some(selected.into());
        frame(&context, &mut app, size, Vec::new());
        let (_, output) = frame(&context, &mut app, size, Vec::new());
        for (id, affected) in [
            ("mhf.debug", true),
            ("mhf.login", true),
            ("mhf.base", false),
            ("mhf.config", false),
        ] {
            assert_eq!(
                error_text(&output, row(id), error_color).next().is_some(),
                affected,
                "{id}"
            );
        }
        let mut errors = error_text(&output, details(selected), error_color);
        if has_dependency_error {
            assert!(errors.any(|text| text.contains("mhf.base")));
        } else {
            assert!(errors.next().is_none(), "{selected}");
        }
    }
    assert!(
        !context
            .read_response(Id::new("save_mod_settings"))
            .unwrap()
            .enabled()
    );

    app.draft.get_mut("mhf.base").unwrap().enabled = None;
    app.update_preview();
    assert!(app.preview.is_ok());
    assert!(
        app.diagnostics
            .values()
            .all(|diagnostic| diagnostic.issues.is_empty())
    );
    frame(&context, &mut app, size, Vec::new());
    let (_, output) = frame(&context, &mut app, size, Vec::new());
    for id in ["mhf.base", "mhf.config", "mhf.debug", "mhf.login"] {
        assert!(
            error_text(&output, row(id), error_color).next().is_none(),
            "{id}"
        );
    }
    assert!(
        error_text(&output, details("mhf.debug"), error_color)
            .next()
            .is_none()
    );
    assert!(
        context
            .read_response(Id::new("save_mod_settings"))
            .unwrap()
            .enabled()
    );
}

#[test]
fn only_automatic_mods_in_the_launch_selection_are_diagnosed() {
    let context = egui::Context::default();
    let mut app = app(&context);
    app.draft.get_mut("mhf.base").unwrap().enabled = Some(false);
    let size = egui::vec2(960.0, 640.0);
    let error_color = context.global_style().visuals.error_fg_color;
    for enabled in [None, Some(false)] {
        app.draft.get_mut("mhf.login").unwrap().enabled = enabled;
        app.update_preview();
        // Launch defaults must not add automatic mods to the saved/exported selection.
        assert!(app.preview.is_ok());
        assert!(!app.diagnostics.contains_key("mhf.debug"));
        let login_affected = enabled.is_none();
        assert_eq!(
            app.diagnostics
                .get("mhf.login")
                .is_some_and(|diagnostic| !diagnostic.issues.is_empty()),
            login_affected
        );
        app.selected = Some("mhf.login".into());
        frame(&context, &mut app, size, Vec::new());
        let (_, output) = frame(&context, &mut app, size, Vec::new());
        for (id, affected) in [
            ("mhf.debug", false),
            ("mhf.login", login_affected),
            ("mhf.base", false),
            ("mhf.config", false),
        ] {
            let row = context
                .read_response(Id::new(("mod_row", id)))
                .unwrap()
                .rect;
            assert_eq!(
                error_text(&output, row, error_color).next().is_some(),
                affected,
                "{id}"
            );
        }
        let left = context
            .read_response(Id::new(("version", "mhf.login")))
            .unwrap()
            .rect
            .left();
        let save = context.read_response(Id::new("save_mod_settings")).unwrap();
        let details =
            Rect::from_min_max(egui::pos2(left, 0.0), egui::pos2(size.x, save.rect.top()));
        assert_eq!(
            error_text(&output, details, error_color).next().is_some(),
            login_affected
        );
        assert!(save.enabled());
    }
}

#[test]
fn unused_automatic_mods_do_not_report_disabled_dependencies() {
    let context = egui::Context::default();
    let mut app = app(&context);
    for id in ["mhf.config", "mhf.debug", "mhf.login"] {
        app.draft.get_mut(id).unwrap().enabled = Some(false);
    }
    app.selected = Some("mhf.base".into());
    let size = egui::vec2(960.0, 640.0);
    let error_color = context.global_style().visuals.error_fg_color;
    for enabled in [false, true, false] {
        app.draft.get_mut("mhf.debug").unwrap().enabled = Some(enabled);
        app.update_preview();
        assert_eq!(app.preview.is_err(), enabled);
        if enabled {
            assert!(app.diagnostics["mhf.base"].issues.iter().any(|issue| {
                issue.dependency.as_deref() == Some("mhf.config")
                    && issue.kind == DependencyIssueKind::Disabled
            }));
            assert!(!app.diagnostics["mhf.debug"].issues.is_empty());
            assert!(!app.diagnostics.contains_key("mhf.config"));
        } else {
            assert!(app.diagnostics.is_empty());
            assert!(app.preview.as_ref().unwrap().mods.is_empty());
        }
        frame(&context, &mut app, size, Vec::new());
        let (_, output) = frame(&context, &mut app, size, Vec::new());
        let row = context
            .read_response(Id::new(("mod_row", "mhf.base")))
            .unwrap();
        let version = context
            .read_response(Id::new(("version", "mhf.base")))
            .unwrap();
        let save = context.read_response(Id::new("save_mod_settings")).unwrap();
        let details = Rect::from_min_max(
            egui::pos2(version.rect.left(), 0.0),
            egui::pos2(size.x, save.rect.top()),
        );
        for rect in [row.rect, details] {
            assert_eq!(
                error_text(&output, rect, error_color).next().is_some(),
                enabled
            );
        }
    }
}

#[test]
fn invalid_version_is_scoped_to_its_mod_and_dependency_errors_fit_narrow_windows() {
    let context = egui::Context::default();
    let mut app = app(&context);
    app.draft.get_mut("mhf.debug").unwrap().version = "invalid range".into();
    app.update_preview();
    assert_eq!(app.diagnostics.len(), 1);
    assert_eq!(
        app.diagnostics["mhf.debug"].issues[0].kind,
        DependencyIssueKind::InvalidVersion
    );
    app.draft.get_mut("mhf.debug").unwrap().version.clear();
    app.draft.get_mut("mhf.debug").unwrap().enabled = Some(true);
    app.draft.get_mut("mhf.base").unwrap().enabled = Some(false);
    app.selected = Some("mhf.debug".into());
    app.update_preview();
    for size in [
        egui::vec2(960.0, 640.0),
        egui::vec2(420.0, 440.0),
        egui::vec2(480.0, 320.0),
    ] {
        frame(&context, &mut app, size, Vec::new());
        let (bounds, _) = frame(&context, &mut app, size, Vec::new());
        assert!(bounds.right() <= size.x + 1.0, "{size:?}: {bounds:?}");
        assert!(bounds.bottom() <= size.y + 1.0, "{size:?}: {bounds:?}");
        let viewport = Rect::from_min_size(Pos2::ZERO, size);
        for id in ["save_mod_settings", "revert_mod_settings"] {
            assert!(viewport.contains_rect(context.read_response(Id::new(id)).unwrap().rect));
        }
    }
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
        frame(&context, &mut app, size, Vec::new());
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
