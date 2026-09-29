//! Compilation of a parsed document: name resolution, statement encoding, and
//! assembly of the pointer-free graph the native selector consumes.
//!
//! Both base kinds compile to the same shape: a table is a map from logical
//! index to node that holds only what the document declares.  An empty base
//! means an undeclared index is an empty slot; `base native;` means it keeps
//! the native entry once the binding merges the declaration onto the live
//! block.  The compiler never reads the client's tables, so it has nothing
//! to say about how far a table reaches.

use std::collections::{HashMap, HashSet};

use super::allocation::{Allocation, event_functions};
use super::condition::Degrees;
use super::parser::{Callee, Document, Function, Statement, StatementKind, check_document};
use super::slot::NativeSlot;
use super::{DebugInfo, SourceLocation, SourceMapping};
use crate::ai::control::EVENT_SLOTS;
use crate::ai::{Base, CallRelocation, Diagnostic, Error, Node, Program, Result, Table, bytecode};

/// A compiled document plus the facts the compiler could not prove statically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    pub program: Program,
    pub warnings: Vec<Diagnostic>,
    pub debug_info: DebugInfo,
}

/// A name the compiler owns (spec §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Command {
    /// Author-writable: `opcode` followed by exactly `args` literal bytes.
    opcode: &'static [u8],
    args: usize,
}

fn reserved_command(name: &str) -> Option<Command> {
    let (opcode, args): (&[u8], usize) = match name {
        "stop" => (&[0x68], 0),
        "clear_requests" => (&[0x1e], 0),
        "mark_unhandled" => (&[0x0d, 0x04], 0),
        "nop" => (&[0x92], 0),
        "wait" => (&[0x48], 1),
        "area_end" => (&[0xff, 0xfb], 0),
        "route_move_end" => (&[0xff, 0xfe], 0),
        _ => return None,
    };
    Some(Command { opcode, args })
}

/// Whether `name` belongs to the command vocabulary the compiler owns. The
/// parser asks this before letting an action alias take the name (spec §6).
pub(super) fn is_reserved_command(name: &str) -> bool {
    reserved_command(name).is_some()
}

/// Names a document declares, resolved once per compilation.
struct Names<'a> {
    actions: HashMap<&'a str, (u8, u8)>,
    states: HashMap<&'a str, u8>,
}

impl<'a> Names<'a> {
    fn collect(document: &'a Document) -> Self {
        Self {
            actions: document
                .actions
                .iter()
                .map(|decl| (decl.name.as_str(), (decl.group, decl.id)))
                .collect(),
            states: document
                .states
                .iter()
                .map(|decl| (decl.name.as_str(), decl.index))
                .collect(),
        }
    }
}

/// Which lane a body belongs to. The block decides the lane, and the lane
/// decides which commands are legal and which hints apply (spec §7, §9).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Scope {
    States,
    Events,
}

struct EncodedScript {
    mappings: Vec<SourceMapping>,
    bytes: Vec<u8>,
    relocations: Vec<(usize, NativeSlot)>,
}

fn push_script(
    nodes: &mut Vec<Node>,
    relocations: &mut Vec<CallRelocation>,
    encoded: EncodedScript,
    debug_info: &mut DebugInfo,
) -> usize {
    let script = nodes.len();
    nodes.push(Node::Script(encoded.bytes));
    debug_info
        .mappings
        .extend(encoded.mappings.into_iter().map(|mut mapping| {
            mapping.script = script;
            mapping
        }));
    relocations.extend(
        encoded
            .relocations
            .into_iter()
            .map(|(offset, target)| CallRelocation {
                script,
                offset,
                target,
            }),
    );
    script
}

struct Compiler<'a> {
    mappings: Vec<SourceMapping>,
    owner: SourceLocation,
    auto_finish: bool,
    names: Names<'a>,
    warnings: Vec<Diagnostic>,
    functions: &'a [super::parser::Function],
    allocation: &'a Allocation,
    relocations: Vec<(usize, NativeSlot)>,
    native_scope: Option<NativeSlot>,
    raw_boundary: usize,
    raw_return_blocked: bool,
    return_ending: Option<u8>,
}

impl Compiler<'_> {
    fn encode_script(
        &mut self,
        body: &[Statement],
        scope: Scope,
        slot: Option<NativeSlot>,
        ending: Option<u8>,
        owner: SourceLocation,
    ) -> Result<EncodedScript> {
        self.owner = owner;
        self.mappings.clear();
        self.native_scope = slot;
        self.raw_boundary = 0;
        self.raw_return_blocked = false;
        self.return_ending = slot
            .map(NativeSlot::ending)
            .or_else(|| ending.filter(|_| matches!(scope, Scope::Events)));
        self.relocations.clear();
        let mut bytes = Vec::new();
        let terminated = self.encode_body(body, scope, &mut bytes)?;
        if !terminated && let Some(ending) = ending {
            let start = bytes.len();
            finish(&mut bytes, ending, slot)?;
            if bytes.len() > start {
                self.mappings.push(SourceMapping {
                    script: 0,
                    start,
                    end: bytes.len(),
                    source: self.owner.clone(),
                    generated: true,
                });
            }
        }
        self.mappings.sort_by_key(|mapping| mapping.start);
        Ok(EncodedScript {
            mappings: std::mem::take(&mut self.mappings),
            bytes,
            relocations: std::mem::take(&mut self.relocations),
        })
    }

    fn record_statement(
        &mut self,
        statement: &Statement,
        start: usize,
        children: usize,
        out: &[u8],
    ) -> Result<()> {
        let mut occupied: Vec<_> = self.mappings[children..]
            .iter()
            .map(|mapping| (mapping.start, mapping.end))
            .collect();
        occupied.sort_unstable();
        let structured = matches!(
            statement.kind,
            StatementKind::EntryBody(_)
                | StatementKind::Handle { .. }
                | StatementKind::If { .. }
                | StatementKind::Random(_)
                | StatementKind::TargetDistanceGroups(_)
                | StatementKind::TargetAngle { .. }
                | StatementKind::ContextQuery { .. }
                | StatementKind::AreaRouteProfile { .. }
                | StatementKind::Area { .. }
                | StatementKind::SpeciesGroup { .. }
                | StatementKind::Request { .. }
                | StatementKind::DebugMode { .. }
                | StatementKind::Species { .. }
        );
        let mut cursor = start;
        // Child statements own their ranges; only the gaps belong to this
        // statement's instructions or generated control-flow markers.
        for (next, end) in occupied
            .into_iter()
            .chain(std::iter::once((out.len(), out.len())))
        {
            if cursor < next {
                for instruction in bytecode::decode(&out[cursor..next])? {
                    let offset = cursor + instruction.offset;
                    self.mappings.push(SourceMapping {
                        script: 0,
                        start: offset,
                        end: offset + instruction.bytes.len(),
                        source: SourceLocation {
                            function: self.owner.function.clone(),
                            ..statement.source.clone()
                        },
                        generated: structured && offset != start,
                    });
                }
            }
            cursor = cursor.max(end);
        }
        Ok(())
    }

    fn encode_branch(
        &mut self,
        statement: &Statement,
        name: &str,
        body: &[Statement],
        continuation: &[&Statement],
        scope: Scope,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        let start = out.len();
        let previous_boundary = self.raw_boundary;
        self.raw_boundary = start;
        let result =
            self.encode_sequence(body.iter().chain(continuation.iter().copied()), scope, out);
        self.raw_boundary = previous_boundary;
        result?;
        if !closed(&out[start..])? {
            return Err(statement.error(format!(
                "{name} branch contains an unclosed native conditional"
            )));
        }
        Ok(())
    }

    fn encode_body(&mut self, body: &[Statement], scope: Scope, out: &mut Vec<u8>) -> Result<bool> {
        self.encode_sequence(body.iter(), scope, out)
    }

    fn encode_sequence<'s>(
        &mut self,
        mut body: impl Iterator<Item = &'s Statement> + Clone,
        scope: Scope,
        out: &mut Vec<u8>,
    ) -> Result<bool> {
        while let Some(statement) = body.next() {
            let start = out.len();
            let children = self.mappings.len();
            if out.len() > super::super::decompile::MAX_SCRIPT_BYTES {
                return Err(statement.error("generated script exceeds 64 KiB"));
            }
            if matches!(statement.kind, StatementKind::Return | StatementKind::Pass) {
                let passing = matches!(statement.kind, StatementKind::Pass);
                // Functions and events emit their scope's native return;
                // entry returns remain lexical.
                if self.return_ending.is_none()
                    && (self.raw_return_blocked || !closed(&out[self.raw_boundary..])?)
                {
                    return Err(statement.error(if passing {
                        "pass inside a native conditional is not supported"
                    } else {
                        "return inside a native conditional is not supported"
                    }));
                }
                if passing {
                    out.extend_from_slice(&[0x0d, 4]);
                }
                if let Some(ending) = self.return_ending {
                    out.extend_from_slice(&[0xff, ending]);
                    // A return inside an open native block still has to reproduce
                    // that block's closing markers, so it only ends the body once
                    // the script is closed and no statement follows.
                    self.record_statement(statement, start, children, out)?;
                    let trailing = body.clone().next().is_none();
                    if trailing && closed(out)? {
                        return Ok(true);
                    }
                    continue;
                }
                self.record_statement(statement, start, children, out)?;
                return Ok(false);
            }
            if let StatementKind::Call {
                callee: Callee::Name(name),
                args,
            } = &statement.kind
                && self.functions.iter().any(|function| function.name == *name)
            {
                let terminated = self.encode_call(statement, name, args, out)?;
                self.record_statement(statement, start, children, out)?;
                if terminated {
                    return Ok(true);
                }
                continue;
            }
            // Entry returns need lexical restructuring; subscript returns emit
            // their own FF instruction and leave the caller untouched.
            let move_continuation = self.return_ending.is_none() && contains_exit(statement);
            let continuation: Vec<_> = if move_continuation {
                body.clone().collect()
            } else {
                Vec::new()
            };
            self.encode_statement(statement, &continuation, scope, out)?;
            self.record_statement(statement, start, children, out)?;
            if out.len() > super::super::decompile::MAX_SCRIPT_BYTES {
                return Err(statement.error("generated script exceeds 64 KiB"));
            }
            if move_continuation {
                return Ok(false);
            }
            if self.auto_finish
                && matches!(
                    statement.kind,
                    StatementKind::Transition { .. }
                        | StatementKind::Restart
                        | StatementKind::Reset
                        | StatementKind::ResetForgetTarget
                )
                && closed(out)?
            {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Emit one native call. Same-level 81/82 calls terminate this path;
    /// table 9 inherits the runtime stage, so no such assumption is made there.
    fn encode_call(
        &mut self,
        statement: &Statement,
        name: &str,
        args: &[u8],
        out: &mut Vec<u8>,
    ) -> Result<bool> {
        if name == "main" {
            return Err(statement.error("main is an entry point, not a callable helper"));
        }
        self.require_args(statement, args, 0, name)?;
        let slot = self.allocation.slots[name];
        if self.allocation.automatic.contains(&slot) {
            self.relocations.push((out.len(), slot));
        }
        let call = slot.call();
        out.extend_from_slice(&call);
        Ok(self
            .native_scope
            .is_some_and(|current| current.is_same_level_call(&call))
            && closed(out)?)
    }

    fn encode_statement(
        &mut self,
        statement: &Statement,
        continuation: &[&Statement],
        scope: Scope,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        let previous_blocked = self.raw_return_blocked;
        if matches!(
            statement.kind,
            StatementKind::If { .. }
                | StatementKind::Random(_)
                | StatementKind::TargetDistanceGroups(_)
                | StatementKind::TargetAngle { .. }
                | StatementKind::ContextQuery { .. }
                | StatementKind::AreaRouteProfile { .. }
                | StatementKind::Area { .. }
                | StatementKind::SpeciesGroup { .. }
                | StatementKind::Request { .. }
                | StatementKind::DebugMode { .. }
                | StatementKind::Species { .. }
        ) {
            self.raw_return_blocked |= !closed(&out[self.raw_boundary..])?;
        }
        match &statement.kind {
            StatementKind::EntryBody(body) => {
                self.encode_body(body, scope, out)?;
            }
            StatementKind::Handle { handler, then_body } => {
                out.extend_from_slice(&[0x1b, 0, 1, 0x0c, 4, 1]);
                let previous_boundary = self.raw_boundary;
                let previous_blocked = self.raw_return_blocked;
                self.raw_boundary = out.len();
                self.raw_return_blocked = false;
                let result = self.encode_call(statement, handler, &[], out);
                self.raw_boundary = previous_boundary;
                self.raw_return_blocked = previous_blocked;
                result?;
                out.extend_from_slice(&[0x2b, 0, 4, 1]);
                self.encode_branch(statement, "then", then_body, &[], scope, out)?;
                out.extend_from_slice(&[0x2b, 2, 0x1b, 2]);
            }
            StatementKind::Random(branches) => {
                let total: u64 = branches.iter().map(|(weight, _)| u64::from(*weight)).sum();
                if total == 0 {
                    return Err(statement.error("random requires at least one positive weight"));
                }
                let mut weights: Vec<u8> = branches
                    .iter()
                    .map(|(weight, _)| (u64::from(*weight) * 32 / total) as u8)
                    .collect();
                let mut order: Vec<usize> = (0..branches.len()).collect();
                order.sort_by_key(|&index| {
                    std::cmp::Reverse(u64::from(branches[index].0) * 32 % total)
                });
                let remaining = 32
                    - weights
                        .iter()
                        .map(|&weight| usize::from(weight))
                        .sum::<usize>();
                for &index in order.iter().take(remaining) {
                    weights[index] += 1;
                }
                if branches
                    .iter()
                    .zip(&weights)
                    .any(|((original, _), normalized)| *original > 0 && *normalized == 0)
                {
                    return Err(statement.error(
                        "random weight normalizes to zero; increase it or reduce the other weights",
                    ));
                }
                if branches
                    .iter()
                    .any(|(weight, _)| u64::from(*weight) * 32 % total != 0)
                {
                    self.warnings.push(Diagnostic::at(
                        statement.line,
                        statement.column,
                        format!("random weights rounded to {weights:?} out of 32"),
                    ));
                }
                out.extend_from_slice(&[0x80, 0, branches.len() as u8]);
                for (index, ((_, body), weight)) in branches.iter().zip(weights).enumerate() {
                    out.extend_from_slice(&[0x80, index as u8 + 1, weight]);
                    self.encode_branch(statement, "random", body, continuation, scope, out)?;
                }
                out.extend_from_slice(&[0x80, 0xff]);
            }
            StatementKind::ContextQuery {
                branches, fallback, ..
            }
            | StatementKind::AreaRouteProfile { branches, fallback } => {
                if !(1..=255).contains(&branches.len())
                    || branches.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                {
                    return Err(
                        statement.error("byte match requires 1..255 strictly increasing cases")
                    );
                }
                let opcode = if let StatementKind::ContextQuery { argument, .. } = &statement.kind {
                    out.extend_from_slice(&[0x79, 0, branches.len() as u8, *argument]);
                    0x79
                } else {
                    out.extend_from_slice(&[0x57, 0, branches.len() as u8]);
                    0x57
                };
                for (value, body) in branches {
                    out.extend_from_slice(&[opcode, 1]);
                    if opcode == 0x57 {
                        out.push(0);
                    }
                    out.push(*value);
                    self.encode_branch(statement, "byte match", body, continuation, scope, out)?;
                }
                out.extend_from_slice(&[opcode, 2]);
                self.encode_branch(
                    statement,
                    "byte match else",
                    fallback,
                    continuation,
                    scope,
                    out,
                )?;
                out.extend_from_slice(&[opcode, 3]);
            }
            StatementKind::TargetAngle { branches, fallback } => {
                if !(1..=255).contains(&branches.len()) {
                    return Err(statement.error("target angle match requires 1..255 cases"));
                }
                out.extend_from_slice(&[0x20, 0, branches.len() as u8]);
                for (degrees, body) in branches {
                    let threshold = degrees.native();
                    let actual = Degrees::from_native(threshold).value();
                    if degrees.value() != actual {
                        self.warnings.push(Diagnostic::at(statement.line, statement.column, format!(
                            "target_angle case {} quantized/clamped to {actual} degrees; native maximum is 358.59375",
                            degrees.value()
                        )));
                    }
                    out.extend_from_slice(&[0x20, 1, threshold]);
                    self.encode_branch(statement, "target angle", body, continuation, scope, out)?;
                }
                if fallback.is_some() || !continuation.is_empty() {
                    out.extend_from_slice(&[0x20, 2]);
                    self.encode_branch(
                        statement,
                        "target angle else",
                        fallback.as_deref().unwrap_or_default(),
                        continuation,
                        scope,
                        out,
                    )?;
                }
                out.extend_from_slice(&[0x20, 3]);
            }
            StatementKind::Area { branches, fallback }
            | StatementKind::SpeciesGroup { branches, fallback }
            | StatementKind::Request { branches, fallback }
            | StatementKind::DebugMode { branches, fallback }
            | StatementKind::Species { branches, fallback } => {
                let opcode = match statement.kind {
                    StatementKind::Area { .. } => 0x15,
                    StatementKind::DebugMode { .. } => 0x94,
                    StatementKind::Species { .. } => 0x70,
                    StatementKind::Request { .. } => 0x1d,
                    _ => 0x2c,
                };
                // Request values below a case bypass even the native fallback,
                // so moving an entry's continuation into its branches loses that path.
                if opcode == 0x1d && !continuation.is_empty() {
                    return Err(statement.error(
                        "request match with an entry return cannot preserve the following statements; use a helper function",
                    ));
                }
                if matches!(opcode, 0x1d | 0x70 | 0x94)
                    && branches.windows(2).any(|pair| pair[0].0 >= pair[1].0)
                {
                    return Err(statement.error("byte match cases must be strictly increasing"));
                }
                if !(1..=255).contains(&branches.len()) {
                    return Err(statement.error("byte match requires 1..255 cases"));
                }
                out.extend_from_slice(&[opcode, 0, branches.len() as u8]);
                for (value, body) in branches {
                    out.extend_from_slice(&[opcode, 1]);
                    if opcode == 0x15 {
                        out.extend_from_slice(&value.to_be_bytes());
                    } else {
                        out.push(
                            u8::try_from(*value)
                                .map_err(|_| statement.error("byte match case must be 0..255"))?,
                        );
                    }
                    self.encode_branch(statement, "byte match", body, continuation, scope, out)?;
                }
                if let Some(fallback) = fallback {
                    out.extend_from_slice(&[opcode, 2]);
                    self.encode_branch(
                        statement,
                        "byte match else",
                        fallback,
                        continuation,
                        scope,
                        out,
                    )?;
                }
                out.extend_from_slice(&[opcode, 3]);
            }
            StatementKind::TargetDistanceGroups(branches) => {
                if !(2..=5).contains(&branches.len()) {
                    return Err(statement.error("distance match requires 1..4 groups and else"));
                }
                out.extend_from_slice(&[0x83, 0, branches.len() as u8 - 1]);
                for (index, body) in branches.iter().enumerate() {
                    out.extend_from_slice(&[0x83, index as u8 + 1]);
                    self.encode_branch(statement, "distance", body, continuation, scope, out)?;
                }
                out.extend_from_slice(&[0x83, 0xff]);
            }
            StatementKind::SelectTargetEntity(strategy) => {
                out.extend_from_slice(&strategy.encode())
            }
            StatementKind::SelectTargetArea(area) => {
                out.extend_from_slice(&[0x06, 3, 0]);
                out.extend_from_slice(&area.to_be_bytes());
            }
            StatementKind::SelectTargetPlayerArea => out.extend_from_slice(&[0x06, 10, 0, 0]),
            StatementKind::SelectPlayerSlot(slot) => out.extend_from_slice(&[0x06, 1, 0, *slot]),
            StatementKind::SelectTargetPoint(target) => out.extend_from_slice(&target.encode()),
            StatementKind::BindAwarenessTarget => out.push(0x11),
            StatementKind::BindCurrentTarget => out.push(0x13),
            StatementKind::BindTargetArea(area) => {
                out.push(0x1a);
                out.extend_from_slice(&area.to_be_bytes());
            }
            StatementKind::BindTargetGroundPoint(profile) => {
                out.extend_from_slice(&[0x49, *profile])
            }
            StatementKind::SetMode(mode) => out.extend_from_slice(&[0x40, *mode as u8]),
            StatementKind::ResolveTarget => out.push(0x4d),
            StatementKind::ReplenishRecoveryMeter => out.extend_from_slice(&[0x4e, 0]),
            StatementKind::ReplenishForagingMeter => out.extend_from_slice(&[0x4f, 0]),
            StatementKind::InitAreaChange {
                list,
                count,
                handler,
                end_policy,
            } => out.extend_from_slice(&[0x17, *list, *count, *handler, *end_policy]),
            StatementKind::TryChangeArea => out.push(0x18),
            StatementKind::BindScannedObject => out.push(0x2d),
            StatementKind::ClearUndetectedPlayerTrackingTimers => out.extend_from_slice(&[0x5b, 0]),
            StatementKind::SelectPerceptionProfile(index) => out.extend_from_slice(&[0x2e, *index]),
            StatementKind::IncrementRandomValue => out.push(0x84),
            StatementKind::If {
                condition,
                then_body,
                else_body,
            } => {
                let encoding = condition.encoding();
                if let super::condition::Condition::TargetAngleIn { min, max } = condition {
                    let actual_min = Degrees::from_native(min.native()).value();
                    let actual_max = Degrees::from_native(max.native()).value();
                    if min.value() != actual_min || max.value() != actual_max {
                        self.warnings.push(Diagnostic::at(statement.line, statement.column, format!(
                            "target_angle_in({}, {}) quantized/clamped to [{actual_min}, {actual_max}] degrees; native maximum is 358.59375",
                            min.value(), max.value()
                        )));
                    }
                }
                out.extend_from_slice(&encoding.begin);
                self.encode_branch(statement, "if", then_body, continuation, scope, out)?;
                if else_body.is_some() || !continuation.is_empty() {
                    out.extend_from_slice(encoding.otherwise);
                    self.encode_branch(
                        statement,
                        "else",
                        else_body.as_deref().unwrap_or_default(),
                        continuation,
                        scope,
                        out,
                    )?;
                }
                out.extend_from_slice(encoding.end);
            }
            StatementKind::Return | StatementKind::Pass => {
                unreachable!("handled by encode_body")
            }
            StatementKind::Call { callee, args } => match callee {
                Callee::Action { group, id } => {
                    self.require_args(statement, args, 1, "an action call")?;
                    out.extend_from_slice(&bytecode::encode_action(*group, *id, args[0]));
                }
                Callee::Name(name) => {
                    if let Some(command) = reserved_command(name) {
                        self.require_args(statement, args, command.args, &format!("'{name}'"))?;
                        out.extend_from_slice(command.opcode);
                        out.extend_from_slice(args);
                    } else if let Some(&(group, id)) = self.names.actions.get(name.as_str()) {
                        self.require_args(statement, args, 1, "an action call")?;
                        out.extend_from_slice(&bytecode::encode_action(group, id, args[0]));
                    } else {
                        return Err(statement.error(format!(
                            "unknown name '{name}': not a reserved command and not declared in the actions block (spec §6)"
                        )));
                    }
                }
            },
            StatementKind::Transition { state } => {
                if state == "main" {
                    return Err(
                        statement.error("use restart; to re-enter main, not transition main;")
                    );
                }
                if scope != Scope::States {
                    return Err(statement.error(
                        "transition is only valid inside a states block: 0x07 rewrites the global main index, so an event lane would jump into the state table and never return to its own cursor (spec §7)",
                    ));
                }
                let index = self
                    .names
                    .states
                    .get(state.as_str())
                    .copied()
                    .ok_or_else(|| {
                        statement.error(format!(
                            "transition target '{state}' is not declared in the states block"
                        ))
                    })?;
                out.extend_from_slice(&bytecode::encode_main_jump(index));
            }
            StatementKind::Reset => out.extend_from_slice(&[0xff, 0x00]),
            StatementKind::ResetForgetTarget => out.extend_from_slice(&[0xff, 0xf7]),
            StatementKind::Restart => {
                if scope != Scope::States {
                    return Err(statement.error(
                        "restart is only valid inside a states block: it reloads the state table entry main[0][0] (spec §7)",
                    ));
                }
                out.push(0x04);
            }
            StatementKind::Native { bytes } => {
                out.extend_from_slice(bytes);
                self.warnings.push(Diagnostic::at(
                    statement.line,
                    statement.column,
                    format!(
                        "native(...) keeps its bytes verbatim: {}. Instruction widths and native block structure are checked (spec §6)",
                        describe_escape(bytes)
                    ),
                ));
            }
        }
        self.raw_return_blocked = previous_blocked;
        Ok(())
    }

    fn require_args(
        &self,
        statement: &Statement,
        args: &[u8],
        expected: usize,
        what: &str,
    ) -> Result<()> {
        if args.len() != expected {
            return Err(statement.error(format!(
                "{what} takes exactly {expected} argument(s), found {}",
                args.len()
            )));
        }
        Ok(())
    }
}

/// The native protocol hands control back through one shared byte rather than a
/// stack of request contexts, so a handler is only reachable from `handle` or
/// from the tail of another handler.
fn validate_handlers(document: &Document) -> Result<()> {
    let functions: HashMap<&str, &Function> = document
        .functions
        .iter()
        .map(|function| (function.name.as_str(), function))
        .collect();

    fn validate_body(
        body: &[Statement],
        inside_handler: bool,
        tail: bool,
        functions: &HashMap<&str, &Function>,
    ) -> Result<()> {
        for (index, statement) in body.iter().enumerate() {
            let tail_here = tail && index + 1 == body.len();
            match &statement.kind {
                StatementKind::Handle { handler, then_body } => {
                    if inside_handler {
                        return Err(statement.error(
                            "a handler cannot start another handle block; dispatch from the ordinary entry",
                        ));
                    }
                    match functions.get(handler.as_str()).copied() {
                        Some(function) if function.handler => {}
                        Some(_) => {
                            return Err(statement.error(format!(
                                "handle target '{handler}' must be declared with `handler fn`"
                            )));
                        }
                        None => {
                            return Err(statement.error(format!(
                                "handle target '{handler}' is not a declared function"
                            )));
                        }
                    }
                    // The protocol owns the takeover byte, so `then` is the
                    // caller's code and cannot leave it with a lexical return.
                    if then_body.iter().any(contains_exit) {
                        return Err(statement.error(
                            "return; and pass; cannot appear in a then block; write end; or return from the enclosing function after the block",
                        ));
                    }
                    validate_body(then_body, false, tail_here, functions)?;
                }
                StatementKind::Call {
                    callee: Callee::Name(name),
                    ..
                } if functions
                    .get(name.as_str())
                    .is_some_and(|function| function.handler) =>
                {
                    if !inside_handler {
                        return Err(statement.error(format!(
                            "'{name}' is a request handler; enter it with `handle {name}() then {{ ... }}`"
                        )));
                    }
                    if !tail_here {
                        return Err(statement.error(format!(
                            "a handler call must be the last action on its path: '{name}'"
                        )));
                    }
                }
                StatementKind::EntryBody(body) => {
                    validate_body(body, inside_handler, tail_here, functions)?;
                }
                StatementKind::If {
                    then_body,
                    else_body,
                    ..
                } => {
                    validate_body(then_body, inside_handler, tail_here, functions)?;
                    if let Some(body) = else_body {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                }
                StatementKind::Random(branches) => {
                    for (_, body) in branches {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                }
                StatementKind::ContextQuery {
                    branches, fallback, ..
                }
                | StatementKind::AreaRouteProfile { branches, fallback } => {
                    for (_, body) in branches {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                    validate_body(fallback, inside_handler, tail_here, functions)?;
                }
                StatementKind::Area { branches, fallback }
                | StatementKind::SpeciesGroup { branches, fallback }
                | StatementKind::Request { branches, fallback }
                | StatementKind::DebugMode { branches, fallback }
                | StatementKind::Species { branches, fallback } => {
                    for (_, body) in branches {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                    if let Some(fallback) = fallback {
                        validate_body(fallback, inside_handler, tail_here, functions)?;
                    }
                }
                StatementKind::TargetDistanceGroups(branches) => {
                    for body in branches {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                }
                StatementKind::TargetAngle { branches, fallback } => {
                    for (_, body) in branches {
                        validate_body(body, inside_handler, tail_here, functions)?;
                    }
                    if let Some(fallback) = fallback {
                        validate_body(fallback, inside_handler, tail_here, functions)?;
                    }
                }
                _ => {}
            }
        }
        Ok(())
    }

    for function in &document.functions {
        validate_body(&function.body, function.handler, true, &functions)?;
    }
    for body in document
        .states
        .iter()
        .filter_map(|decl| decl.body.as_ref())
        .chain(document.events.iter().filter_map(|decl| decl.body.as_ref()))
    {
        validate_body(body, false, true, &functions)?;
    }

    // An ordinary function reached from a handler must not start a second
    // request protocol of its own.
    fn contains_handle(body: &[Statement]) -> bool {
        body.iter().any(|statement| match &statement.kind {
            StatementKind::Handle { .. } => true,
            StatementKind::EntryBody(body) => contains_handle(body),
            StatementKind::If {
                then_body,
                else_body,
                ..
            } => contains_handle(then_body) || else_body.as_deref().is_some_and(contains_handle),
            StatementKind::Random(branches) => {
                branches.iter().any(|(_, body)| contains_handle(body))
            }
            StatementKind::ContextQuery {
                branches, fallback, ..
            }
            | StatementKind::AreaRouteProfile { branches, fallback } => {
                branches.iter().any(|(_, body)| contains_handle(body)) || contains_handle(fallback)
            }
            StatementKind::SpeciesGroup { branches, fallback }
            | StatementKind::Request { branches, fallback }
            | StatementKind::DebugMode { branches, fallback }
            | StatementKind::Species { branches, fallback }
            | StatementKind::Area { branches, fallback } => {
                branches.iter().any(|(_, body)| contains_handle(body))
                    || fallback.as_deref().is_some_and(contains_handle)
            }
            StatementKind::TargetDistanceGroups(branches) => {
                branches.iter().any(|body| contains_handle(body))
            }
            StatementKind::TargetAngle { branches, fallback } => {
                branches.iter().any(|(_, body)| contains_handle(body))
                    || fallback.as_deref().is_some_and(contains_handle)
            }
            _ => false,
        })
    }
    for function in document
        .functions
        .iter()
        .filter(|function| function.handler)
    {
        let mut pending = called_functions(&function.body);
        let mut visited = HashSet::new();
        while let Some(name) = pending.pop() {
            if !visited.insert(name) {
                continue;
            }
            let Some(callee) = functions.get(name).copied() else {
                continue;
            };
            if contains_handle(&callee.body) {
                return Err(Error::new(format!(
                    "{}: a function reached from a handler cannot start another handle block",
                    callee.name
                )));
            }
            pending.extend(called_functions(&callee.body));
        }
    }
    Ok(())
}

/// Every named call inside a body, including nested branches.
pub(super) fn called_functions(body: &[Statement]) -> Vec<&str> {
    let mut names = Vec::new();
    for statement in body {
        match &statement.kind {
            StatementKind::Call {
                callee: Callee::Name(name),
                ..
            } => names.push(name.as_str()),
            StatementKind::EntryBody(body) => names.extend(called_functions(body)),
            StatementKind::Handle { handler, then_body } => {
                names.push(handler.as_str());
                names.extend(called_functions(then_body));
            }
            StatementKind::If {
                then_body,
                else_body,
                ..
            } => {
                names.extend(called_functions(then_body));
                if let Some(body) = else_body {
                    names.extend(called_functions(body));
                }
            }
            StatementKind::Random(branches) => {
                for (_, body) in branches {
                    names.extend(called_functions(body));
                }
            }
            StatementKind::ContextQuery {
                branches, fallback, ..
            }
            | StatementKind::AreaRouteProfile { branches, fallback } => {
                for (_, body) in branches {
                    names.extend(called_functions(body));
                }
                names.extend(called_functions(fallback));
            }
            StatementKind::SpeciesGroup { branches, fallback }
            | StatementKind::Request { branches, fallback }
            | StatementKind::DebugMode { branches, fallback }
            | StatementKind::Species { branches, fallback }
            | StatementKind::Area { branches, fallback } => {
                for (_, body) in branches {
                    names.extend(called_functions(body));
                }
                if let Some(fallback) = fallback {
                    names.extend(called_functions(fallback));
                }
            }
            StatementKind::TargetDistanceGroups(branches) => {
                for body in branches {
                    names.extend(called_functions(body));
                }
            }
            StatementKind::TargetAngle { branches, fallback } => {
                for (_, body) in branches {
                    names.extend(called_functions(body));
                }
                if let Some(fallback) = fallback {
                    names.extend(called_functions(fallback));
                }
            }
            _ => {}
        }
    }
    names
}

/// Calls and anonymous entries have their own return boundary.
fn contains_exit(statement: &Statement) -> bool {
    match &statement.kind {
        StatementKind::Return | StatementKind::Pass => true,
        StatementKind::Handle { then_body, .. } => then_body.iter().any(contains_exit),
        StatementKind::If {
            then_body,
            else_body,
            ..
        } => then_body
            .iter()
            .chain(else_body.iter().flatten())
            .any(contains_exit),
        StatementKind::Random(branches) => branches
            .iter()
            .any(|(_, body)| body.iter().any(contains_exit)),
        StatementKind::ContextQuery {
            branches, fallback, ..
        }
        | StatementKind::AreaRouteProfile { branches, fallback } => branches
            .iter()
            .flat_map(|(_, body)| body)
            .chain(fallback)
            .any(contains_exit),
        StatementKind::SpeciesGroup { branches, fallback }
        | StatementKind::Request { branches, fallback }
        | StatementKind::DebugMode { branches, fallback }
        | StatementKind::Species { branches, fallback }
        | StatementKind::Area { branches, fallback } => branches
            .iter()
            .flat_map(|(_, body)| body)
            .chain(fallback.iter().flatten())
            .any(contains_exit),
        StatementKind::TargetDistanceGroups(branches) => {
            branches.iter().any(|body| body.iter().any(contains_exit))
        }
        StatementKind::TargetAngle { branches, fallback } => branches
            .iter()
            .flat_map(|(_, body)| body)
            .chain(fallback.iter().flatten())
            .any(contains_exit),
        _ => false,
    }
}

/// Say what a `native(...)` escape is standing in for, using the same
/// classification as the codec: a dispatched opcode that has no name yet, or a
/// byte that reaches the interpreter's switch default (spec §5).
fn describe_escape(bytes: &[u8]) -> String {
    let Some(&opcode) = bytes.first() else {
        return "the statement has no bytes".to_owned();
    };
    if bytecode::is_stop(opcode) {
        format!(
            "0x{opcode:02x} reaches the interpreter's switch default, which halts the script and rewinds the cursor"
        )
    } else {
        format!(
            "0x{opcode:02x} is dispatched by the interpreter but has no name in this vocabulary"
        )
    }
}

fn check_decodes(bytes: &[u8], line: usize, column: usize, what: &str) -> Result<()> {
    bytecode::decode(bytes).map_err(|error| Error::at(line, column, format!("{what}: {error}")))?;
    Ok(())
}

/// Endings verified in 108675A0 and the corresponding native event scripts.
/// Entry returns end their lane; subscript returns use their native slot.
fn finish(bytes: &mut Vec<u8>, ending: u8, slot: Option<NativeSlot>) -> Result<()> {
    let mut structure = bytecode::ScriptStructure::default();
    let mut terminal = false;
    for instruction in bytecode::decode(bytes)? {
        structure.push(&instruction.bytes)?;
        terminal = structure.is_closed()
            && (slot.is_some_and(|slot| slot.is_same_level_call(&instruction.bytes))
                || bytecode::is_stop(instruction.opcode)
                || matches!(instruction.opcode, 0x04 | 0x07 | 0x68)
                || (instruction.opcode == 0xff
                    && matches!(instruction.bytes.get(1), Some(0..=3 | 0xf5..=0xff))));
    }
    if !structure.is_closed() {
        return Err(Error::new(
            "function ends inside a native conditional block",
        ));
    }
    if !terminal {
        bytes.extend_from_slice(&[0xff, ending]);
    }
    if bytes.len() > super::super::decompile::MAX_SCRIPT_BYTES {
        return Err(Error::new("expanded function exceeds 64 KiB"));
    }
    Ok(())
}

fn ensure_root_table(program: &mut Program, root_index: usize) -> usize {
    let Node::Table(root) = &program.nodes[program.root] else {
        unreachable!()
    };
    if let Some(table) = root.get(root_index) {
        return table;
    }
    let table = program.nodes.len();
    program.nodes.push(Node::Table(Table::new()));
    let Node::Table(root) = &mut program.nodes[program.root] else {
        unreachable!()
    };
    root.insert(root_index, table);
    table
}

fn closed(bytes: &[u8]) -> Result<bool> {
    let mut structure = bytecode::ScriptStructure::default();
    for instruction in bytecode::decode(bytes)? {
        structure.push(&instruction.bytes)?;
    }
    Ok(structure.is_closed())
}

fn function_location(function: &Function) -> SourceLocation {
    let mut source = function.source.clone();
    source.function = function
        .name
        .rsplit("::")
        .next()
        .unwrap_or(&function.name)
        .into();
    source
}

impl Document {
    fn declaration_location(&self, name: &str, line: usize, column: usize) -> SourceLocation {
        let file = &self.source_files[0];
        let byte_start = file
            .source
            .split_inclusive('\n')
            .take(line.saturating_sub(1))
            .map(str::len)
            .sum::<usize>()
            + column.saturating_sub(1);
        SourceLocation {
            path: file.path.clone(),
            function: name.into(),
            line,
            column,
            end_line: line,
            end_column: column,
            byte_start,
            byte_end: byte_start,
        }
    }

    /// Compile the document into a pointer-free graph.
    ///
    /// A `base native;` document compiles to a declaration: its tables carry
    /// only the indices it writes, and the rest waits for the native block.
    /// The binding merges that declaration onto the block it will replace.
    pub fn compile(&self) -> Result<Compiled> {
        check_document(self)?;
        validate_handlers(self)?;
        if self.module || !self.imports.is_empty() {
            return Err(Error::new(
                "compile an entry project to resolve imports; a module is not an entry",
            ));
        }
        if self.auto_finish {
            for body in self
                .states
                .iter()
                .filter_map(|d| d.body.as_ref())
                .chain(self.events.iter().filter_map(|d| d.body.as_ref()))
            {
                if matches!(
                    body.as_slice(),
                    [Statement {
                        kind: StatementKind::EntryBody(_),
                        ..
                    }]
                ) {
                    continue;
                }
                if let [
                    Statement {
                        kind:
                            StatementKind::Call {
                                callee: Callee::Name(name),
                                args,
                            },
                        ..
                    },
                ] = body.as_slice()
                    && args.is_empty()
                    && self.functions.iter().any(|f| f.name == *name)
                {
                    continue;
                }
                return Err(Error::new(
                    "states/events entries require => followed by a declared function or a block",
                ));
            }
        }
        let allocation = Allocation::new(self)?;
        let mut compiler = Compiler {
            mappings: Vec::new(),
            owner: SourceLocation::default(),
            auto_finish: self.auto_finish,
            names: Names::collect(self),
            warnings: Vec::new(),
            functions: &self.functions,
            allocation: &allocation,
            relocations: Vec::new(),
            native_scope: None,
            raw_boundary: 0,
            raw_return_blocked: false,
            return_ending: None,
        };
        let states = self.encode_states(&mut compiler)?;
        let events = self.encode_events(&mut compiler)?;
        let mut debug_info = DebugInfo {
            files: self.source_files.clone(),
            mappings: Vec::new(),
        };
        let mut program = match self.base {
            Base::Empty => assemble_empty(self, states, events, &mut debug_info)?,
            Base::Native => assemble_native(self, states, events, &mut debug_info)?,
        };
        self.encode_native_functions(&mut compiler, &mut program, &mut debug_info)?;
        program.automatic_slots = allocation.automatic.iter().copied().collect();
        program.validate_lossless()?;
        Ok(Compiled {
            program,
            warnings: compiler.warnings,
            debug_info,
        })
    }

    fn encode_native_functions(
        &self,
        compiler: &mut Compiler<'_>,
        program: &mut Program,
        debug_info: &mut DebugInfo,
    ) -> Result<()> {
        let event_functions = event_functions(self);
        let mut functions: Vec<_> = self
            .functions
            .iter()
            .filter(|function| function.name != "main")
            .collect();
        functions.sort_by_key(|function| compiler.allocation.slots[&function.name]);
        for function in functions {
            let slot = compiler.allocation.slots[&function.name];
            let scope = if event_functions.contains(function.name.as_str()) {
                Scope::Events
            } else {
                Scope::States
            };
            let encoded = compiler
                .encode_script(
                    &function.body,
                    scope,
                    Some(slot),
                    Some(slot.ending()),
                    function_location(function),
                )
                .map_err(|error| Error::new(format!("{}: {error}", function.name)))?;
            let script = push_script(
                &mut program.nodes,
                &mut program.relocations,
                encoded,
                debug_info,
            );
            let table = ensure_root_table(program, slot.table);
            let Node::Table(table) = &mut program.nodes[table] else {
                unreachable!()
            };
            table.insert(usize::from(slot.index), script);
        }
        Ok(())
    }

    fn encode_states(
        &self,
        compiler: &mut Compiler<'_>,
    ) -> Result<Vec<(u8, Option<EncodedScript>)>> {
        let mut encoded = Vec::with_capacity(self.states.len());
        if let Some(main) = self.functions.iter().find(|f| f.name == "main") {
            let script = compiler.encode_script(
                &main.body,
                Scope::States,
                None,
                Some(0),
                function_location(main),
            )?;
            check_decodes(&script.bytes, main.line, main.column, "main")?;
            encoded.push((0, Some(script)));
        }
        for decl in &self.states {
            let Some(body) = &decl.body else {
                encoded.push((decl.index, None));
                continue;
            };
            let script = compiler.encode_script(
                body,
                Scope::States,
                None,
                self.auto_finish.then_some(0),
                self.declaration_location(&decl.name, decl.line, decl.column),
            )?;
            check_decodes(
                &script.bytes,
                decl.line,
                decl.column,
                &format!("state '{}'", decl.name),
            )?;
            encoded.push((decl.index, Some(script)));
        }
        Ok(encoded)
    }

    fn encode_events(
        &self,
        compiler: &mut Compiler<'_>,
    ) -> Result<Vec<(u8, Option<EncodedScript>)>> {
        let mut encoded = Vec::with_capacity(self.events.len());
        for decl in &self.events {
            let Some(body) = &decl.body else {
                encoded.push((decl.slot, None));
                continue;
            };
            let event = &EVENT_SLOTS[usize::from(decl.slot)];
            let script = compiler.encode_script(
                body,
                Scope::Events,
                None,
                self.auto_finish.then_some(event.ending),
                self.declaration_location(event.name, decl.line, decl.column),
            )?;
            let what = format!("event '{}'", event.name);
            check_decodes(&script.bytes, decl.line, decl.column, &what)?;
            encoded.push((decl.slot, Some(script)));
        }
        Ok(encoded)
    }
}

/// Build a self-contained graph: the descriptor, the state table, and one
/// script per declared entry.  An index the document does not write is empty.
fn assemble_empty(
    document: &Document,
    states: Vec<(u8, Option<EncodedScript>)>,
    events: Vec<(u8, Option<EncodedScript>)>,
    debug_info: &mut DebugInfo,
) -> Result<Program> {
    if states.is_empty() {
        return Err(Error::new(
            "an empty base needs a states block: the interpreter enters through the state table's entry 0, and there is no native table to inherit (spec §8)",
        ));
    }

    let main_index = 1;
    let mut nodes = vec![Node::Table(Table::new()), Node::Table(Table::new())];
    let mut relocations = Vec::new();
    let mut root = Table::new();
    root.insert(0, main_index);
    let mut main = Table::new();

    for (index, body) in states {
        let Some(script) = body else { continue };
        let node = push_script(&mut nodes, &mut relocations, script, debug_info);
        main.insert(usize::from(index), node);
    }
    if main.get(0).is_none() {
        return Err(Error::new(
            "state index 0 needs a script: the interpreter enters through the state table's entry 0 (spec §8)",
        ));
    }

    for (slot, body) in events {
        let Some(encoded) = body else { continue };
        let script = push_script(&mut nodes, &mut relocations, encoded, debug_info);
        let cell = nodes.len();
        nodes.push(Node::Table(Table::from_entries([(0, Some(script))])));
        root.insert(EVENT_SLOTS[usize::from(slot)].root_index, cell);
    }

    nodes[main_index] = Node::Table(main);
    nodes[0] = Node::Table(root);
    Ok(Program {
        species: document.species,
        base: Base::Empty,
        root: 0,
        nodes,
        automatic_slots: Vec::new(),
        relocations,
    })
}

/// Build a `base native;` declaration: every index the document does not write
/// keeps the native entry at the same position.
///
/// The compiler holds the native layout but never the native data, so it writes
/// nothing it was not told to: a table carries only the declared indices and no
/// length (spec §8.1).  The state table gets a local node only when the
/// document declares a state at all; a document that declares none keeps the
/// whole descriptor.
fn assemble_native(
    document: &Document,
    states: Vec<(u8, Option<EncodedScript>)>,
    events: Vec<(u8, Option<EncodedScript>)>,
    debug_info: &mut DebugInfo,
) -> Result<Program> {
    let mut nodes = vec![Node::Table(Table::new())];
    let mut relocations = Vec::new();
    let mut root = Table::new();

    if !states.is_empty() {
        let main_index = nodes.len();
        nodes.push(Node::Table(Table::new()));
        let mut main = Table::new();
        for (index, body) in states {
            let index = usize::from(index);
            match body {
                Some(encoded) => {
                    let node = push_script(&mut nodes, &mut relocations, encoded, debug_info);
                    main.insert(index, node);
                }
                // A bare entry clears the slot it names.
                None => main.clear(index),
            };
        }
        if main.declares(0) && main.get(0).is_none() {
            return Err(Error::new(
                "clearing state index 0 would leave the state table unenterable: the interpreter enters through its entry 0 (spec §8)",
            ));
        }
        nodes[main_index] = Node::Table(main);
        root.insert(0, main_index);
    }

    for (slot, body) in events {
        let root_index = EVENT_SLOTS[usize::from(slot)].root_index;
        match body {
            Some(encoded) => {
                let script = push_script(&mut nodes, &mut relocations, encoded, debug_info);
                let cell = nodes.len();
                nodes.push(Node::Table(Table::from_entries([(0, Some(script))])));
                root.insert(root_index, cell);
            }
            // A bare entry clears the slot it names.
            None => root.clear(root_index),
        };
    }

    nodes[0] = Node::Table(root);
    Ok(Program {
        species: document.species,
        base: Base::Native,
        root: 0,
        nodes,
        automatic_slots: Vec::new(),
        relocations,
    })
}
