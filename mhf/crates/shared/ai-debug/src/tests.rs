use super::*;

fn snapshot(offset: u32, value: i64) -> Snapshot {
    Snapshot {
        instance: InstanceId {
            session: 9,
            slot: 2,
            generation: 4,
        },
        pc: Some(ProgramLocation {
            revision: 7,
            script: 3,
            offset,
        }),
        frame: 10,
        fields: BTreeMap::from([("request".into(), value)]),
    }
}

fn transition(offset: u32) -> Transition {
    Transition {
        before: snapshot(offset, i64::from(offset)),
        after: snapshot(offset + 1, i64::from(offset + 1)),
        opcode: 0x04,
        operands: vec![],
        outcome: Outcome::Continue,
        note: String::new(),
    }
}

fn trace(count: u32) -> TraceBuffer {
    let mut trace = TraceBuffer::new(2048);
    for offset in 0..count {
        trace.push(transition(offset)).unwrap();
    }
    trace
}

#[test]
fn continue_bypasses_current_breakpoint_once_but_stops_on_next_visit() {
    let mut debugger = Debugger::default();
    let state = snapshot(0, 0);
    let id = debugger.add_breakpoint(BreakpointKind::Location(state.pc.unwrap()), None);
    assert!(!debugger.before_instruction(&state, 0x04));
    assert_eq!(debugger.stop_reason(), Some(&StopReason::Breakpoint(id)));
    debugger.resume();
    assert!(debugger.before_instruction(&state, 0x04));
    debugger.after_instruction(transition(0)).unwrap();
    assert!(!debugger.before_instruction(&state, 0x04));
}

#[test]
fn resume_does_not_skip_a_different_boundary() {
    let mut debugger = Debugger::default();
    debugger.add_breakpoint(BreakpointKind::Opcode(0x04), None);
    assert!(!debugger.before_instruction(&snapshot(0, 0), 0x04));
    debugger.resume();
    assert!(!debugger.before_instruction(&snapshot(1, 1), 0x04));
}

#[test]
fn conditional_breakpoints_read_captured_values_only() {
    let mut debugger = Debugger::default();
    debugger.add_breakpoint(
        BreakpointKind::Opcode(0x04),
        Some(FieldPredicate {
            field: "request".into(),
            comparison: Comparison::Greater,
            value: 4,
        }),
    );
    assert!(debugger.before_instruction(&snapshot(0, 4), 0x04));
    assert!(!debugger.before_instruction(&snapshot(0, 5), 0x04));
    let missing = FieldPredicate {
        field: "missing".into(),
        comparison: Comparison::NotEqual,
        value: 0,
    };
    assert!(!missing.matches(&snapshot(0, 4)));
}

#[test]
fn watchpoint_stops_after_write_and_does_not_bypass_next_instruction_breakpoint() {
    let mut debugger = Debugger::default();
    let watch = debugger.add_breakpoint(BreakpointKind::FieldChanged("request".into()), None);
    let next = debugger.add_breakpoint(BreakpointKind::Location(snapshot(1, 1).pc.unwrap()), None);
    assert!(debugger.before_instruction(&snapshot(0, 0), 0x04));
    debugger.after_instruction(transition(0)).unwrap();
    assert_eq!(debugger.stop_reason(), Some(&StopReason::Breakpoint(watch)));
    debugger.resume();
    assert!(!debugger.before_instruction(&snapshot(1, 1), 0x04));
    assert_eq!(debugger.stop_reason(), Some(&StopReason::Breakpoint(next)));
}

#[test]
fn external_field_watchpoint_stops_before_dispatch_and_records_input_after_resume() {
    let mut debugger = Debugger::default();
    let watch = debugger.add_breakpoint(
        BreakpointKind::FieldChanged("lane".into()),
        Some(FieldPredicate {
            field: "lane".into(),
            comparison: Comparison::Equal,
            value: 2,
        }),
    );
    let mut first = transition(0);
    first.before.fields.insert("lane".into(), 0);
    first.after.fields.insert("lane".into(), 0);
    debugger.after_instruction(first).unwrap();

    let mut event = transition(1);
    event.before.frame = 11;
    event.after.frame = 11;
    event.before.pc.as_mut().unwrap().script = 4;
    event.after.pc.as_mut().unwrap().script = 4;
    event.before.fields.insert("lane".into(), 2);
    event.after.fields.insert("lane".into(), 2);
    assert!(!debugger.before_instruction(&event.before, event.opcode));
    assert_eq!(debugger.stop_reason(), Some(&StopReason::Breakpoint(watch)));
    assert_eq!(debugger.trace.entries().len(), 1);

    debugger.resume();
    assert!(debugger.before_instruction(&event.before, event.opcode));
    let after = event.after.clone();
    debugger.after_instruction(event).unwrap();
    assert_eq!(debugger.mode(), RunMode::Running);
    let entry = debugger.trace.entries().back().unwrap();
    assert_eq!(
        entry.input.fields,
        [FieldChange {
            field: "lane".into(),
            before: Some(0),
            after: Some(2)
        }]
    );
    assert!(!entry.changes.iter().any(|change| change.field == "lane"));
    assert!(debugger.before_instruction(&after, entry.opcode));
    let mut replay = ReplaySession::new(debugger.trace.recording().unwrap()).unwrap();
    replay.seek(2).unwrap();
    assert_eq!(replay.snapshot(), &after);
}

#[test]
fn external_watchpoints_need_a_same_instance_baseline() {
    let mut debugger = Debugger::default();
    debugger.add_breakpoint(BreakpointKind::FieldChanged("request".into()), None);
    assert!(debugger.before_instruction(&snapshot(0, 5), 0x04));
    debugger.after_instruction(transition(0)).unwrap();
    debugger.resume();
    let mut other = snapshot(1, 99);
    other.instance.generation += 1;
    assert!(debugger.before_instruction(&other, 0x04));
}

#[test]
fn stepping_and_until_yield_enforce_budgets() {
    let mut debugger = Debugger::default();
    debugger.add_breakpoint(BreakpointKind::Opcode(0x04), None);
    debugger.step();
    assert!(debugger.before_instruction(&snapshot(0, 0), 0x04));
    debugger.after_instruction(transition(0)).unwrap();
    assert_eq!(debugger.stop_reason(), Some(&StopReason::StepComplete));
    debugger.set_breakpoints(vec![]).unwrap();
    debugger.run_until_yield(2);
    debugger.after_instruction(transition(1)).unwrap();
    assert_eq!(debugger.mode(), RunMode::UntilYield);
    debugger.after_instruction(transition(2)).unwrap();
    assert_eq!(debugger.stop_reason(), Some(&StopReason::BudgetExhausted));
    debugger.run_until_yield(10);
    let mut yielded = transition(3);
    yielded.outcome = Outcome::Yield;
    debugger.after_instruction(yielded).unwrap();
    assert_eq!(debugger.stop_reason(), Some(&StopReason::Yielded));
    debugger.run_until_yield(0);
    assert!(!debugger.before_instruction(&snapshot(4, 4), 0x04));
}

#[test]
fn external_updates_are_explicit_inputs_not_instruction_writes() {
    let mut trace = trace(1);
    let mut next = transition(1);
    next.before.frame = 11;
    next.after.frame = 11;
    next.before.fields.insert("request".into(), 25);
    trace.push(next).unwrap();
    let entry = trace.entries().back().unwrap();
    assert_eq!(
        entry.input.fields[0],
        FieldChange {
            field: "request".into(),
            before: Some(1),
            after: Some(25)
        }
    );
    assert_eq!(entry.changes[0].before, Some(25));
    let mut replay = ReplaySession::new(trace.recording().unwrap()).unwrap();
    replay.seek(2).unwrap();
    assert_eq!(replay.snapshot().frame, 11);
    assert_eq!(replay.snapshot().fields["request"], 2);
}

#[test]
fn bounded_trace_replays_retained_suffix_and_exposes_dropped_prefix() {
    let mut trace = TraceBuffer::new(2);
    for offset in 0..100 {
        trace.push(transition(offset)).unwrap();
        assert!(trace.entries().len() <= 2);
    }
    assert_eq!(trace.dropped(), 98);
    let recording = trace.recording().unwrap();
    assert_eq!(recording.initial, snapshot(98, 98));
    assert_eq!(recording.entries[0].sequence, 98);
    let mut replay = ReplaySession::new(recording).unwrap();
    replay.seek(2).unwrap();
    assert_eq!(replay.snapshot(), &snapshot(100, 100));
    assert_eq!(TraceBuffer::new(0).capacity(), 1);
    assert_eq!(TraceBuffer::new(usize::MAX).capacity(), MAX_TRACE_ENTRIES);
}

#[test]
fn import_detects_gaps_delta_divergence_and_corrupt_checkpoints() {
    let original = trace(2).recording().unwrap();
    let mut corrupt = original.clone();
    corrupt.entries[1].sequence += 1;
    assert!(corrupt.validate().unwrap_err().0.contains("gap"));
    let mut corrupt = original.clone();
    corrupt.entries[1].changes[0].before = Some(500);
    let json = serde_json::to_string(&corrupt).unwrap();
    assert!(
        Recording::from_json(&json)
            .unwrap_err()
            .0
            .contains("diverged")
    );
    let mut corrupt = original.clone();
    corrupt.checkpoints[0]
        .snapshot
        .fields
        .insert("request".into(), 500);
    assert!(corrupt.validate().unwrap_err().0.contains("checkpoint"));
    let mut corrupt = original;
    corrupt.entries[1].instance.generation += 1;
    assert!(corrupt.validate().unwrap_err().0.contains("instance"));
}

#[test]
fn checkpoint_seek_and_backward_steps_restore_exact_state() {
    let recording = trace(200).recording().unwrap();
    assert_eq!(recording.checkpoints.len(), 4);
    let json = recording.to_json().unwrap();
    let mut replay = ReplaySession::new(Recording::from_json(&json).unwrap()).unwrap();
    assert!(!replay.step_back().unwrap());
    replay.seek(140).unwrap();
    assert_eq!(replay.snapshot(), &snapshot(140, 140));
    assert!(replay.step_back().unwrap());
    assert_eq!(replay.snapshot(), &snapshot(139, 139));
    assert!(replay.step_forward().unwrap());
    assert_eq!(replay.snapshot(), &snapshot(140, 140));
    assert!(replay.seek(201).is_err());
    assert_eq!(replay.position(), 140);
    replay.seek(200).unwrap();
    assert!(!replay.step_forward().unwrap());
}

#[test]
fn captured_script_bytes_are_checked_against_dispatch_record() {
    let mut recording = trace(2).recording().unwrap();
    recording.scripts.push(ScriptImage {
        revision: 7,
        script: 3,
        name: "main".into(),
        bytes: vec![0x04, 0x04].into(),
        source: None,
        source_spans: Arc::default(),
    });
    recording.validate().unwrap();
    Arc::make_mut(&mut recording.scripts[0].bytes)[1] = 0x10;
    assert!(recording.validate().unwrap_err().0.contains("script bytes"));
}

#[test]
fn failed_capture_keeps_valid_trace_and_pauses_debugger() {
    let mut debugger = Debugger::default();
    debugger.after_instruction(transition(0)).unwrap();
    let mut oversized = transition(1);
    oversized.operands = vec![0; 33];
    assert!(debugger.after_instruction(oversized).is_err());
    assert_eq!(debugger.mode(), RunMode::Paused);
    assert_eq!(debugger.trace.entries().len(), 1);
    debugger.trace.recording().unwrap().validate().unwrap();
    debugger.reset();
    assert!(debugger.trace.entries().is_empty());
    assert_eq!(debugger.stop_reason(), Some(&StopReason::User));
    let empty = debugger.trace.recording_or(snapshot(0, 0)).unwrap();
    ReplaySession::new(empty).unwrap();
}

#[test]
fn backward_frames_and_changed_instances_require_new_recordings() {
    let mut trace = trace(1);
    let mut next = transition(1);
    next.before.frame = 9;
    next.after.frame = 9;
    assert!(trace.push(next).unwrap_err().0.contains("backwards"));
    let mut next = transition(1);
    next.before.instance.generation = 5;
    next.after.instance.generation = 5;
    assert!(trace.push(next).unwrap_err().0.contains("instance"));
    assert_eq!(trace.entries().len(), 1);
}
