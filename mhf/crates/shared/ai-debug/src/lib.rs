//! Portable AI breakpoints and captured-state replay.
//!
//! A backend supplies one captured instruction transition. This crate
//! controls dispatch and records explicit changes between dispatches as inputs.
//! Replay verifies the recording and reconstructs captured state; it does not
//! execute unrecorded game functions or claim to simulate the whole client.

use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub const RECORDING_VERSION: u32 = 1;
pub const MAX_TRACE_ENTRIES: usize = 65_536;
pub const MAX_RECORDING_BYTES: usize = 32 * 1024 * 1024;
const MAX_FIELDS: usize = 256;
const MAX_LABEL_BYTES: usize = 256;
const CHECKPOINT_INTERVAL: usize = 64;
const MAX_BREAKPOINTS: usize = 256;

type Boundary = (InstanceId, Option<ProgramLocation>, u8);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct InstanceId {
    pub session: u64,
    pub slot: u32,
    pub generation: u32,
}

/// A logical block identifier and offset, never a process address.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProgramLocation {
    pub revision: u64,
    pub script: u32,
    pub offset: u32,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snapshot {
    pub instance: InstanceId,
    pub pc: Option<ProgramLocation>,
    pub frame: u64,
    /// Captured scalar state. Pointer-valued fields must be normalized by the
    /// backend, or omitted; floating point fields may use documented raw bits.
    pub fields: BTreeMap<String, i64>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    #[default]
    Continue,
    Yield,
    Reset,
    Halt,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Transition {
    pub before: Snapshot,
    pub after: Snapshot,
    pub opcode: u8,
    /// Operand bytes, excluding the opcode.
    pub operands: Vec<u8>,
    pub outcome: Outcome,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<i64>,
    pub after: Option<i64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct StateDelta {
    pub pc_before: Option<ProgramLocation>,
    pub pc_after: Option<ProgramLocation>,
    pub frame_before: u64,
    pub frame_after: u64,
    pub fields: Vec<FieldChange>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TraceEntry {
    pub sequence: u64,
    pub instance: InstanceId,
    pub frame: u64,
    pub pc_before: Option<ProgramLocation>,
    pub pc_after: Option<ProgramLocation>,
    pub opcode: u8,
    pub operands: Vec<u8>,
    /// External updates since the previous instruction, including frame and
    /// cursor changes. These are not attributed to the instruction's writes.
    pub input: StateDelta,
    pub changes: Vec<FieldChange>,
    pub outcome: Outcome,
    pub note: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Comparison {
    Equal,
    NotEqual,
    Less,
    LessOrEqual,
    Greater,
    GreaterOrEqual,
    BitsSet,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldPredicate {
    pub field: String,
    pub comparison: Comparison,
    pub value: i64,
}

impl FieldPredicate {
    pub fn matches(&self, snapshot: &Snapshot) -> bool {
        let Some(&actual) = snapshot.fields.get(&self.field) else {
            return false;
        };
        match self.comparison {
            Comparison::Equal => actual == self.value,
            Comparison::NotEqual => actual != self.value,
            Comparison::Less => actual < self.value,
            Comparison::LessOrEqual => actual <= self.value,
            Comparison::Greater => actual > self.value,
            Comparison::GreaterOrEqual => actual >= self.value,
            Comparison::BitsSet => actual & self.value == self.value,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BreakpointKind {
    Location(ProgramLocation),
    Opcode(u8),
    FieldChanged(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Breakpoint {
    pub id: u64,
    pub enabled: bool,
    pub kind: BreakpointKind,
    pub condition: Option<FieldPredicate>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum RunMode {
    Paused,
    #[default]
    Running,
    Step,
    UntilYield,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StopReason {
    User,
    Breakpoint(u64),
    StepComplete,
    Yielded,
    BudgetExhausted,
    Invalidated(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn invalid(message: impl Into<String>) -> Error {
    Error(message.into())
}

fn field_changes(before: &Snapshot, after: &Snapshot) -> Vec<FieldChange> {
    let mut changes = Vec::new();
    for (field, &previous) in &before.fields {
        let current = after.fields.get(field).copied();
        if current != Some(previous) {
            changes.push(FieldChange {
                field: field.clone(),
                before: Some(previous),
                after: current,
            });
        }
    }
    for (field, &current) in &after.fields {
        if !before.fields.contains_key(field) {
            changes.push(FieldChange {
                field: field.clone(),
                before: None,
                after: Some(current),
            });
        }
    }
    changes.sort_unstable_by(|left, right| left.field.cmp(&right.field));
    changes
}

impl StateDelta {
    fn between(before: &Snapshot, after: &Snapshot) -> Self {
        Self {
            pc_before: before.pc,
            pc_after: after.pc,
            frame_before: before.frame,
            frame_after: after.frame,
            fields: field_changes(before, after),
        }
    }

    fn apply(&self, state: &mut Snapshot) -> Result<(), Error> {
        if state.pc != self.pc_before || state.frame != self.frame_before {
            return Err(invalid("input cursor/frame diverged from recorded state"));
        }
        if self.frame_after < self.frame_before {
            return Err(invalid("recording frame moved backwards"));
        }
        apply_fields(&self.fields, &mut state.fields)?;
        state.pc = self.pc_after;
        state.frame = self.frame_after;
        Ok(())
    }
}

fn apply_fields(changes: &[FieldChange], fields: &mut BTreeMap<String, i64>) -> Result<(), Error> {
    if changes.len() > MAX_FIELDS * 2 {
        return Err(invalid("too many field changes"));
    }
    let mut previous: Option<&str> = None;
    for change in changes {
        if change.field.len() > MAX_LABEL_BYTES
            || previous.is_some_and(|field| field >= change.field.as_str())
            || change.before == change.after
        {
            return Err(invalid("invalid or unordered field changes"));
        }
        previous = Some(&change.field);
        if fields.get(&change.field).copied() != change.before {
            return Err(invalid(format!(
                "field {} diverged from recorded value",
                change.field
            )));
        }
    }
    // Check recorded preconditions before applying any writes.
    for change in changes {
        if let Some(value) = change.after {
            fields.insert(change.field.clone(), value);
        } else {
            fields.remove(&change.field);
        }
    }
    if fields.len() > MAX_FIELDS {
        return Err(invalid("too many captured state fields"));
    }
    Ok(())
}

fn validate_snapshot(snapshot: &Snapshot) -> Result<(), Error> {
    if snapshot.fields.len() > MAX_FIELDS
        || snapshot
            .fields
            .keys()
            .any(|field| field.len() > MAX_LABEL_BYTES)
    {
        return Err(invalid("snapshot exceeds captured state limits"));
    }
    Ok(())
}

impl TraceEntry {
    fn apply(&self, state: &Snapshot) -> Result<Snapshot, Error> {
        if self.instance != state.instance {
            return Err(invalid("recording changed monster instance"));
        }
        if self.note.len() > 4096 || self.operands.len() > 32 {
            return Err(invalid("instruction record exceeds size limits"));
        }
        let mut next = state.clone();
        self.input.apply(&mut next)?;
        if next.pc != self.pc_before || next.frame != self.frame {
            return Err(invalid(
                "instruction input does not match its execution location",
            ));
        }
        apply_fields(&self.changes, &mut next.fields)?;
        next.pc = self.pc_after;
        Ok(next)
    }
}

/// A suffix of the captured execution. Eviction advances the initial snapshot
/// so the retained suffix remains replayable, and reports exactly how many
/// earlier instructions were dropped.
#[derive(Clone, Debug)]
pub struct TraceBuffer {
    capacity: usize,
    initial: Option<Snapshot>,
    latest: Option<Snapshot>,
    entries: VecDeque<TraceEntry>,
    dropped: u64,
}

impl TraceBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity: capacity.clamp(1, MAX_TRACE_ENTRIES),
            initial: None,
            latest: None,
            entries: VecDeque::new(),
            dropped: 0,
        }
    }

    pub fn entries(&self) -> &VecDeque<TraceEntry> {
        &self.entries
    }

    pub fn snapshot(&self) -> Option<&Snapshot> {
        self.latest.as_ref()
    }

    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    pub fn capacity(&self) -> usize {
        self.capacity
    }

    pub fn clear(&mut self) {
        *self = Self::new(self.capacity);
    }

    pub fn push(&mut self, transition: Transition) -> Result<(), Error> {
        validate_snapshot(&transition.before)?;
        validate_snapshot(&transition.after)?;
        if transition.before.instance != transition.after.instance
            || self
                .latest
                .as_ref()
                .is_some_and(|state| state.instance != transition.before.instance)
        {
            return Err(invalid("monster instance changed; start a new recording"));
        }
        if transition.after.frame != transition.before.frame {
            return Err(invalid(
                "an instruction transition must stay within one frame",
            ));
        }
        let previous = self.latest.as_ref().unwrap_or(&transition.before);
        let entry = TraceEntry {
            sequence: self.dropped + self.entries.len() as u64,
            instance: transition.before.instance,
            frame: transition.before.frame,
            pc_before: transition.before.pc,
            pc_after: transition.after.pc,
            opcode: transition.opcode,
            operands: transition.operands,
            input: StateDelta::between(previous, &transition.before),
            changes: field_changes(&transition.before, &transition.after),
            outcome: transition.outcome,
            note: transition.note,
        };
        if entry.apply(previous)? != transition.after {
            return Err(invalid(
                "transition does not reconstruct its output snapshot",
            ));
        }
        if self.initial.is_none() {
            self.initial = Some(transition.before);
        }
        if self.entries.len() == self.capacity
            && let Some(old) = self.entries.pop_front()
        {
            self.initial = Some(
                old.apply(
                    self.initial
                        .as_ref()
                        .expect("nonempty trace has initial state"),
                )?,
            );
            self.dropped += 1;
        }
        self.entries.push_back(entry);
        self.latest = Some(transition.after);
        Ok(())
    }

    pub fn recording(&self) -> Result<Recording, Error> {
        let initial = self
            .initial
            .clone()
            .ok_or_else(|| invalid("no instructions recorded"))?;
        let mut recording = Recording {
            dropped: self.dropped,
            entries: self.entries.iter().cloned().collect(),
            ..Recording::empty(initial)
        };
        recording.checkpoints = recording.validated_checkpoints()?;
        Ok(recording)
    }

    pub fn recording_or(&self, snapshot: Snapshot) -> Result<Recording, Error> {
        if self.initial.is_some() {
            self.recording()
        } else {
            validate_snapshot(&snapshot)?;
            Ok(Recording::empty(snapshot))
        }
    }
}

impl Default for TraceBuffer {
    fn default() -> Self {
        Self::new(2048)
    }
}

#[derive(Clone, Debug)]
pub struct Debugger {
    pub trace: TraceBuffer,
    breakpoints: Vec<Breakpoint>,
    mode: RunMode,
    stop_reason: Option<StopReason>,
    next_breakpoint: u64,
    skip_once: bool,
    breakpoint_boundary: Option<Boundary>,
    skip_boundary: Option<Boundary>,
    remaining: Option<usize>,
}

impl Default for Debugger {
    fn default() -> Self {
        Self::new(2048)
    }
}

impl Debugger {
    pub fn new(capacity: usize) -> Self {
        Self {
            trace: TraceBuffer::new(capacity),
            breakpoints: Vec::new(),
            mode: RunMode::Running,
            stop_reason: None,
            next_breakpoint: 1,
            skip_once: false,
            breakpoint_boundary: None,
            skip_boundary: None,
            remaining: None,
        }
    }

    pub fn mode(&self) -> RunMode {
        self.mode
    }

    pub fn stop_reason(&self) -> Option<&StopReason> {
        self.stop_reason.as_ref()
    }

    pub fn breakpoints(&self) -> &[Breakpoint] {
        &self.breakpoints
    }

    pub fn add_breakpoint(
        &mut self,
        kind: BreakpointKind,
        condition: Option<FieldPredicate>,
    ) -> u64 {
        let id = self.next_breakpoint;
        self.next_breakpoint += 1;
        self.breakpoints.push(Breakpoint {
            id,
            enabled: true,
            kind,
            condition,
        });
        id
    }

    pub fn remove_breakpoint(&mut self, id: u64) {
        self.breakpoints.retain(|breakpoint| breakpoint.id != id);
    }

    pub fn set_breakpoints(&mut self, breakpoints: Vec<Breakpoint>) -> Result<(), Error> {
        let mut ids = std::collections::BTreeSet::new();
        if breakpoints.len() > MAX_BREAKPOINTS || breakpoints.iter().any(|breakpoint| {
            !ids.insert(breakpoint.id)
                || breakpoint.id == u64::MAX
                || matches!(&breakpoint.kind, BreakpointKind::FieldChanged(field) if field.len() > MAX_LABEL_BYTES)
                || breakpoint.condition.as_ref().is_some_and(|condition| condition.field.len() > MAX_LABEL_BYTES)
        }) {
            return Err(invalid("too many, duplicate, or invalid breakpoints"));
        }
        self.next_breakpoint = ids.last().copied().unwrap_or(0).saturating_add(1);
        self.breakpoints = breakpoints;
        Ok(())
    }

    pub fn pause(&mut self) {
        self.stop(StopReason::User);
    }

    pub fn resume(&mut self) {
        self.skip_once = false;
        self.skip_boundary = self.breakpoint_boundary.take();
        self.mode = RunMode::Running;
        self.stop_reason = None;
        self.remaining = None;
    }

    pub fn step(&mut self) {
        self.mode = RunMode::Step;
        self.stop_reason = None;
        self.skip_once = true;
        self.skip_boundary = None;
        self.breakpoint_boundary = None;
        self.remaining = None;
    }

    pub fn run_until_yield(&mut self, budget: usize) {
        self.resume();
        self.mode = RunMode::UntilYield;
        self.remaining = Some(budget);
        if budget == 0 {
            self.stop(StopReason::BudgetExhausted);
        }
    }

    pub fn invalidate(&mut self, reason: impl Into<String>) {
        self.stop(StopReason::Invalidated(reason.into()));
    }

    /// Reset captures and invalidate the current continuation. Logical location
    /// breakpoints retain their revision and cannot bind to a replacement script.
    pub fn reset(&mut self) {
        self.trace.clear();
        self.skip_once = false;
        self.skip_boundary = None;
        self.pause();
    }

    fn stop(&mut self, reason: StopReason) {
        self.mode = RunMode::Paused;
        self.stop_reason = Some(reason);
        self.remaining = None;
        self.breakpoint_boundary = None;
    }

    pub fn before_instruction(&mut self, snapshot: &Snapshot, opcode: u8) -> bool {
        if self.mode == RunMode::Paused {
            return false;
        }
        if std::mem::take(&mut self.skip_once) {
            return true;
        }
        if self.skip_boundary.take() == Some((snapshot.instance, snapshot.pc, opcode)) {
            return true;
        }
        let hit = self
            .breakpoints
            .iter()
            .find(|breakpoint| {
                breakpoint.enabled
                    && breakpoint
                        .condition
                        .as_ref()
                        .is_none_or(|condition| condition.matches(snapshot))
                    && match &breakpoint.kind {
                        BreakpointKind::Location(location) => snapshot.pc == Some(*location),
                        BreakpointKind::Opcode(expected) => opcode == *expected,
                        BreakpointKind::FieldChanged(field) => {
                            self.trace.snapshot().is_some_and(|previous| {
                                previous.instance == snapshot.instance
                                    && previous.fields.get(field) != snapshot.fields.get(field)
                            })
                        }
                    }
            })
            .map(|breakpoint| breakpoint.id);
        if let Some(id) = hit {
            self.stop(StopReason::Breakpoint(id));
            self.breakpoint_boundary = Some((snapshot.instance, snapshot.pc, opcode));
            false
        } else {
            true
        }
    }

    pub fn after_instruction(&mut self, transition: Transition) -> Result<(), Error> {
        let hit = self
            .breakpoints
            .iter()
            .find(|breakpoint| {
                breakpoint.enabled
                    && breakpoint
                        .condition
                        .as_ref()
                        .is_none_or(|condition| condition.matches(&transition.after))
                    && match &breakpoint.kind {
                        BreakpointKind::FieldChanged(field) => {
                            transition.before.fields.get(field)
                                != transition.after.fields.get(field)
                        }
                        _ => false,
                    }
            })
            .map(|breakpoint| breakpoint.id);
        let outcome = transition.outcome;
        if let Err(error) = self.trace.push(transition) {
            self.invalidate(error.to_string());
            return Err(error);
        }
        if let Some(id) = hit {
            self.stop(StopReason::Breakpoint(id));
        } else if self.mode == RunMode::Step {
            self.stop(StopReason::StepComplete);
        } else if self.mode == RunMode::UntilYield && outcome != Outcome::Continue {
            self.stop(StopReason::Yielded);
        } else if let Some(remaining) = &mut self.remaining {
            *remaining = remaining.saturating_sub(1);
            if *remaining == 0 {
                self.stop(StopReason::BudgetExhausted);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScriptImage {
    pub revision: u64,
    pub script: u32,
    pub name: String,
    pub bytes: Arc<[u8]>,
    pub source: Option<Arc<str>>,
    #[serde(default)]
    pub source_spans: Arc<[SourceSpan]>,
}

/// Half-open bytecode and source byte ranges. Lines and columns are one-based.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceSpan {
    pub offset_start: u32,
    pub offset_end: u32,
    pub path: String,
    pub line: u32,
    pub column: u32,
    pub source_start: u32,
    pub source_end: u32,
}

/// Observations can reproduce captured state but cannot rerun unseen native
/// callbacks. Deterministic execution recordings are reserved for a future
/// backend that captures all host inputs and are rejected by this reader.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplayCapability {
    #[default]
    Observation,
    Deterministic,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Checkpoint {
    /// Number of entries already applied to this snapshot.
    pub position: usize,
    pub snapshot: Snapshot,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recording {
    pub version: u32,
    #[serde(default)]
    pub capability: ReplayCapability,
    pub initial: Snapshot,
    pub dropped: u64,
    pub entries: Vec<TraceEntry>,
    pub scripts: Vec<ScriptImage>,
    pub checkpoints: Vec<Checkpoint>,
}

impl Recording {
    pub fn empty(initial: Snapshot) -> Self {
        Self {
            version: RECORDING_VERSION,
            capability: ReplayCapability::Observation,
            initial,
            dropped: 0,
            entries: Vec::new(),
            scripts: Vec::new(),
            checkpoints: Vec::new(),
        }
    }

    pub fn to_json(&self) -> Result<String, Error> {
        self.validate()?;
        let json = serde_json::to_string(self).map_err(|error| invalid(error.to_string()))?;
        if json.len() > MAX_RECORDING_BYTES {
            return Err(invalid("recording exceeds JSON size limit"));
        }
        Ok(json)
    }

    pub fn from_json(json: &str) -> Result<Self, Error> {
        if json.len() > MAX_RECORDING_BYTES {
            return Err(invalid("recording exceeds JSON size limit"));
        }
        let recording: Self =
            serde_json::from_str(json).map_err(|error| invalid(error.to_string()))?;
        recording.validate()?;
        Ok(recording)
    }

    pub fn validate(&self) -> Result<(), Error> {
        self.validate_with_checkpoints(None)
    }

    fn validated_checkpoints(&self) -> Result<Vec<Checkpoint>, Error> {
        let mut checkpoints = Vec::new();
        self.validate_with_checkpoints(Some(&mut checkpoints))?;
        Ok(checkpoints)
    }

    fn validate_with_checkpoints(
        &self,
        mut generated: Option<&mut Vec<Checkpoint>>,
    ) -> Result<(), Error> {
        if self.version != RECORDING_VERSION {
            return Err(invalid("unsupported AI recording version"));
        }
        if self.capability != ReplayCapability::Observation {
            return Err(invalid(
                "deterministic execution replay is not supported by this reader",
            ));
        }
        if self.entries.len() > MAX_TRACE_ENTRIES || self.scripts.len() > 4096 {
            return Err(invalid("recording exceeds entry limits"));
        }
        validate_snapshot(&self.initial)?;
        let mut scripts = BTreeMap::new();
        for script in &self.scripts {
            if script.name.len() > MAX_LABEL_BYTES || script.bytes.len() > 1024 * 1024 {
                return Err(invalid("script image exceeds size limits"));
            }
            for span in script.source_spans.iter() {
                if span.offset_start >= span.offset_end
                    || span.offset_end as usize > script.bytes.len()
                    || span.source_start > span.source_end
                    || span.line == 0
                    || span.column == 0
                    || span.path.len() > 4096
                    || script.source.as_ref().is_some_and(|source| {
                        !source.is_char_boundary(span.source_start as usize)
                            || !source.is_char_boundary(span.source_end as usize)
                    })
                {
                    return Err(invalid("invalid script source mapping"));
                }
            }
            if scripts
                .insert((script.revision, script.script), script)
                .is_some()
            {
                return Err(invalid("duplicate script image"));
            }
        }
        let mut state = self.initial.clone();
        let mut checkpoints = self.checkpoints.iter().peekable();
        for position in 0..=self.entries.len() {
            if let Some(checkpoint) = checkpoints.peek() {
                if checkpoint.position < position {
                    return Err(invalid("duplicate or unordered checkpoint"));
                }
                if checkpoint.position == position {
                    if checkpoint.snapshot != state {
                        return Err(invalid("checkpoint diverged from recorded state"));
                    }
                    checkpoints.next();
                }
            }
            if position % CHECKPOINT_INTERVAL == 0
                && let Some(generated) = &mut generated
            {
                generated.push(Checkpoint {
                    position,
                    snapshot: state.clone(),
                });
            }
            let Some(entry) = self.entries.get(position) else {
                break;
            };
            if entry.sequence
                != self
                    .dropped
                    .checked_add(position as u64)
                    .ok_or_else(|| invalid("sequence overflow"))?
            {
                return Err(invalid("recording contains an instruction sequence gap"));
            }
            if let Some(location) = entry.pc_before
                && let Some(script) = scripts.get(&(location.revision, location.script))
            {
                let bytes = script
                    .bytes
                    .get(location.offset as usize..)
                    .ok_or_else(|| invalid("instruction offset lies outside script"))?;
                let length = mhf_monster::ai::bytecode::instruction_len(bytes)
                    .map_err(|error| invalid(error.to_string()))?;
                if bytes.first() != Some(&entry.opcode) || bytes[1..length] != entry.operands {
                    return Err(invalid(
                        "recorded instruction differs from captured script bytes",
                    ));
                }
            }
            state = entry.apply(&state)?;
        }
        if checkpoints.next().is_some() {
            return Err(invalid("checkpoint lies outside recording"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct ReplaySession {
    recording: Recording,
    position: usize,
    snapshot: Snapshot,
}

impl ReplaySession {
    pub fn new(mut recording: Recording) -> Result<Self, Error> {
        recording.checkpoints = recording.validated_checkpoints()?;
        Ok(Self {
            snapshot: recording.initial.clone(),
            recording,
            position: 0,
        })
    }

    pub fn recording(&self) -> &Recording {
        &self.recording
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn snapshot(&self) -> &Snapshot {
        &self.snapshot
    }

    /// The next instruction that forward stepping will apply.
    pub fn current_entry(&self) -> Option<&TraceEntry> {
        self.recording.entries.get(self.position)
    }

    pub fn step_forward(&mut self) -> Result<bool, Error> {
        let Some(entry) = self.recording.entries.get(self.position) else {
            return Ok(false);
        };
        self.snapshot = entry.apply(&self.snapshot)?;
        self.position += 1;
        Ok(true)
    }

    pub fn step_back(&mut self) -> Result<bool, Error> {
        if self.position == 0 {
            return Ok(false);
        }
        self.seek(self.position - 1)?;
        Ok(true)
    }

    /// Seek to the state after exactly `position` recorded instructions.
    pub fn seek(&mut self, position: usize) -> Result<(), Error> {
        if position > self.recording.entries.len() {
            return Err(invalid("replay position lies outside recording"));
        }
        let checkpoint = self
            .recording
            .checkpoints
            .iter()
            .rev()
            .find(|checkpoint| checkpoint.position <= position);
        let (mut next, mut cursor) = checkpoint.map_or_else(
            || (self.recording.initial.clone(), 0),
            |checkpoint| (checkpoint.snapshot.clone(), checkpoint.position),
        );
        while cursor < position {
            next = self.recording.entries[cursor].apply(&next)?;
            cursor += 1;
        }
        self.snapshot = next;
        self.position = position;
        Ok(())
    }
}

#[cfg(test)]
mod tests;
