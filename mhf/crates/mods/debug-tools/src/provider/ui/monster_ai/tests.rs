use super::*;

#[test]
fn status_hud_tracks_exact_instance_and_does_not_capture_pointer() {
    let target = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 2,
        serial: 7,
        model: 0,
        species: 6,
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
            |ui| editor.show_hud(ui.ctx(), snapshot),
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
fn compact_workspace_keeps_primary_controls_and_source_above_the_fold() {
    let target = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 1,
        serial: 1,
        model: 0x2000,
        species: 6,
    };
    for width in [340.0, 960.0] {
        let context = egui::Context::default();
        egui_hunter::Theme::default().apply(&context);
        let mut editor = Editor {
            selected: Some(target),
            ..Default::default()
        };
        editor.draft.loaded = Some((target, 0x2000));
        editor
            .draft
            .set_project(mhf_monster::ai::dsl::Project::single(
                None,
                6,
                "mhf_ai 1; species 6; base native; fn main() { restart; }".into(),
            ));
        let snapshot = DebugSnapshot {
            ready: true,
            ai_targets: vec![target],
            ..Default::default()
        };
        let control = DebugControl::new();
        for _ in 0..3 {
            let output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(width, 760.0),
                    )),
                    ..Default::default()
                },
                |ui| editor.show(ui, &snapshot, &control),
            );
            for label in ["更多", "附加", "单步", "应用更改"] {
                let bounds = output
                    .shapes
                    .iter()
                    .find_map(|shape| match &shape.shape {
                        egui::Shape::Text(text) if text.galley.job.text == label => {
                            Some(text.galley.rect.translate(text.pos.to_vec2()))
                        }
                        _ => None,
                    })
                    .unwrap_or_else(|| panic!("missing {label} at {width}"));
                assert!(
                    bounds.right() <= width && bounds.bottom() < 130.0,
                    "{label} at {width}: {bounds:?}"
                );
            }
            let source = context.read_response(egui::Id::new("ai-source")).unwrap();
            assert!(
                source.rect.top() < 210.0,
                "source at {width}: {:?}",
                source.rect
            );
            output.drop_without_applying_deltas();
        }
    }
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
fn replacement_follows_the_requested_spawn_even_when_the_reload_reply_is_lost() {
    use crate::provider::{AiDocument, AiReply};
    use std::sync::Arc;
    let old = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 4,
        serial: 3,
        model: 0x2000,
        species: 6,
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
                    message: "loading".into(),
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
        epoch: 1,
        pool: 0x1000,
        slot: 1,
        serial: 1,
        model: 0x2000,
        species: 6,
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
                message: String::new(),
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
    let first = AiTarget {
        epoch: 1,
        pool: 0x1000,
        slot: 0,
        serial: 1,
        model: 0,
        species: 6,
    };
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
