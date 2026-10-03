use super::*;

const TARGET: AiTarget = AiTarget {
    epoch: 1,
    pool: 0x1000,
    slot: 0,
    serial: 1,
    model: 0,
    species: 6,
};

#[test]
fn status_hud_tracks_exact_instance_and_does_not_capture_pointer() {
    let target = AiTarget {
        slot: 2,
        serial: 7,
        ..TARGET
    };
    let mut editor = Editor {
        selected: Some(target),
        show_status: true,
        ..Default::default()
    };
    let mut snapshot = DebugSnapshot {
        ready: true,
        monster_statuses: vec![crate::provider::MonsterStatus {
            target,
            ai_state: 12,
            action_group: 3,
            action_id: 4,
            action_stage: 1,
            animation: 9,
            frame: 8.5,
            position: [1.0, 2.0, 3.0],
        }],
        ..Default::default()
    };
    let context = egui::Context::default();
    let draw = |editor: &Editor, snapshot: &DebugSnapshot| {
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                events: vec![egui::Event::PointerMoved(egui::pos2(770.0, 570.0))],
                ..Default::default()
            },
            |ui| {
                if let Some(target) = editor.hud_target() {
                    show_hud(ui.ctx(), snapshot, target);
                }
            },
        );
        let texts = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n");
        output.drop_without_applying_deltas();
        assert!(!context.egui_wants_pointer_input());
        texts
    };
    for _ in 0..3 {
        draw(&editor, &snapshot);
    }
    assert!(draw(&editor, &snapshot).contains("AI 主状态 12"));
    snapshot.monster_statuses[0].ai_state = 13;
    assert!(draw(&editor, &snapshot).contains("AI 主状态 13"));
    snapshot.monster_statuses[0].target.serial += 1;
    let texts = draw(&editor, &snapshot);
    assert!(texts.contains("目标已卸载"));
    assert!(!texts.contains("AI 主状态"));
    editor.show_status = false;
    assert!(!draw(&editor, &snapshot).contains("AI 主状态"));
}

#[test]
fn management_displays_only_the_exact_live_instance_without_changing_replay_or_drafts() {
    let target = AiTarget {
        slot: 7,
        serial: 42,
        model: 0x2000,
        ..TARGET
    };
    let status = crate::provider::MonsterStatus {
        target,
        ai_state: 12,
        action_group: 3,
        action_id: 4,
        action_stage: 1,
        animation: 9,
        frame: 8.5,
        position: [1.0, 2.0, 3.0],
    };
    for width in [320.0, 960.0] {
        let context = egui::Context::default();
        egui_hunter::Theme::default()
            .density(egui_hunter::Density::Compact)
            .apply(&context);
        let mut editor = Editor {
            selected: Some(target),
            page: Page::Replay,
            draft: Draft {
                loaded: Some((target, 0x3000)),
                source: "unsaved script".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        let control = DebugControl::new();
        let mut snapshot = DebugSnapshot {
            ready: true,
            ai_targets: vec![target],
            monster_statuses: vec![
                crate::provider::MonsterStatus {
                    target: AiTarget {
                        serial: 43,
                        ..target
                    },
                    ai_state: 77,
                    ..status.clone()
                },
                status.clone(),
            ],
            ..Default::default()
        };
        let draw = |editor: &mut Editor, snapshot: &DebugSnapshot| {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 640.0),
                    )),
                    ..Default::default()
                },
                |ui| editor.show_management(ui, snapshot, &control),
            );
            let texts = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
                    _ => None,
                })
                .collect::<Vec<_>>();
            output.drop_without_applying_deltas();
            texts
        };
        let texts = draw(&mut editor, &snapshot);
        assert!(texts.iter().any(|text| text == "AI 主状态"));
        assert!(texts.iter().any(|text| text == "12"));
        assert!(!texts.iter().any(|text| text == "77"));
        snapshot.monster_statuses.pop();
        let texts = draw(&mut editor, &snapshot);
        assert!(texts.iter().any(|text| text == "实例状态暂不可用"));
        assert!(!texts.iter().any(|text| text == "77"));
        assert_eq!(editor.selected, Some(target));
        assert_eq!(editor.page, Page::Replay);
        assert_eq!(editor.draft.source, "unsaved script");
        assert!(control.commands().is_empty());
    }
}

#[test]
fn workspace_preserves_split_scroll_and_draft_focus_across_trace_visibility_and_resize() {
    let context = egui::Context::default();
    egui_hunter::Theme::default()
        .density(egui_hunter::Density::Compact)
        .apply(&context);
    let mut editor = Editor::default();
    editor
        .draft
        .set_project(mhf_monster::ai::dsl::Project::single(
            None,
            6,
            "restart;\n".repeat(120),
        ));
    let snapshot = DebugSnapshot::default();
    let control = DebugControl::new();
    let mut time = 0.0;
    let mut draw = |editor: &mut Editor, size, events| {
        time += 0.1;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default()
                    .frame(egui::Frame::NONE)
                    .show(ui, |ui| {
                        editor.workspace(ui, &snapshot, &control, false);
                    });
            },
        );
        output.drop_without_applying_deltas();
    };
    let size = egui::vec2(1000.0, 680.0);
    draw(&mut editor, size, vec![]);
    draw(&mut editor, size, vec![]);
    let divider = egui::Id::new(("ai-workspace-inspector", Page::Live)).with("divider");
    let before = context.read_response(divider).unwrap().rect.center();
    let after = before - egui::vec2(100.0, 0.0);
    let click = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    draw(
        &mut editor,
        size,
        vec![egui::Event::PointerMoved(before), click(before, true)],
    );
    draw(&mut editor, size, vec![egui::Event::PointerMoved(after)]);
    draw(&mut editor, size, vec![click(after, false)]);
    draw(&mut editor, size, vec![]);
    let adjusted = context.read_response(divider).unwrap().rect.center().x;
    assert!(
        adjusted < before.x - 50.0,
        "before {before:?}, adjusted {adjusted}"
    );
    let source_id = editor.source_id.unwrap();
    let scroll_id =
        egui::Id::new(("ai-source-pane", Page::Live)).with(egui::IdSalt::new("ai-source-scroll"));
    let mut scroll = egui::scroll_area::State::load(&context, scroll_id).unwrap();
    scroll.offset.y = 300.0;
    scroll.store(&context, scroll_id);
    draw(&mut editor, size, vec![]);
    let offset = egui::scroll_area::State::load(&context, scroll_id)
        .unwrap()
        .offset
        .y;
    assert!(offset > 200.0);
    editor.show_trace = false;
    draw(&mut editor, size, vec![]);
    assert!((context.read_response(divider).unwrap().rect.center().x - adjusted).abs() < 0.5);
    assert!(
        (egui::scroll_area::State::load(&context, scroll_id)
            .unwrap()
            .offset
            .y
            - offset)
            .abs()
            < 0.5
    );
    assert_eq!(editor.source_id, Some(source_id));

    context.memory_mut(|memory| memory.request_focus(source_id));
    draw(
        &mut editor,
        egui::vec2(420.0, 400.0),
        vec![egui::Event::Text("edited".into())],
    );
    assert_eq!(editor.source_id, Some(source_id));
    assert!(context.memory(|memory| memory.has_focus(source_id)));
    assert!(editor.draft.source.contains("edited"));
    let draft = editor.draft.source.clone();
    draw(&mut editor, size, vec![]);
    editor.show_trace = true;
    draw(&mut editor, size, vec![]);
    assert!((context.read_response(divider).unwrap().rect.center().x - adjusted).abs() < 0.5);
    assert_eq!(editor.source_id, Some(source_id));
    assert!(context.memory(|memory| memory.has_focus(source_id)));
    assert_eq!(editor.draft.source, draft);
}

#[test]
fn file_switches_and_apply_snapshot_preserve_all_edited_files() {
    use mhf_monster::ai::dsl::{Project, SourceFile};
    let mut project = Project::single(Some(31), 6, "entry".into());
    project.files.push(SourceFile {
        path: "common/6/combat.mhai".into(),
        source: "helper".into(),
    });
    let mut draft = Draft::default();
    draft.set_project(project);
    draft.source = "edited entry".into();
    draft.select_file(1);
    assert_eq!(draft.source, "helper");
    draft.source = "edited helper".into();
    let snapshot = draft.project_snapshot().unwrap();
    assert_eq!(snapshot.files[0].source, "edited entry");
    assert_eq!(snapshot.files[1].source, "edited helper");
    draft.set_project(snapshot);
    assert_eq!(draft.file, 1);
    draft.select_file(0);
    assert_eq!(draft.source, "edited entry");
    draft.select_file(1);
    assert_eq!(draft.source, "edited helper");
}

#[test]
fn highlighted_editor_preserves_breakpoint_gutter_and_blocks_pending_edits() {
    let target = TARGET;
    let context = egui::Context::default();
    egui_hunter::Theme::default().apply(&context);
    let control = DebugControl::new();
    let mut editor = Editor {
        selected: Some(target),
        draft: Draft {
            source: "fn main() {\n    restart;\n}\n".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    let markers = SourceMarkers {
        path: "test.mhai".into(),
        current_line: Some(2),
        breakpoint_lines: vec![2],
        interactive: true,
    };
    let breakpoint = egui::Id::new(("ai-source-breakpoint", "test.mhai", 2));
    let mut time = 0.0;
    let mut draw = |editor: &mut Editor, events| {
        time += 0.1;
        let output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(600.0, 400.0),
                )),
                time: Some(time),
                events,
                ..Default::default()
            },
            |ui| {
                let height = ui.available_height();
                editor.source_editor(ui, Some(&markers), None, &control, height);
            },
        );
        output.drop_without_applying_deltas();
    };
    for _ in 0..2 {
        draw(&mut editor, vec![]);
    }
    let gutter = context.read_response(breakpoint).unwrap();
    let source_id = editor.source_id.unwrap();
    let source = context.read_response(source_id).unwrap();
    assert!(gutter.rect.right() <= source.rect.left());
    assert!(gutter.rect.top() > source.rect.top());
    let click = |pos, pressed| egui::Event::PointerButton {
        pos,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    };
    let pos = gutter.rect.center();
    draw(
        &mut editor,
        vec![egui::Event::PointerMoved(pos), click(pos, true)],
    );
    draw(&mut editor, vec![click(pos, false)]);
    assert!(matches!(
        control.commands().as_slice(),
        [DebugCommand::AiDebug {
            target: actual,
            operation: AiDebugOperation::SourceBreakpoint { path, line: 2 },
        }] if *actual == target && path == "test.mhai"
    ));

    context.memory_mut(|memory| memory.request_focus(source_id));
    editor.pending = Some(Pending {
        request: 1,
        target,
        replacement: None,
        preserve_draft: false,
        attach: false,
    });
    let source = editor.draft.source.clone();
    draw(&mut editor, vec![egui::Event::Text("ignored".into())]);
    assert_eq!(editor.draft.source, source);
    assert!(!editor.draft.modified);
}

#[test]
fn paused_execution_keeps_the_focused_draft_and_follows_after_blur() {
    use crate::provider::AiDebugSnapshot;
    use mhf_monster::ai::dsl::{DebugInfo, Project, SourceFile, SourceLocation, SourceMapping};
    use std::sync::Arc;

    let target = TARGET;
    let mut project = Project::single(None, 6, "fn main() { restart; }".into());
    project.files.push(SourceFile {
        path: "combat.mhai".into(),
        source: "fn combat() { return; }".into(),
    });
    let info = DebugInfo {
        files: project.files.clone(),
        mappings: vec![SourceMapping {
            script: 0,
            start: 0,
            end: 2,
            source: SourceLocation {
                path: project.files[1].path.clone(),
                line: 1,
                ..Default::default()
            },
            generated: false,
        }],
    };
    let mut editor = Editor {
        selected: Some(target),
        ..Default::default()
    };
    editor.draft.set_project(project);
    let control = DebugControl::new();
    let context = egui::Context::default();
    let draw = |editor: &mut Editor, snapshot: &DebugSnapshot| {
        let output = context.run_ui(Default::default(), |ui| {
            editor.source_page(ui, snapshot, &control, true);
        });
        output.drop_without_applying_deltas();
    };
    draw(&mut editor, &DebugSnapshot::default());
    let source_id = editor.source_id.unwrap();
    context.memory_mut(|memory| memory.request_focus(source_id));
    let state = mhf_ai_debug::Snapshot {
        pc: Some(mhf_ai_debug::ProgramLocation {
            revision: 1,
            script: 0,
            offset: 0,
        }),
        ..Default::default()
    };
    let mut snapshot = DebugSnapshot {
        ai_debug: Some(Arc::new(AiDebugSnapshot {
            target,
            attached: true,
            paused: true,
            state: state.clone(),
            recording: mhf_ai_debug::Recording::empty(state),
            reason: String::new(),
            breakpoints: vec![],
            debug_info: info.into(),
        })),
        ..Default::default()
    };
    draw(&mut editor, &snapshot);
    assert_eq!(editor.draft.file, 0);
    assert!(context.memory(|memory| memory.has_focus(source_id)));

    context.memory_mut(|memory| memory.surrender_focus(source_id));
    Arc::make_mut(snapshot.ai_debug.as_mut().unwrap())
        .state
        .pc
        .as_mut()
        .unwrap()
        .offset = 1;
    draw(&mut editor, &snapshot);
    assert_eq!(editor.draft.file, 1);
    assert_eq!(editor.draft.source, "fn combat() { return; }");
}

#[test]
fn replacement_follows_the_requested_spawn_even_when_the_reload_reply_is_lost() {
    use crate::provider::{AiDocument, AiReply};
    use std::sync::Arc;
    let old = AiTarget {
        slot: 4,
        serial: 3,
        model: 0x2000,
        ..TARGET
    };
    let replacement = AiTarget {
        epoch: 2,
        pool: 0x3000,
        serial: 8,
        species: 94,
        ..old
    };
    let other = AiTarget {
        slot: 0,
        ..replacement
    };
    for deliver_reply in [false, true] {
        let control = DebugControl::new();
        let mut editor = Editor {
            selected: Some(old),
            draft: Draft {
                loaded: Some((old, 0x5000)),
                source: "unsaved old script".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        editor.submit(&control, AiOperation::ReplaceSpecies(94));
        control.commands();
        let mut snapshot = DebugSnapshot {
            ready: true,
            ai_targets: vec![old],
            ..Default::default()
        };
        if deliver_reply {
            snapshot.ai_reply = Some(Arc::new(AiReply {
                request: editor.request,
                target: old,
                result: Ok(AiDocument {
                    descriptor: 0,
                    source: None,
                }),
            }));
        }
        editor.sync_target(&snapshot, &control);
        assert_eq!(editor.selected, Some(old));
        assert!(control.commands().is_empty());
        snapshot.ready = false;
        snapshot.ai_targets.clear();
        editor.sync_target(&snapshot, &control);
        snapshot.ready = true;
        snapshot.ai_targets = vec![other];
        editor.sync_target(&snapshot, &control);
        assert_eq!(editor.selected, Some(old));
        assert!(control.commands().is_empty());
        snapshot.ai_targets.push(replacement);
        editor.sync_target(&snapshot, &control);
        assert_eq!(editor.selected, Some(replacement));
        assert!(editor.draft.source.is_empty());
        assert!(
            matches!(control.commands().as_slice(), [DebugCommand::MonsterAi { target, operation: AiOperation::Inspect, .. }] if *target == replacement)
        );
        assert!(editor.drafts.iter().any(
                |(target, draft)| *target == Some(old) && draft.source == "unsaved old script"
            ));
        editor.sync_target(&snapshot, &control);
        assert!(control.commands().is_empty());
    }
}

#[test]
fn rapid_target_changes_ignore_stale_replies_and_attach_after_source_refresh() {
    use crate::provider::{AiDebugSnapshot, AiDocument, AiReply};
    use std::sync::Arc;
    let first = AiTarget {
        slot: 1,
        model: 0x2000,
        ..TARGET
    };
    let second = AiTarget { slot: 2, ..first };
    let third = AiTarget { slot: 3, ..first };
    let control = DebugControl::new();
    let state = mhf_ai_debug::Snapshot::default();
    let mut snapshot = DebugSnapshot {
        ready: true,
        ai_targets: vec![first, second, third],
        ai_debug: Some(Arc::new(AiDebugSnapshot {
            target: first,
            attached: true,
            paused: true,
            state: state.clone(),
            recording: mhf_ai_debug::Recording::empty(state),
            reason: String::new(),
            breakpoints: vec![],
            debug_info: Default::default(),
        })),
        ..Default::default()
    };
    let mut editor = Editor {
        selected: Some(first),
        draft: Draft {
            loaded: Some((first, 0x3000)),
            source: "first unsaved".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    editor.select_target(Some(second), &snapshot, &control);
    let second_request = editor.request;
    assert!(
        matches!(control.commands().as_slice(), [DebugCommand::AiDebug { target: a, operation: AiDebugOperation::Detach }, DebugCommand::MonsterAi { target: b, operation: AiOperation::Inspect, .. }] if *a == first && *b == second)
    );
    editor.select_target(Some(third), &snapshot, &control);
    let third_request = editor.request;
    control.commands();
    let reply = |request, target, source: &str| {
        Arc::new(AiReply {
            request,
            target,
            result: Ok(AiDocument {
                descriptor: 0x9000,
                source: Some(mhf_monster::ai::dsl::Project::single(
                    None,
                    target.species,
                    source.into(),
                )),
            }),
        })
    };
    snapshot.ai_reply = Some(reply(second_request, second, "second script"));
    editor.sync_target(&snapshot, &control);
    assert_eq!(editor.selected, Some(third));
    assert!(editor.draft.source.is_empty());
    assert!(control.commands().is_empty());
    snapshot.ai_reply = Some(reply(third_request, third, "third script"));
    editor.sync_target(&snapshot, &control);
    assert_eq!(editor.draft.source, "third script");
    assert_eq!(editor.draft.loaded, Some((third, 0x9000)));
    assert!(
        matches!(control.commands().as_slice(), [DebugCommand::AiDebug { target, operation: AiDebugOperation::Attach }] if *target == third)
    );
    editor.select_target(Some(first), &snapshot, &control);
    control.commands();
    snapshot.ai_reply = Some(reply(editor.request, first, "first refreshed"));
    editor.sync_target(&snapshot, &control);
    assert_eq!(editor.draft.source, "first unsaved");
    assert_eq!(editor.draft.loaded, Some((first, 0x9000)));
    editor.select_target(Some(third), &snapshot, &control);
    control.commands();
    snapshot.ai_reply = Some(reply(editor.request, third, "third refreshed"));
    editor.sync_target(&snapshot, &control);
    assert_eq!(
        editor.draft.source, "third refreshed",
        "unmodified cached scripts must refresh"
    );
}

#[test]
fn switching_instances_restores_their_own_drafts_and_descriptors() {
    let first = TARGET;
    let second = AiTarget { slot: 1, ..first };
    let control = DebugControl::new();
    let mut editor = Editor {
        selected: Some(first),
        draft: Draft {
            loaded: Some((first, 0x2000)),
            source: "unsaved first draft".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    editor.select_target(Some(second), &DebugSnapshot::default(), &control);
    assert!(
        matches!(control.commands().as_slice(), [DebugCommand::MonsterAi { target, operation: AiOperation::Inspect, .. }] if *target == second)
    );
    assert!(editor.draft.source.is_empty());
    assert!(editor.draft.loaded.is_none());
    editor.pending = None;
    editor.draft.loaded = Some((second, 0x3000));
    editor.draft.source = "second draft".into();
    editor.select_target(Some(first), &DebugSnapshot::default(), &control);
    assert_eq!(editor.draft.source, "unsaved first draft");
    assert_eq!(editor.draft.loaded, Some((first, 0x2000)));
    assert!(matches!(
        control.commands().as_slice(),
        [DebugCommand::MonsterAi {
            operation: AiOperation::Inspect,
            ..
        }]
    ));
    editor.select_target(Some(second), &DebugSnapshot::default(), &control);
    assert_eq!(editor.draft.source, "second draft");
    assert_eq!(editor.draft.loaded, Some((second, 0x3000)));
    assert!(matches!(
        control.commands().as_slice(),
        [DebugCommand::MonsterAi {
            operation: AiOperation::Inspect,
            ..
        }]
    ));
}
