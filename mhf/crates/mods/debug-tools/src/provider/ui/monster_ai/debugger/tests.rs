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
fn reused_instance_cannot_show_or_control_previous_debug_session() {
    let debug = fixture();
    let target = debug.target;
    let snapshot = DebugSnapshot {
        ai_debug: Some(std::sync::Arc::new(debug)),
        ..Default::default()
    };
    assert!(session(&snapshot, Some(target)).is_some());
    assert!(
        session(
            &snapshot,
            Some(AiTarget {
                serial: target.serial + 1,
                ..target
            })
        )
        .is_none()
    );
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
fn live_controls_send_commands_for_the_exact_instance_and_disable_stale_targets() {
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect, Shape};
    let debug = fixture();
    let target = debug.target;
    let snapshot = DebugSnapshot {
        ready: true,
        ai_targets: vec![target],
        ai_debug: Some(std::sync::Arc::new(debug)),
        ..Default::default()
    };
    let control = DebugControl::new();
    let context = egui::Context::default();
    let mut debugger = DebuggerUi::default();
    let mut draw = |events: Vec<Event>, active: bool| {
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(340.0, 650.0))),
                events,
                ..Default::default()
            },
            |ui| {
                debugger.controls(
                    ui,
                    &control,
                    Some(target),
                    active,
                    session(&snapshot, Some(target)),
                )
            },
        );
        let labels = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                Shape::Text(text) => Some((
                    text.galley.job.text.clone(),
                    text.galley.rect.translate(text.pos.to_vec2()).center(),
                )),
                _ => None,
            })
            .collect::<Vec<_>>();
        output.drop_without_applying_deltas();
        labels
    };
    for label in ["继续", "单步", "至让出"] {
        draw(vec![], true);
        let labels = draw(vec![], true);
        let position = labels.iter().find(|(text, _)| text == label).unwrap().1;
        for pressed in [true, false] {
            draw(
                vec![
                    Event::PointerMoved(position),
                    Event::PointerButton {
                        pos: position,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    },
                ],
                true,
            );
        }
    }
    let commands = control.commands();
    assert!(
        matches!(commands.as_slice(), [DebugCommand::AiDebug { target: a, operation: AiDebugOperation::Continue }, DebugCommand::AiDebug { target: b, operation: AiDebugOperation::StepInstruction }, DebugCommand::AiDebug { target: c, operation: AiDebugOperation::RunUntilYield }] if *a == target && *b == target && *c == target)
    );
    draw(vec![], false);
    let position = draw(vec![], false)
        .iter()
        .find(|(text, _)| text == "单步")
        .unwrap()
        .1;
    for pressed in [true, false] {
        draw(
            vec![
                Event::PointerMoved(position),
                Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                },
            ],
            false,
        );
    }
    assert!(control.commands().is_empty());
}

#[test]
fn replay_trace_selection_keeps_cursor_state_and_next_instruction_consistent() {
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
    let recording = trace.recording().unwrap();
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
        draw(vec![
            egui::Event::PointerMoved(position),
            egui::Event::PointerButton {
                pos: position,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
    draw(vec![]);
    assert_eq!(debugger.replay.as_ref().unwrap().position(), 0);
    assert_eq!(debugger.replay.as_ref().unwrap().snapshot(), &initial);
    assert_eq!(debugger.selected_event, Some(first));
}

#[test]
fn detached_capture_is_labeled_archived_and_preserves_offline_replay() {
    let mut debug = fixture();
    debug.attached = false;
    let target = debug.target;
    let mut ui = DebuggerUi::default();
    ui.load_recording(debug.recording.clone());
    let loaded = ui.replay.as_ref().unwrap().recording().clone();
    let snapshot = DebugSnapshot {
        ai_debug: Some(std::sync::Arc::new(debug)),
        ..Default::default()
    };
    let context = egui::Context::default();
    let output = context.run_ui(egui::RawInput::default(), |root| {
        ui.recording_actions(
            root,
            &DebugControl::new(),
            Some(target),
            false,
            session(&snapshot, Some(target)),
        )
    });
    let labels = output
        .shapes
        .iter()
        .filter_map(|shape| match &shape.shape {
            egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>();
    assert!(!labels.contains(&"AI 已暂停"));
    assert!(labels.contains(&"保存录制…"));
    assert_eq!(ui.replay.as_ref().unwrap().recording(), &loaded);
    output.drop_without_applying_deltas();
}
