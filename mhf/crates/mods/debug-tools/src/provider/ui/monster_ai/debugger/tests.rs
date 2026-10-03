use super::*;
use mhf_ai_debug::{BreakpointKind, InstanceId, ProgramLocation, Recording, Snapshot};

fn fixture() -> AiDebugSnapshot {
    let state = Snapshot {
        instance: InstanceId {
            session: 3,
            slot: 4,
            generation: 9,
        },
        pc: Some(ProgramLocation {
            revision: 7,
            script: 2,
            offset: 12,
        }),
        frame: 42,
        fields: [("ai_state".into(), 2)].into_iter().collect(),
    };
    AiDebugSnapshot {
        target: AiTarget {
            epoch: 3,
            pool: 0x1000,
            slot: 4,
            serial: 9,
            model: 0x2000,
            species: 6,
        },
        attached: true,
        state: state.clone(),
        paused: true,
        reason: String::new(),
        recording: Recording::empty(state),
        breakpoints: Vec::new(),
        debug_info: Default::default(),
    }
}

#[test]
fn address_breakpoints_keep_program_revision_and_validate_ranges() {
    let debug = fixture();
    let mut ui = DebuggerUi {
        breakpoint_script: "2".into(),
        breakpoint_offset: "0x10".into(),
        ..Default::default()
    };
    let breakpoint = ui.make_breakpoint(&debug).unwrap();
    assert_eq!(
        breakpoint.kind,
        BreakpointKind::Location(ProgramLocation {
            revision: 7,
            script: 2,
            offset: 16
        })
    );
    ui.breakpoint_script = "4294967296".into();
    assert!(ui.make_breakpoint(&debug).is_err());
    ui.breakpoint_kind = 1;
    ui.breakpoint_opcode = "0x100".into();
    assert!(ui.make_breakpoint(&debug).is_err());
}

#[test]
fn condition_breakpoints_only_use_captured_fields() {
    let debug = fixture();
    let mut ui = DebuggerUi {
        breakpoint_kind: 1,
        breakpoint_opcode: "5".into(),
        breakpoint_condition: true,
        breakpoint_field: "unknown".into(),
        breakpoint_value: "2".into(),
        ..Default::default()
    };
    assert!(ui.make_breakpoint(&debug).is_err());
    ui.breakpoint_field = "ai_state".into();
    let breakpoint = ui.make_breakpoint(&debug).unwrap();
    assert!(breakpoint.condition.unwrap().matches(&debug.state));
}

#[test]
fn invalid_import_preserves_the_loaded_replay() {
    let recording = fixture().recording;
    let mut ui = DebuggerUi::default();
    ui.load_recording(recording.clone());
    let loaded = ui.replay.as_ref().unwrap().recording().clone();
    let mut invalid = recording.clone();
    invalid.version += 1;
    ui.load_recording(invalid);
    assert!(ui.error.is_some());
    assert_eq!(ui.replay.unwrap().recording(), &loaded);
}

#[test]
fn live_control_icons_preserve_exact_targets_state_tooltips_and_disabled_input() {
    let mut debug = fixture();
    let target = debug.target;
    let control = DebugControl::new();
    let context = egui::Context::default();
    mhf_font::install(&context);
    egui_hunter::Theme::default()
        .density(egui_hunter::Density::Compact)
        .apply(&context);
    let mut debugger = DebuggerUi::new();
    let id = egui::Id::new("ai-attach");
    let mut draw = |attached, active, events| {
        debug.attached = attached;
        let output = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.horizontal(|ui| {
                    debugger.controls(ui, &control, Some(target), active, Some(&debug))
                });
            },
        );
        let expected = if attached {
            "分离调试器"
        } else {
            "附加调试器"
        };
        let tooltip = output.shapes.iter().any(|shape| {
            matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.job.text == expected)
        });
        for event in &output.platform_output.events {
            let info = event.widget_info();
            let (role, selected) = match info.label.as_deref() {
                Some("附加调试器" | "分离调试器") => {
                    (egui::WidgetType::SelectableLabel, Some(attached))
                }
                Some("暂停" | "继续" | "单步" | "至让出") => {
                    (egui::WidgetType::Button, None)
                }
                _ => continue,
            };
            assert_eq!(info.typ, role);
            assert_eq!(info.selected, selected);
        }
        output.drop_without_applying_deltas();
        (context.read_response(id).unwrap().rect, tooltip)
    };
    let mut original = None;
    for attached in [false, true] {
        let (rect, _) = draw(attached, true, Vec::new());
        if let Some(original) = original {
            assert_eq!(
                rect, original,
                "changing attachment state must not move the toggle"
            );
        }
        original = Some(rect);
        for pressed in [true, false] {
            draw(attached, true, pointer_click(rect.center(), pressed));
        }
        assert!(
            draw(attached, true, Vec::new()).1,
            "focused toggle must identify its current action"
        );
        assert!(context.read_response(egui::Id::new("ai-detach")).is_none());
    }
    assert!(matches!(control.commands().as_slice(), [
        DebugCommand::AiDebug { target: a, operation: AiDebugOperation::Attach },
        DebugCommand::AiDebug { target: b, operation: AiDebugOperation::Detach },
    ] if *a == target && *b == target));
    for id in ["ai-pause", "ai-step", "ai-run-yield"] {
        draw(true, true, Vec::new());
        let position = context
            .read_response(egui::Id::new(id))
            .unwrap()
            .rect
            .center();
        for pressed in [true, false] {
            draw(true, true, pointer_click(position, pressed));
        }
    }
    assert!(matches!(control.commands().as_slice(), [
        DebugCommand::AiDebug { target: a, operation: AiDebugOperation::Continue },
        DebugCommand::AiDebug { target: b, operation: AiDebugOperation::StepInstruction },
        DebugCommand::AiDebug { target: c, operation: AiDebugOperation::RunUntilYield },
    ] if *a == target && *b == target && *c == target));
    for attached in [false, true] {
        for id in ["ai-attach", "ai-pause", "ai-step", "ai-run-yield"] {
            draw(attached, false, Vec::new());
            draw(attached, false, Vec::new());
            let response = context.read_response(egui::Id::new(id)).unwrap();
            assert!(!response.enabled());
            for pressed in [true, false] {
                draw(
                    attached,
                    false,
                    pointer_click(response.rect.center(), pressed),
                );
            }
        }
    }
    assert!(control.commands().is_empty());
}

fn inspector_frame(
    context: &egui::Context,
    debugger: &mut DebuggerUi,
    debug: &AiDebugSnapshot,
    control: &DebugControl,
    target: AiTarget,
    active: bool,
    events: Vec<egui::Event>,
) -> (Option<egui::Response>, Vec<String>) {
    let snapshot = DebugSnapshot {
        ai_debug: Some(std::sync::Arc::new(debug.clone())),
        ..Default::default()
    };
    let mut response = None;
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(340.0, 650.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            debugger.inspect(ui, &snapshot, Some(target), false, control, active);
            response = context.read_response(egui::Id::new(("ai-field-breakpoint", "ai_state")));
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    output.drop_without_applying_deltas();
    (response, texts)
}

fn pointer_click(position: egui::Pos2, pressed: bool) -> Vec<egui::Event> {
    vec![
        egui::Event::PointerMoved(position),
        egui::Event::PointerButton {
            pos: position,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[test]
fn field_breakpoint_icon_removes_disabled_breakpoints_and_adds_for_the_exact_instance() {
    let context = egui::Context::default();
    mhf_font::install(&context);
    egui_hunter::Theme::default()
        .density(egui_hunter::Density::Compact)
        .apply(&context);
    context.all_styles_mut(|style| style.interaction.tooltip_delay = 0.0);
    let control = DebugControl::new();
    let mut debugger = DebuggerUi::new();
    let mut debug = fixture();
    let unrelated = mhf_ai_debug::Breakpoint {
        id: 8,
        enabled: true,
        kind: BreakpointKind::Opcode(5),
        condition: None,
    };
    debug.breakpoints = vec![
        mhf_ai_debug::Breakpoint {
            id: 3,
            enabled: false,
            kind: BreakpointKind::FieldChanged("ai_state".into()),
            condition: None,
        },
        unrelated.clone(),
    ];
    let mut draw = |debug: &AiDebugSnapshot, events| {
        inspector_frame(
            &context,
            &mut debugger,
            debug,
            &control,
            debug.target,
            true,
            events,
        )
    };
    let (initial, _) = draw(&debug, vec![]);
    let initial = initial.unwrap();
    assert!(initial.enabled());
    let (hovered, _) = draw(
        &debug,
        vec![egui::Event::PointerMoved(initial.rect.center())],
    );
    assert!(hovered.unwrap().hovered());
    draw(&debug, vec![]);
    let (hovered, texts) = draw(&debug, vec![]);
    assert_eq!(hovered.unwrap().rect, initial.rect);
    assert!(texts.iter().any(|text| text == "移除字段断点"));
    for pressed in [true, false] {
        draw(&debug, pointer_click(initial.rect.center(), pressed));
    }
    let commands = control.commands();
    let [
        DebugCommand::AiDebug {
            target,
            operation: AiDebugOperation::SetBreakpoints(breakpoints),
        },
    ] = commands.as_slice()
    else {
        panic!("field breakpoint click must send one replacement set");
    };
    assert_eq!(*target, debug.target);
    assert_eq!(breakpoints, std::slice::from_ref(&unrelated));
    debug.breakpoints = breakpoints.clone();
    let (response, texts) = draw(&debug, vec![]);
    assert_eq!(response.unwrap().rect, initial.rect);
    assert!(texts.iter().any(|text| text == "添加字段断点"));
    for pressed in [true, false] {
        draw(&debug, pointer_click(initial.rect.center(), pressed));
    }
    let commands = control.commands();
    let [
        DebugCommand::AiDebug {
            target,
            operation: AiDebugOperation::SetBreakpoints(breakpoints),
        },
    ] = commands.as_slice()
    else {
        panic!("field breakpoint click must create a replacement set");
    };
    assert_eq!(*target, debug.target);
    assert_eq!(
        breakpoints,
        &[
            unrelated,
            mhf_ai_debug::Breakpoint {
                id: 9,
                enabled: true,
                kind: BreakpointKind::FieldChanged("ai_state".into()),
                condition: None,
            },
        ]
    );
    assert!(debugger.error.is_none());
}

#[test]
fn field_breakpoint_icon_disables_pending_and_detached_instances_and_rejects_reused_targets() {
    for (attached, active) in [(true, false), (false, true)] {
        let context = egui::Context::default();
        let control = DebugControl::new();
        let mut debugger = DebuggerUi::new();
        let mut debug = fixture();
        debug.attached = attached;
        let response = inspector_frame(
            &context,
            &mut debugger,
            &debug,
            &control,
            debug.target,
            active,
            vec![],
        )
        .0
        .unwrap();
        assert!(!response.enabled());
        for pressed in [true, false] {
            inspector_frame(
                &context,
                &mut debugger,
                &debug,
                &control,
                debug.target,
                active,
                pointer_click(response.rect.center(), pressed),
            );
        }
        assert!(control.commands().is_empty());
    }
    let context = egui::Context::default();
    let control = DebugControl::new();
    let mut debugger = DebuggerUi::new();
    let debug = fixture();
    let reused = AiTarget {
        serial: debug.target.serial + 1,
        ..debug.target
    };
    let (response, _) = inspector_frame(
        &context,
        &mut debugger,
        &debug,
        &control,
        reused,
        true,
        vec![],
    );
    assert!(response.is_none());
    assert!(control.commands().is_empty());
}

fn two_step_recording() -> Recording {
    let mut state = fixture().state;
    let mut trace = mhf_ai_debug::TraceBuffer::default();
    for value in [3, 4] {
        let before = state.clone();
        state.fields.insert("ai_state".into(), value);
        state.pc.as_mut().unwrap().offset += 1;
        trace
            .push(mhf_ai_debug::Transition {
                before,
                after: state.clone(),
                opcode: 5,
                operands: vec![],
                outcome: mhf_ai_debug::Outcome::Continue,
                note: String::new(),
            })
            .unwrap();
    }
    trace.recording().unwrap()
}

fn recording_actions_frame(
    context: &egui::Context,
    debugger: &mut DebuggerUi,
    snapshot: &DebugSnapshot,
    control: &DebugControl,
    replay: bool,
    events: Vec<egui::Event>,
) -> Vec<String> {
    let target = snapshot.ai_debug.as_ref().map(|debug| debug.target);
    let output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(640.0, 480.0),
            )),
            events,
            ..Default::default()
        },
        |ui| {
            ui.horizontal(|ui| {
                if replay {
                    debugger.replay_actions(ui, snapshot, target);
                } else {
                    debugger.recording_actions(
                        ui,
                        control,
                        target,
                        false,
                        session(snapshot, target),
                    );
                }
            });
            debugger.import_dialog(ui.ctx());
        },
    );
    let texts = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.clone()),
            _ => None,
        })
        .collect();
    output.drop_without_applying_deltas();
    texts
}

#[test]
fn live_and_replay_save_queue_json_once_without_opening_the_import_dialog() {
    let recording = two_step_recording();
    let expected_json = recording.to_json().unwrap();
    for replay in [false, true] {
        let mut debug = fixture();
        debug.attached = false;
        debug.recording = recording.clone();
        let snapshot = DebugSnapshot {
            ai_debug: Some(std::sync::Arc::new(debug)),
            ..Default::default()
        };
        let context = egui::Context::default();
        let control = DebugControl::new();
        let mut debugger = DebuggerUi::new();
        if replay {
            debugger.load_recording(recording.clone());
        }
        let save_id = if replay {
            "ai-replay-save"
        } else {
            "ai-recording-save"
        };
        for _ in 0..2 {
            recording_actions_frame(&context, &mut debugger, &snapshot, &control, replay, vec![]);
        }
        let response = context.read_response(egui::Id::new(save_id)).unwrap();
        assert!(response.enabled(), "{save_id}");
        for pressed in [true, false] {
            let texts = recording_actions_frame(
                &context,
                &mut debugger,
                &snapshot,
                &control,
                replay,
                pointer_click(response.rect.center(), pressed),
            );
            assert!(
                !texts
                    .iter()
                    .any(|text| text == "录制文件路径" || text == "保存文件")
            );
        }
        assert!(!debugger.import_open);
        let json = debugger.take_recording_save().unwrap();
        assert_eq!(json, expected_json);
        assert_eq!(Recording::from_json(&json).unwrap(), recording);
        assert!(debugger.take_recording_save().is_none());
        if replay {
            assert_eq!(debugger.replay.as_ref().unwrap().recording(), &recording);
        } else {
            assert!(
                !context
                    .read_response(egui::Id::new("ai-trace-clear"))
                    .unwrap()
                    .enabled()
            );
        }
        assert!(control.commands().is_empty());
    }
}

#[test]
fn recording_save_results_handle_cancel_success_and_failure_and_keep_import_separate() {
    let mut debugger = DebuggerUi {
        error: Some("原有错误".into()),
        ..DebuggerUi::new()
    };
    debugger.recording_save_finished(Ok(false));
    assert_eq!(debugger.error.as_deref(), Some("原有错误"));
    debugger.recording_save_finished(Err("保存失败：磁盘不可写".into()));
    assert_eq!(debugger.error.as_deref(), Some("保存失败：磁盘不可写"));
    debugger.recording_save_finished(Ok(true));
    assert!(debugger.error.is_none());
    debugger.recording_save_finished(Ok(false));
    assert!(debugger.error.is_none());

    let mut invalid = fixture().recording;
    invalid.version += 1;
    debugger.request_recording_save(invalid.to_json());
    assert!(debugger.error.is_some());
    assert!(debugger.take_recording_save().is_none());
    assert!(!debugger.import_open);

    let context = egui::Context::default();
    let control = DebugControl::new();
    let snapshot = DebugSnapshot::default();
    for replay in [false, true] {
        recording_actions_frame(&context, &mut debugger, &snapshot, &control, replay, vec![]);
        let id = if replay {
            "ai-replay-save"
        } else {
            "ai-recording-save"
        };
        assert!(!context.read_response(egui::Id::new(id)).unwrap().enabled());
    }
    let point = context
        .read_response(egui::Id::new("ai-replay-import"))
        .unwrap()
        .rect
        .center();
    for pressed in [true, false] {
        recording_actions_frame(
            &context,
            &mut debugger,
            &snapshot,
            &control,
            true,
            pointer_click(point, pressed),
        );
    }
    let texts = recording_actions_frame(&context, &mut debugger, &snapshot, &control, true, vec![]);
    assert!(debugger.import_open);
    for label in [
        "导入录制",
        "录制文件路径",
        "打开文件",
        "粘贴录制 JSON",
        "验证并导入",
    ] {
        assert!(texts.iter().any(|text| text == label), "{label}");
    }
    assert!(!texts.iter().any(|text| text == "保存文件"));
    assert!(debugger.take_recording_save().is_none());
}

#[test]
fn replay_trace_selection_keeps_cursor_state_and_next_instruction_consistent() {
    let recording = two_step_recording();
    let first = recording.entries[0].sequence;
    let initial = recording.initial.clone();
    let mut debugger = DebuggerUi::new();
    debugger.load_recording(recording);
    debugger.replay.as_mut().unwrap().seek(2).unwrap();
    let context = egui::Context::default();
    let mut draw = |events| {
        let output = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                debugger.replay_controls(ui);
                debugger.replay_trace(ui);
            },
        );
        let position = output.shapes.iter().find_map(|shape| match &shape.shape {
            egui::Shape::Text(text) if text.galley.job.text.starts_with(&format!("#{first}")) => {
                Some(text.galley.rect.translate(text.pos.to_vec2()).center())
            }
            _ => None,
        });
        output.drop_without_applying_deltas();
        position
    };
    draw(vec![]);
    let position = draw(vec![]).unwrap();
    for pressed in [true, false] {
        draw(pointer_click(position, pressed));
    }
    draw(vec![]);
    assert_eq!(debugger.replay.as_ref().unwrap().position(), 0);
    assert_eq!(debugger.replay.as_ref().unwrap().snapshot(), &initial);
    assert_eq!(debugger.selected_event, Some(first));
}

#[test]
fn replay_icons_move_and_reset_the_recorded_state_and_disable_at_boundaries() {
    fn frame(context: &egui::Context, debugger: &mut DebuggerUi, events: Vec<egui::Event>) {
        let output = context.run_ui(
            egui::RawInput {
                events,
                ..Default::default()
            },
            |ui| {
                ui.horizontal(|ui| debugger.replay_controls(ui));
            },
        );
        output.drop_without_applying_deltas();
    }
    let context = egui::Context::default();
    let mut debugger = DebuggerUi::new();
    debugger.load_recording(two_step_recording());
    for (id, position, value) in [
        ("ai-replay-next", 1, 3),
        ("ai-replay-previous", 0, 2),
        ("ai-replay-next", 1, 3),
        ("ai-replay-next", 2, 4),
        ("ai-replay-start", 0, 2),
    ] {
        for _ in 0..2 {
            frame(&context, &mut debugger, Vec::new());
        }
        let response = context.read_response(egui::Id::new(id)).unwrap();
        assert!(response.enabled());
        for pressed in [true, false] {
            frame(
                &context,
                &mut debugger,
                pointer_click(response.rect.center(), pressed),
            );
        }
        let replay = debugger.replay.as_ref().unwrap();
        assert_eq!(replay.position(), position);
        assert_eq!(replay.snapshot().fields["ai_state"], value);
        assert_eq!(replay.snapshot().pc.unwrap().offset, 12 + position as u32);
    }
    for _ in 0..2 {
        frame(&context, &mut debugger, Vec::new());
    }
    assert!(
        !context
            .read_response(egui::Id::new("ai-replay-previous"))
            .unwrap()
            .enabled()
    );
    assert!(
        !context
            .read_response(egui::Id::new("ai-replay-start"))
            .unwrap()
            .enabled()
    );
    assert!(debugger.error.is_none());
}
