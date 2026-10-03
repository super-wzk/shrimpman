//! Bounded extraction of reachable state scripts and event entry scripts.
//!
//! Native tables have no lengths. Follow explicit state references instead of
//! guessing a table end from adjacent pointers. Explicit 81/82/16 references
//! recover annotated functions. Other tables remain
//! inherited through `base native`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::dsl::{
    condition::{ConditionMarker, Degrees, Mode},
    slot::NativeSlot,
    target::{EntityTarget, PointTarget},
};
use super::{Error, Result, bytecode, control::EVENT_SLOTS};

pub const MAX_SCRIPT_BYTES: usize = 64 * 1024;
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum ScriptRef {
    State(u8),
    Subscript(NativeSlot),
}

/// A checked byte reader. Implementations must report unreadable memory.
pub trait Memory {
    fn bytes(&self, address: u32, length: usize) -> Result<Vec<u8>>;

    fn word(&self, address: u32) -> Result<u32> {
        let bytes = self.bytes(address, 4)?;
        let bytes: [u8; 4] = bytes
            .try_into()
            .map_err(|_| Error::new("short pointer read"))?;
        Ok(u32::from_le_bytes(bytes))
    }
}

#[derive(Debug)]
pub struct Decompiled {
    pub source: String,
    pub warnings: Vec<String>,
}

fn indexed(address: u32, index: usize) -> Result<u32> {
    if address == 0 {
        return Err(Error::new("null AI table"));
    }
    address
        .checked_add((index * 4) as u32)
        .ok_or_else(|| Error::new("AI address overflow"))
}

/// Extract state 0, the current state and their explicit 07 references,
/// plus each of the seven event entry points. A failed entry is inherited,
/// never emitted as an empty script. At least one body must be recoverable.
pub fn decompile(
    memory: &impl Memory,
    descriptor: u32,
    species: u8,
    current: u8,
    map: Option<u32>,
) -> Result<Decompiled> {
    let table = memory.word(indexed(descriptor, 0)?)?;
    let mut pending = BTreeSet::from([ScriptRef::State(0), ScriptRef::State(current)]);
    let mut visited = BTreeSet::new();
    let mut states = BTreeMap::new();
    let mut events = BTreeMap::new();
    let mut subs = BTreeMap::new();
    let mut warnings = Vec::new();
    let mut total = 0;
    for (index, slot) in EVENT_SLOTS.iter().enumerate() {
        let result = (|| {
            let cell = memory.word(indexed(descriptor, slot.root_index)?)?;
            if cell == 0 {
                return Ok(None);
            }
            let address = memory.word(cell)?;
            if address == 0 {
                return Ok(None);
            }
            script(memory, address).map(Some)
        })();
        match result {
            Ok(Some(body)) => {
                references(&body, &mut pending);
                total += body.len();
                events.insert(index, body);
            }
            Ok(None) => {}
            Err(error) => warnings.push(format!("事件 {index} 沿用原生：{error}")),
        }
    }
    while let Some(reference) = pending.pop_first() {
        if !visited.insert(reference) {
            continue;
        }
        if visited.len() > 4096 {
            return Err(Error::new("AI extraction exceeds 4096 entries"));
        }
        let result = (|| {
            let (table, index, slot) = match reference {
                ScriptRef::State(index) => (table, index, None),
                ScriptRef::Subscript(slot) => (
                    memory.word(indexed(descriptor, slot.table)?)?,
                    slot.index,
                    Some(slot),
                ),
            };
            let address = memory.word(indexed(table, usize::from(index))?)?;
            extract_script(memory, address, slot)
        })();
        match result {
            Ok(body) => {
                total += body.len();
                if total > super::MAX_PAYLOAD {
                    return Err(Error::new("AI extraction exceeds byte budget"));
                }
                references(&body, &mut pending);
                match reference {
                    ScriptRef::State(index) => {
                        states.insert(index, body);
                    }
                    ScriptRef::Subscript(slot) => {
                        subs.insert(slot, body);
                    }
                }
            }
            Err(error) => warnings.push(match reference {
                ScriptRef::State(index) => format!("状态 {index} 沿用原生：{error}"),
                ScriptRef::Subscript(slot) => {
                    format!("表 {} 项 {} 沿用原生：{error}", slot.table, slot.index)
                }
            }),
        }
    }
    if states.is_empty() && events.is_empty() {
        return Err(Error::new(format!(
            "没有可反编译的脚本：{}",
            warnings.join("；")
        )));
    }
    let mut source = format!("mhf_ai 1;\nspecies {species};\n");
    if let Some(map) = map {
        writeln!(source, "map {map};").unwrap();
    }
    source.push_str(
        "base native;\n\n// 部分导出：已追踪状态、事件和子脚本；未导出的表项沿用原生。\n",
    );
    for warning in &warnings {
        writeln!(source, "// {warning}").unwrap();
    }
    let handlers = handler_slots(&states, &events, &subs);
    let state_context = BodyContext {
        states: Some(&states),
        subs: &subs,
        handlers: &handlers,
        allow_implicit_pass: false,
        return_ending: None,
    };
    // Entries already own a script. Named functions always make native calls,
    // so anonymous bodies preserve the entry bytes without an extra call layer.
    source.push_str("\nstates {\n");
    for (&index, bytes) in &states {
        if index == 0 {
            continue;
        }
        writeln!(source, "    state_{index} = {index} => {{").unwrap();
        append_indented_body(
            &mut source,
            without_automatic_tail(bytes, 0)?,
            1,
            &state_context,
        )?;
        source.push_str("    }\n");
    }
    source.push_str("}\n\nevents {\n");
    for (&index, bytes) in &events {
        let event = &EVENT_SLOTS[index];
        writeln!(source, "    {} => {{", event.name).unwrap();
        let event_context = BodyContext {
            states: None,
            return_ending: Some(event.ending),
            ..state_context
        };
        append_indented_body(
            &mut source,
            without_automatic_tail(bytes, event.ending)?,
            1,
            &event_context,
        )?;
        source.push_str("    }\n");
    }
    source.push_str("}\n");
    if let Some(bytes) = states.get(&0) {
        source.push_str("\nfn main() {\n");
        format_body(
            &mut source,
            without_automatic_tail(bytes, 0)?,
            &state_context,
        )?;
        source.push_str("}\n");
    }
    for (slot, bytes) in &subs {
        let handler = handlers.contains(slot);
        writeln!(
            source,
            "\n@slot(table = {}, index = {})\n{}fn {}() {{",
            slot.table,
            slot.index,
            if handler { "handler " } else { "" },
            sub_name(*slot)
        )
        .unwrap();
        format_body(
            &mut source,
            without_automatic_tail(bytes, slot.ending())?,
            &BodyContext {
                states: None,
                subs: &subs,
                handlers: &handlers,
                allow_implicit_pass: handler,
                return_ending: Some(slot.ending()),
            },
        )?;
        source.push_str("}\n");
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::new("AI source exceeds size limit"));
    }
    let compiled = super::dsl::parse(&source)?.compile()?;
    let super::Node::Table(root) = &compiled.program.nodes[compiled.program.root] else {
        unreachable!()
    };
    let scripts = states
        .iter()
        .map(|(&index, bytes)| (0, usize::from(index), Some(ScriptRef::State(index)), bytes))
        .chain(
            events
                .iter()
                .map(|(&index, bytes)| (EVENT_SLOTS[index].root_index, 0, None, bytes)),
        )
        .chain(subs.iter().map(|(&slot, bytes)| {
            (
                slot.table,
                usize::from(slot.index),
                Some(ScriptRef::Subscript(slot)),
                bytes,
            )
        }));
    for (root_index, index, reference, bytes) in scripts {
        let super::Node::Table(table) = &compiled.program.nodes[root.get(root_index).unwrap()]
        else {
            unreachable!()
        };
        if !matches!(
            &compiled.program.nodes[table.get(index).unwrap()],
            super::Node::Script(actual) if matches_export(actual, bytes)?
        ) {
            return Err(Error::new(match reference {
                Some(ScriptRef::State(_)) => "state round-trip mismatch".into(),
                None => "event round-trip mismatch".into(),
                Some(ScriptRef::Subscript(slot)) => {
                    format!(
                        "subscript round-trip mismatch: {}:{}",
                        slot.table, slot.index
                    )
                }
            }));
        }
    }
    Ok(Decompiled { source, warnings })
}

/// The only canonicalization permitted on export is the equivalent RNG opcode.
/// Decode boundaries so operand bytes with value 7B are never rewritten.
fn matches_export(actual: &[u8], original: &[u8]) -> Result<bool> {
    let instructions = bytecode::decode(original)?;
    Ok(actual.len() == original.len()
        && instructions.iter().all(|instruction| {
            let expected = if instruction.bytes == [0x7b] {
                &[0x84][..]
            } else {
                &instruction.bytes
            };
            actual[instruction.offset..instruction.offset + expected.len()] == *expected
        }))
}

/// Only omit a complete, matching instruction outside native conditional blocks.
/// The compiler reconstructs this tail; early and mismatched exits stay explicit.
fn without_automatic_tail(bytes: &[u8], ending: u8) -> Result<&[u8]> {
    let instructions = bytecode::decode(bytes)?;
    let mut structure = bytecode::ScriptStructure::default();
    for instruction in &instructions {
        structure.push(&instruction.bytes)?;
    }
    if structure.is_closed()
        && instructions
            .last()
            .is_some_and(|instruction| instruction.bytes == [0xff, ending])
    {
        Ok(&bytes[..bytes.len() - 2])
    } else {
        Ok(bytes)
    }
}

fn references(bytes: &[u8], pending: &mut BTreeSet<ScriptRef>) {
    for instruction in bytecode::decode(bytes).expect("extracted instructions decode") {
        if let [0x07, index] = instruction.bytes.as_slice() {
            pending.insert(ScriptRef::State(*index));
        } else if let Some(slot) = NativeSlot::from_call(&instruction.bytes) {
            pending.insert(ScriptRef::Subscript(slot));
        }
    }
}

fn script(memory: &impl Memory, address: u32) -> Result<Vec<u8>> {
    extract_script(memory, address, None)
}

/// Extract one script through a proven terminal outside native marker blocks.
///
/// Reads use the shared bytecode codec and structure checks, with the same
/// 64 KiB search budget as project decompilation. The reader owns the accessible
/// address range. Calls are retained without following their target pointers.
/// Pass a slot only when the caller knows its native execution level; without
/// that context, 81/82 calls do not establish a same-level tail-call boundary.
pub fn extract_script(
    memory: &impl Memory,
    address: u32,
    slot: Option<NativeSlot>,
) -> Result<Vec<u8>> {
    if address == 0 {
        return Err(Error::new("null script"));
    }
    let mut bytes = Vec::new();
    let mut structure = bytecode::ScriptStructure::default();
    while bytes.len() < MAX_SCRIPT_BYTES {
        let position = address
            .checked_add(bytes.len() as u32)
            .ok_or_else(|| Error::new("script address overflow"))?;
        let mut instruction = memory.bytes(position, 1)?;
        if instruction.len() != 1 {
            return Err(Error::new("short instruction read"));
        }
        // Read only the bytes the codec needs; do not probe beyond a terminal.
        let length = loop {
            if let Ok(length) = bytecode::instruction_len(&instruction) {
                break length;
            }
            if instruction.len() == 7 {
                return Err(Error::new("invalid instruction width"));
            }
            let needed = instruction.len() + 1;
            instruction = memory.bytes(position, needed)?;
            if instruction.len() != needed {
                return Err(Error::new("short instruction read"));
            }
        };
        instruction.truncate(length);
        let opcode = instruction[0];
        let selector = instruction.get(1).copied();
        structure
            .push(&instruction)
            .map_err(|error| Error::new(format!("{position:#010x}: {error}")))?;
        let terminal = bytecode::is_stop(opcode)
            || slot.is_some_and(|slot| slot.is_same_level_call(&instruction))
            || matches!(opcode, 0x04 | 0x07)
            || (opcode == 0xff && matches!(selector, Some(0..=3 | 0xf5..=0xff)));
        bytes.extend_from_slice(&instruction);
        if terminal && structure.is_closed() {
            return Ok(bytes);
        }
    }
    Err(Error::new("script has no proven end within 64 KiB"))
}

#[cfg(test)]
fn format_test_body(
    out: &mut String,
    bytes: &[u8],
    states: Option<&BTreeMap<u8, Vec<u8>>>,
) -> Result<()> {
    format_body(
        out,
        bytes,
        &BodyContext {
            states,
            subs: &BTreeMap::new(),
            handlers: &BTreeSet::new(),
            allow_implicit_pass: false,
            return_ending: None,
        },
    )
}

fn sub_name(slot: NativeSlot) -> String {
    format!("sub_{}_{}", slot.table, slot.index)
}

/// Keep ordinary calls separate from request-owned handler calls in one scan.
/// Any ordinary call site keeps its callee a plain subscript.
fn collect_targets(
    bytes: &[u8],
    ordinary: &mut BTreeSet<NativeSlot>,
    handled: &mut BTreeSet<NativeSlot>,
) {
    let Ok(instructions) = bytecode::decode(bytes) else {
        return;
    };
    let mut index = 0;
    while index < instructions.len() {
        if let Some(recovered) = recover_request(&instructions[index..]) {
            if let Some(slot) = NativeSlot::from_call(recovered.call) {
                handled.insert(slot);
            }
            collect_targets(&recovered.then_body, ordinary, handled);
            index += recovered.instruction_count;
            continue;
        }
        if let Some(slot) = NativeSlot::from_call(&instructions[index].bytes) {
            ordinary.insert(slot);
        }
        index += 1;
    }
}

/// A raw-rendered body cannot call a declared handler. Revisit these calls
/// whenever pruning changes which request protocols can be recovered.
fn collect_raw_call_targets(
    bytes: &[u8],
    handlers: &BTreeSet<NativeSlot>,
    targets: &mut BTreeSet<NativeSlot>,
) {
    let Ok(instructions) = bytecode::decode(bytes) else {
        return;
    };
    if !can_structure_body(&instructions, handlers) {
        targets.extend(
            instructions
                .iter()
                .filter_map(|instruction| NativeSlot::from_call(&instruction.bytes)),
        );
        return;
    }
    // Recovered `then` bodies are rendered separately and can themselves fall
    // back to raw instructions even when their caller is structured.
    let mut index = 0;
    while index < instructions.len() {
        if let Some(recovered) = recover_request(&instructions[index..]) {
            collect_raw_call_targets(&recovered.then_body, handlers, targets);
            index += recovered.instruction_count;
        } else {
            index += 1;
        }
    }
}

/// Decide which subscripts can be declared `handler fn`.
///
/// The compiler enters a handler only from `handle` or from the tail of another
/// handler, so a candidate must have no ordinary call sites, must keep its own
/// return paths lossless when rendered as `pass`, and must not reach a second dispatch.
fn handler_slots(
    states: &BTreeMap<u8, Vec<u8>>,
    events: &BTreeMap<usize, Vec<u8>>,
    subs: &BTreeMap<NativeSlot, Vec<u8>>,
) -> BTreeSet<NativeSlot> {
    let bodies = states.values().chain(events.values()).chain(subs.values());
    let mut referenced = BTreeSet::new();
    let mut handled = BTreeSet::new();
    for body in bodies {
        collect_targets(body, &mut referenced, &mut handled);
    }
    let mut candidates: BTreeSet<NativeSlot> = handled
        .into_iter()
        .filter(|slot| {
            !referenced.contains(slot)
                && subs
                    .get(slot)
                    .is_some_and(|body| handler_body_ok(body, slot.ending()))
        })
        .collect();
    // Dropping a candidate can turn a nested dispatch into the only remaining
    // problem, so the pruning repeats until the set stops shrinking.
    loop {
        let mut rejected = BTreeSet::new();
        for body in states.values().chain(events.values()).chain(subs.values()) {
            collect_raw_call_targets(body, &candidates, &mut rejected);
        }
        rejected.retain(|slot| candidates.contains(slot));
        for slot in &candidates {
            // A handler may not reach a second dispatch, and may not reach
            // another handler, because the compiler only allows a handler call
            // at the very tail of a handler, which a recovered body cannot prove.
            let mut pending = vec![*slot];
            let mut visited = BTreeSet::new();
            while let Some(current) = pending.pop() {
                if !visited.insert(current) {
                    continue;
                }
                let Some(body) = subs.get(&current) else {
                    continue;
                };
                let mut calls = BTreeSet::new();
                let mut handles = BTreeSet::new();
                collect_targets(body, &mut calls, &mut handles);
                let nested =
                    (current != *slot && !handles.is_empty()) || !calls.is_disjoint(&candidates);
                if nested {
                    rejected.insert(*slot);
                    break;
                }
                pending.extend(calls);
            }
        }
        if rejected.is_empty() {
            break;
        }
        candidates.retain(|slot| !rejected.contains(slot));
    }
    candidates
}

type BranchBodies = Vec<(u8, Vec<u8>)>;

struct RecoveredMatch {
    instruction_count: usize,
    selector: String,
    branches: Vec<(u16, Vec<u8>)>,
    fallback: Option<Vec<u8>>,
}

/// Only recover complete numeric matches that re-encode losslessly.
fn recover_match(instructions: &[bytecode::Instruction]) -> Option<RecoveredMatch> {
    let (opcode, count, selector) = match instructions.first()?.bytes.as_slice() {
        [0x15, 0, count] => (0x15, count, "self.area".to_owned()),
        [0x20, 0, count] => (0x20, count, "self.target_angle()".to_owned()),
        [0x2c, 0, count] => (0x2c, count, "self.species_group".to_owned()),
        [0x94, 0, count] => (0x94, count, "context.debug_mode".to_owned()),
        [0x1d, 0, count] => (0x1d, count, "self.request".to_owned()),
        [0x70, 0, count] => (0x70, count, "self.species".to_owned()),
        [0x57, 0, count] => (0x57, count, "self.area_route_profile".to_owned()),
        [0x79, 0, count, argument] => (0x79, count, format!("context.query({argument})")),
        _ => return None,
    };
    let case_value = |bytes: &[u8]| match bytes {
        [op @ (0x1d | 0x20 | 0x2c | 0x70 | 0x79 | 0x94), 1, value] if *op == opcode => {
            Some(u16::from(*value))
        }
        [0x57, 1, 0, value] if opcode == 0x57 => Some(u16::from(*value)),
        [0x15, 1, high, low] if opcode == 0x15 => Some(u16::from_be_bytes([*high, *low])),
        _ => None,
    };
    let first = case_value(&instructions.get(1)?.bytes)?;
    if *count == 0 {
        return None;
    }
    let mut branches = vec![(first, Vec::new())];
    let mut fallback: Option<Vec<u8>> = None;
    let mut structure = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate().skip(2) {
        if structure.is_closed() {
            match instruction.bytes.as_slice() {
                bytes if let Some(value) = case_value(bytes) => {
                    // Area, angle, and species-group matches keep source order;
                    // every other selector requires ascending cases.
                    if fallback.is_some()
                        || (!matches!(opcode, 0x15 | 0x20 | 0x2c) && value <= branches.last()?.0)
                    {
                        return None;
                    }
                    branches.push((value, Vec::new()));
                    continue;
                }
                [op, 2] if *op == opcode => {
                    if fallback.is_some() {
                        return None;
                    }
                    fallback = Some(Vec::new());
                    continue;
                }
                [op, 3] if *op == opcode => {
                    if branches.len() != usize::from(*count) {
                        return None;
                    }
                    // Route-profile and callback matches always carry an else.
                    if matches!(opcode, 0x57 | 0x79) && fallback.is_none() {
                        return None;
                    }
                    return Some(RecoveredMatch {
                        instruction_count: index + 1,
                        selector,
                        branches,
                        fallback,
                    });
                }
                _ => {}
            }
        }
        structure.push(&instruction.bytes).ok()?;
        let body = match &mut fallback {
            Some(body) => body,
            None => &mut branches.last_mut()?.1,
        };
        body.extend_from_slice(&instruction.bytes);
    }
    None
}

struct RecoveredRequest<'a> {
    instruction_count: usize,
    /// The native call that enters the handler.
    call: &'a [u8],
    then_body: Vec<u8>,
}

/// The canonical protocol: request guard, default takeover, handler call, the
/// takeover check, the `then` block, and both closing markers. Anything else,
/// including an `1B 01` else, stays a raw escape.
fn recover_request(instructions: &[bytecode::Instruction]) -> Option<RecoveredRequest<'_>> {
    if instructions.first()?.bytes != [0x1b, 0, 1]
        || instructions.get(1)?.bytes != [0x0c, 4, 1]
        || instructions.get(3)?.bytes != [0x2b, 0, 4, 1]
    {
        return None;
    }
    let call = instructions.get(2)?.bytes.as_slice();
    let mut structure = bytecode::ScriptStructure::default();
    let mut then_body = Vec::new();
    for (index, instruction) in instructions.iter().enumerate().skip(4) {
        let bytes = instruction.bytes.as_slice();
        if structure.is_closed() {
            if bytes == [0x2b, 2] {
                return (instructions.get(index + 1)?.bytes == [0x1b, 2]).then_some(
                    RecoveredRequest {
                        instruction_count: index + 2,
                        call,
                        then_body,
                    },
                );
            }
            if bytes == [0x2b, 1] {
                return None;
            }
        }
        structure.push(bytes).ok()?;
        then_body.extend_from_slice(bytes);
    }
    None
}

/// A clear can continue as `mark_unhandled()`. Clears recovered as `pass` must
/// finish their return path with only closing markers to preserve the bytes.
fn handler_body_ok(bytes: &[u8], ending: u8) -> bool {
    let Ok(instructions) = bytecode::decode(bytes) else {
        return false;
    };
    for (index, instruction) in instructions.iter().enumerate() {
        if instruction.bytes != [0x0d, 0x04] {
            continue;
        }
        let start = match instructions.get(index + 1) {
            Some(next) if next.bytes == [0xff, ending] => index + 2,
            Some(_) => continue,
            None => index + 1,
        };
        let mut structure = bytecode::ScriptStructure::default();
        if instructions[..start]
            .iter()
            .any(|instruction| structure.push(&instruction.bytes).is_err())
        {
            return false;
        }
        for instruction in &instructions[start..] {
            let depth = structure.depth();
            if structure.push(&instruction.bytes).is_err() || structure.depth() >= depth {
                return false;
            }
        }
        if !structure.is_closed() {
            return false;
        }
    }
    true
}

fn append_indented_body(
    out: &mut String,
    bytes: &[u8],
    indent: usize,
    context: &BodyContext,
) -> Result<()> {
    let mut rendered = String::new();
    format_body(&mut rendered, bytes, context)?;
    let prefix = "    ".repeat(indent);
    for line in rendered.lines() {
        writeln!(out, "{prefix}{line}").unwrap();
    }
    Ok(())
}

/// Recover canonical weighted choices or exhaustive distance groups.
/// Other counts, selectors and weights stay byte-preserving native escapes.
fn recover_choice(instructions: &[bytecode::Instruction]) -> Option<(usize, BranchBodies, bool)> {
    let [opcode @ (0x80 | 0x83), 0, count] = instructions.first()?.bytes.as_slice() else {
        return None;
    };
    let distance = *opcode == 0x83;
    let first = match instructions.get(1)?.bytes.as_slice() {
        [0x83, 1] if distance && (1..=4).contains(count) => 1,
        [0x80, 1, weight] if !distance && (1..=31).contains(count) && *weight <= 32 => *weight,
        _ => return None,
    };
    let mut branches = vec![(first, Vec::new())];
    let mut structure = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate().skip(2) {
        if structure.is_closed() {
            match instruction.bytes.as_slice() {
                [op, 0xff] if op == opcode => {
                    let complete = if distance {
                        branches.len() == usize::from(*count) + 1
                    } else {
                        branches.len() == usize::from(*count)
                            && branches
                                .iter()
                                .map(|(weight, _)| usize::from(*weight))
                                .sum::<usize>()
                                == 32
                    };
                    return complete.then_some((index + 1, branches, distance));
                }
                [0x83, selector] if distance => {
                    if usize::from(*selector) != branches.len() + 1 || *selector > count + 1 {
                        return None;
                    }
                    branches.push((*selector, Vec::new()));
                    continue;
                }
                [0x80, selector, weight] if !distance && *selector != 0 => {
                    if usize::from(*selector) != branches.len() + 1 || *weight > 32 {
                        return None;
                    }
                    branches.push((*weight, Vec::new()));
                    continue;
                }
                _ => {}
            }
        }
        structure.push(&instruction.bytes).ok()?;
        branches.last_mut()?.1.extend_from_slice(&instruction.bytes);
    }
    None
}

/// Everything a recovered body needs to name what it renders.
#[derive(Clone, Copy)]
struct BodyContext<'a> {
    states: Option<&'a BTreeMap<u8, Vec<u8>>>,
    subs: &'a BTreeMap<NativeSlot, Vec<u8>>,
    /// Subscripts entered only through `handle`, used to recover the protocol.
    handlers: &'a BTreeSet<NativeSlot>,
    /// A whole handler body may recover its stripped trailing return as `pass`.
    allow_implicit_pass: bool,
    /// Native return shared by all nested bodies in this function or event.
    return_ending: Option<u8>,
}

/// Whether `bytes` is the enclosing slot's own return instruction.
fn is_own_return(bytes: &[u8], context: &BodyContext) -> bool {
    let [0xff, ending] = bytes else {
        return false;
    };
    context.return_ending == Some(*ending)
}

fn can_structure_body(
    instructions: &[bytecode::Instruction],
    handlers: &BTreeSet<NativeSlot>,
) -> bool {
    // Only raise complete, single-else known blocks. Unusual native structures
    // remain byte-preserving escapes rather than acquiring new DSL semantics.
    let mut condition_blocks = Vec::new();
    let mut structured = true;
    let mut index = 0;
    while index < instructions.len() {
        let instruction = &instructions[index];
        let opcode = instruction.opcode;
        if instruction.bytes == [0x1b, 0, 1] {
            // The request protocol owns this guard: fold it whole or stay raw.
            match recover_request(&instructions[index..]).filter(|recovered| {
                NativeSlot::from_call(recovered.call).is_some_and(|slot| handlers.contains(&slot))
            }) {
                Some(recovered) => {
                    index += recovered.instruction_count;
                    continue;
                }
                None => structured = false,
            }
            index += 1;
            continue;
        }
        match ConditionMarker::decode(&instruction.bytes) {
            Some(ConditionMarker::Begin(_)) => condition_blocks.push((opcode, false)),
            Some(ConditionMarker::Else) => match condition_blocks.last_mut() {
                Some((open, seen_else)) if *open == opcode && !*seen_else => *seen_else = true,
                _ => structured = false,
            },
            Some(ConditionMarker::End) => {
                if !condition_blocks
                    .pop()
                    .is_some_and(|(open, _)| open == opcode)
                {
                    structured = false;
                }
            }
            Some(ConditionMarker::Unsupported) => structured = false,
            None => {}
        }
        index += 1;
    }
    structured && condition_blocks.is_empty()
}

fn format_body(out: &mut String, bytes: &[u8], context: &BodyContext) -> Result<()> {
    let instructions = bytecode::decode(bytes)?;
    let structured = can_structure_body(&instructions, context.handlers);
    // Nested blocks share explicit returns, but not the stripped function tail.
    let nested_context = BodyContext {
        allow_implicit_pass: false,
        ..*context
    };
    let mut indent = 1;
    let mut skip_until = 0;
    // Validate native markers even when a block cannot be recovered structurally.
    let mut raw_blocks = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate() {
        if index < skip_until {
            continue;
        }
        if structured
            && let Some(recovered) = recover_request(&instructions[index..])
            && let Some(target) =
                NativeSlot::from_call(recovered.call).filter(|slot| context.handlers.contains(slot))
        {
            writeln!(
                out,
                "{}handle {}() then {{",
                "    ".repeat(indent),
                sub_name(target)
            )
            .unwrap();
            // `then` is the caller's own code, so it is not a handler body.
            append_indented_body(out, &recovered.then_body, indent, &nested_context)?;
            writeln!(out, "{}}}", "    ".repeat(indent)).unwrap();
            skip_until = index + recovered.instruction_count;
            continue;
        }
        if structured && let Some(recovered) = recover_match(&instructions[index..]) {
            writeln!(
                out,
                "{}match {} {{",
                "    ".repeat(indent),
                recovered.selector
            )
            .unwrap();
            for (value, body) in &recovered.branches {
                let value = if instruction.opcode == 0x20 {
                    Degrees::from_native(*value as u8).value().to_string()
                } else {
                    value.to_string()
                };
                writeln!(out, "{}{value} => {{", "    ".repeat(indent + 1)).unwrap();
                append_indented_body(out, body, indent + 1, &nested_context)?;
                writeln!(out, "{}}}", "    ".repeat(indent + 1)).unwrap();
            }
            if let Some(fallback) = &recovered.fallback {
                writeln!(out, "{}else => {{", "    ".repeat(indent + 1)).unwrap();
                append_indented_body(out, fallback, indent + 1, &nested_context)?;
                writeln!(out, "{}}}", "    ".repeat(indent + 1)).unwrap();
            }
            writeln!(out, "{}}}", "    ".repeat(indent)).unwrap();
            skip_until = index + recovered.instruction_count;
            continue;
        }
        if structured
            && let Some((length, branches, distance)) = recover_choice(&instructions[index..])
        {
            let header = if distance {
                "match self.target_distance_group()"
            } else {
                "random"
            };
            writeln!(out, "{}{header} {{", "    ".repeat(indent)).unwrap();
            let fallback = branches.len() as u8;
            for (weight, body) in branches {
                let weight = if distance && weight == fallback {
                    "else".to_owned()
                } else {
                    weight.to_string()
                };
                let mut rendered = String::new();
                format_body(&mut rendered, &body, &nested_context)?;
                let mut lines = rendered.lines();
                let single_statement = lines
                    .next()
                    .filter(|line| line.trim_end().ends_with(';') && lines.next().is_none());
                if let Some(line) = single_statement {
                    writeln!(
                        out,
                        "{}{weight} => {}",
                        "    ".repeat(indent + 1),
                        line.trim_start()
                    )
                    .unwrap();
                } else {
                    writeln!(out, "{}{weight} => {{", "    ".repeat(indent + 1)).unwrap();
                    for line in rendered.lines() {
                        writeln!(out, "{}{line}", "    ".repeat(indent + 1)).unwrap();
                    }
                    writeln!(out, "{}}}", "    ".repeat(indent + 1)).unwrap();
                }
            }
            writeln!(out, "{}}}", "    ".repeat(indent)).unwrap();
            skip_until = index + length;
            continue;
        }
        let b = &instruction.bytes;
        if structured && let Some(marker) = ConditionMarker::decode(b) {
            match marker {
                ConditionMarker::Begin(condition) => {
                    writeln!(out, "{}if {} {{", "    ".repeat(indent), condition.name()).unwrap();
                    indent += 1;
                }
                ConditionMarker::Else => {
                    writeln!(out, "{}}} else {{", "    ".repeat(indent - 1)).unwrap();
                }
                ConditionMarker::End => {
                    indent -= 1;
                    writeln!(out, "{}}}", "    ".repeat(indent)).unwrap();
                }
                ConditionMarker::Unsupported => unreachable!("validated before rendering"),
            }
            continue;
        }
        // Explicit clears followed by their own return are lossless in any function.
        // Preserve implicit-return recovery only for existing handler tails.
        if *b == [0x0d, 0x04] {
            let followed_by_return = instructions
                .get(index + 1)
                .is_some_and(|next| is_own_return(&next.bytes, context));
            let handler_tail = context.allow_implicit_pass && index + 1 == instructions.len();
            if followed_by_return || handler_tail {
                writeln!(out, "{}pass;", "    ".repeat(indent)).unwrap();
                skip_until = if followed_by_return {
                    index + 2
                } else {
                    index + 1
                };
                continue;
            }
        }
        raw_blocks.push(b)?;
        let statement = match b.as_slice() {
            bytes
                if let Some(slot) = NativeSlot::from_call(bytes)
                    && context.subs.contains_key(&slot) =>
            {
                format!("{}();", sub_name(slot))
            }
            bytes if is_own_return(bytes, context) => "return;".into(),
            [0x11] => "self.bind_awareness_target();".into(),
            [0x13] => "self.bind_current_target();".into(),
            [0x1a, hi, lo] => format!("self.bind_target_area({});", u16::from_be_bytes([*hi, *lo])),
            [0x49, profile] => format!("self.bind_target_ground_point({profile});"),
            [0x40, value] if let Some(mode) = Mode::from_native(*value) => {
                format!("self.set_mode({});", mode.name())
            }
            [0x4d] => "self.resolve_target();".into(),
            [0x4e, 0] => "self.replenish_recovery_meter(0x50);".into(),
            [0x4f, 0] => "self.replenish_foraging_meter();".into(),
            [0x17, list, count, handler, end_policy] => {
                format!("self.init_area_change({list}, {count}, {handler}, {end_policy});")
            }
            [0x18] => "self.try_change_area();".into(),
            [0x2d] => "self.bind_scanned_object();".into(),
            [0x5b, 0] => "self.clear_undetected_player_tracking_timers();".into(),
            [0x2e, index] => format!("self.select_perception_profile({index});"),
            [0x7b] | [0x84] => "self.increment_random_value();".into(),
            bytes if let Some(strategy) = EntityTarget::decode(bytes) => {
                format!(
                    "self.select_target_entity(EntityTarget::{});",
                    strategy.name()
                )
            }
            [0x05, group, id, arg] => format!("self.action({group}:{id}, {arg});"),
            [0x06, 3, 0, hi, lo] => format!(
                "self.select_target_area({});",
                u16::from_be_bytes([*hi, *lo])
            ),
            [0x06, 10, 0, 0] => "self.select_target_area(AreaTarget::TargetPlayer);".into(),
            [0x06, 1, 0, slot] => format!("self.select_target_entity({slot});"),
            bytes if let Some(target) = PointTarget::decode(bytes) => {
                format!("self.select_target_point({});", target.arguments())
            }
            [0x07, index]
                if *index != 0
                    && context
                        .states
                        .is_some_and(|states| states.contains_key(index)) =>
            {
                format!("transition state_{index};")
            }
            [0x04] if context.states.is_some() => "restart;".into(),
            [0x68] => "stop();".into(),
            [0xff, 0x00] => "end;".into(),
            [0xff, 0xf7] => "end forget_target;".into(),
            [0x1e] => "clear_requests();".into(),
            [0x0d, 0x04] => "mark_unhandled();".into(),
            [0x92] => "nop();".into(),
            [0x48, ticks] => format!("wait({ticks});"),
            [0xff, 0xfb] => "area_end();".into(),
            [0xff, 0xfe] => "route_move_end();".into(),
            _ => format!(
                "native({});",
                b.iter()
                    .map(|b| format!("0x{b:02x}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        };
        writeln!(out, "{}{statement}", "    ".repeat(indent)).unwrap();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Node, dsl};

    #[derive(Default)]
    struct Image(BTreeMap<u32, u8>);
    impl Image {
        fn put(&mut self, address: u32, bytes: &[u8]) {
            self.0.extend(
                bytes
                    .iter()
                    .enumerate()
                    .map(|(i, b)| (address + i as u32, *b)),
            );
        }
        fn pointer(&mut self, address: u32, target: u32) {
            self.put(address, &target.to_le_bytes());
        }
    }
    fn main_image() -> Image {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image
    }

    impl Memory for Image {
        fn bytes(&self, address: u32, length: usize) -> Result<Vec<u8>> {
            (0..length)
                .map(|i| {
                    self.0
                        .get(&(address + i as u32))
                        .copied()
                        .ok_or_else(|| Error::new("unreadable test memory"))
                })
                .collect()
        }
    }

    #[test]
    fn table_nine_calls_and_nested_reentry_round_trip_as_fixed_functions() {
        let mut image = Image::default();
        image.put(0x100, &[0; 76]);
        let tables = BTreeMap::from([(0, 0x200), (1, 0xa00), (9, 0x600), (15, 0xc00)]);
        for (&index, &address) in &tables {
            image.pointer(0x100 + index as u32 * 4, address);
        }
        let scripts: &[(usize, usize, u32, &[u8])] = &[
            (0, 0, 0x300, &[0x16, 200, 0x81, 3, 0x82, 0, 7, 0xff, 0]),
            (9, 200, 0x400, &[0x16, 201, 0x48, 1, 0xff, 3]),
            (9, 201, 0x500, &[0x39, 0, 0xff, 3, 0x39, 2, 0x92, 0xff, 3]),
            (1, 3, 0xb00, &[0x16, 200, 0xff, 1]),
            (15, 7, 0xd00, &[0x16, 200, 0xff, 2]),
        ];
        for &(table, index, address, bytes) in scripts {
            image.pointer(tables[&table] + index as u32 * 4, address);
            image.put(address, bytes);
        }
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty(), "{:?}", result.warnings);
        assert!(result.source.contains("@slot(table = 9, index = 200)"));
        assert!(result.source.contains("sub_9_201();"));
        assert!(!result.source.contains("native(0x16"));
        let program = dsl::parse(&result.source)
            .unwrap()
            .compile()
            .unwrap()
            .program;
        assert!(program.automatic_slots.is_empty());
        let Node::Table(root) = &program.nodes[program.root] else {
            panic!()
        };
        for &(table, index, _, bytes) in scripts {
            let Node::Table(table) = &program.nodes[root.get(table).unwrap()] else {
                panic!()
            };
            assert_eq!(
                program.nodes[table.get(index).unwrap()],
                Node::Script(bytes.to_vec())
            );
        }
    }

    #[test]
    fn unreadable_table_nine_targets_stay_native() {
        let mut image = Image::default();
        image.put(0x100, &[0; 76]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.pointer(0x124, 0x400);
        image.put(0x300, &[0x16, 200, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(!result.warnings.is_empty());
        assert!(result.source.contains("native(0x16, 0xc8)"));
        assert!(!result.source.contains("@slot(table = 9"));
    }

    #[test]
    fn rathian_state_three_keeps_zero_case_operand_and_nested_species_cases() {
        // IDA ZZ HD bytes at 11854D88, through the outer FF 00 (not padding).
        // Previously 94/0 swallowed 94/1 and misread 11854D8E's operand as STOP.
        let bytes = [
            0x13, 0x94, 0, 2, 0x94, 1, 0, 0x70, 0, 7, 0x70, 1, 1, 0x35, 0, 0x81, 3, 0x35, 1, 0x81,
            4, 0x35, 2, 0x70, 1, 0x0b, 0x35, 0, 0x81, 5, 0x35, 1, 0x81, 6, 0x35, 2, 0x70, 1, 0x25,
            0x35, 0, 0x81, 0x16, 0x35, 1, 0x81, 0x17, 0x35, 2, 0x70, 1, 0x29, 0x35, 0, 0x81, 0x18,
            0x35, 1, 0x81, 0x19, 0x35, 2, 0x70, 1, 0x2a, 0x35, 0, 0x81, 0x1a, 0x35, 1, 0x81, 0x1b,
            0x35, 2, 0x70, 1, 0x31, 0x35, 0, 0x81, 0x1c, 0x35, 1, 0x81, 0x1d, 0x35, 2, 0x70, 1,
            0x64, 0x81, 1, 0x70, 2, 0x35, 0, 0x81, 3, 0x35, 1, 0x81, 4, 0x35, 2, 0x70, 3, 0x94, 1,
            1, 0x82, 3, 4, 0x94, 3, 0xff, 0,
        ];
        let mut image = Image::default();
        image.put(0x11854d88, &bytes);
        assert_eq!(script(&image, 0x11854d88).unwrap(), bytes);
        bytecode::validate_structure(&bytes).unwrap();
        image.put(0x100, &[0; 76]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0xff, 0]);
        image.pointer(0x20c, 0x11854d88);
        // Called bodies are intentionally absent; only they should be inherited.
        // decompile also recompiles and checks the entire recovered state bytes.
        let result = decompile(&image, 0x100, 1, 3, None).unwrap();
        assert!(result.source.contains("state_3 = 3 => {"));
        assert!(result.source.contains("match context.debug_mode"));
        assert!(result.source.contains("0 =>"));
        assert!(!result.source.contains("native(0x94"));
        assert!(
            !result
                .warnings
                .iter()
                .any(|warning| warning.starts_with("状态"))
        );
        assert!(
            result
                .warnings
                .iter()
                .any(|warning| warning.contains("表 18 项 4"))
        );
    }

    #[test]
    fn list_families_round_trip_separate_first_cases_and_nested_blocks() {
        for opcode in [
            0x15, 0x1c, 0x1d, 0x20, 0x23, 0x27, 0x2c, 0x33, 0x3e, 0x57, 0x70, 0x73, 0x75, 0x76,
            0x79, 0x7a, 0x7d, 0x94,
        ] {
            let mut bytes = vec![opcode, 0, 1];
            if opcode == 0x79 {
                bytes.push(3); // Callback query ID.
            }
            bytes.extend_from_slice(&[opcode, 1, 0]);
            if matches!(opcode, 0x15 | 0x3e | 0x57) {
                bytes.push(0);
            }
            bytes.extend_from_slice(&[
                0x35, 0, 0xff, 0, 0x35, 2, opcode, 2, 0x92, opcode, 3, 0xff, 0,
            ]);
            let mut image = main_image();
            image.put(0x300, &bytes);
            assert_eq!(script(&image, 0x300).unwrap(), bytes);
            let result = decompile(&image, 0x100, 1, 0, None).unwrap();
            assert!(result.warnings.is_empty());
            let rendered = match opcode {
                0x15 => "match self.area".to_owned(),
                0x1d => "match self.request".to_owned(),
                0x20 => "match self.target_angle()".to_owned(),
                0x2c => "match self.species_group".to_owned(),
                0x70 => "match self.species".to_owned(),
                0x94 => "match context.debug_mode".to_owned(),
                0x57 => "match self.area_route_profile".to_owned(),
                0x79 => "match context.query(3)".to_owned(),
                other => format!("native(0x{other:02x}, 0x00, 0x01"),
            };
            assert!(
                result.source.contains(&rendered),
                "{rendered}: {}",
                result.source
            );
        }
    }

    #[test]
    fn request_dispatch_recovers_only_with_a_complete_case_count() {
        for (count, recovered) in [(2u8, true), (7, false)] {
            let mut image = main_image();
            image.put(
                0x300,
                &[
                    0x1d, 0, count, 0x1d, 1, 1, 0x92, 0x1d, 1, 2, 0x92, 0x1d, 3, 0xff, 0,
                ],
            );
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert_eq!(
                result.source.contains("match self.request {"),
                recovered,
                "count {count}: {}",
                result.source
            );
            if recovered {
                assert!(result.source.contains("nop();"), "{}", result.source);
                assert!(!result.source.contains("native(0x1d"));
            } else {
                assert!(result.source.contains("native(0x1d, 0x00, 0x07);"));
            }
        }
    }

    #[test]
    fn target_strategies_decompile_and_recompile_losslessly() {
        let mut image = main_image();
        image.put(0x300, &[0x52, 0x53, 0x5f, 0x7e, 0x12, 0x58, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for name in [
            "SameArea",
            "SameAreaGroundGroup",
            "SameOrAllowedArea",
            "TrackedPlayer",
            "LeaderTarget",
            "PlayerOrMonster",
        ] {
            assert!(
                result
                    .source
                    .contains(&format!("self.select_target_entity(EntityTarget::{name});"))
            );
        }
        assert!(!result.source.contains("native("));
    }

    #[test]
    fn rng_normalization_only_changes_opcodes_in_all_entry_kinds() {
        assert!(
            matches_export(&[0x84, 0x48, 0x7b, 0xff, 0], &[0x7b, 0x48, 0x7b, 0xff, 0]).unwrap()
        );
        assert!(
            !matches_export(&[0x84, 0x48, 0x84, 0xff, 0], &[0x7b, 0x48, 0x7b, 0xff, 0]).unwrap()
        );
        let mut image = main_image();
        image.put(0x300, &[0x7b, 0x81, 0, 0xff, 0]);
        image.pointer(0x104, 0x400);
        image.pointer(0x400, 0x500);
        image.put(0x500, &[0x7b, 0xff, 1]);
        image.pointer(0x100 + EVENT_SLOTS[3].root_index as u32 * 4, 0x600);
        image.pointer(0x600, 0x700);
        image.put(0x700, &[0x7b, 0xff, EVENT_SLOTS[3].ending]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(
            result
                .source
                .matches("self.increment_random_value();")
                .count(),
            3
        );
    }

    #[test]
    fn area_change_initialization_round_trips_each_u8_operand() {
        for operand in 0..4 {
            let mut bytes = Vec::new();
            for value in 0..=u8::MAX {
                let mut args = [3, 5, 7, 9];
                args[operand] = value;
                bytes.push(0x17);
                bytes.extend_from_slice(&args);
            }
            let source = round_trip_body(&bytes);
            for value in 0..=u8::MAX {
                let mut args = [3, 5, 7, 9];
                args[operand] = value;
                assert!(
                    source.contains(&format!(
                        "self.init_area_change({}, {}, {}, {});",
                        args[0], args[1], args[2], args[3]
                    )),
                    "operand {operand}, value {value}"
                );
            }
        }

        let source = round_trip_body(&[
            0x14, 0, 45, 0x17, 3, 128, 5, 255, 0x14, 1, 0x17, 7, 127, 9, 2, 0x14, 2, 0x18,
        ]);
        assert!(source.contains("if self.target_angle_at_least(45) {"));
        assert!(source.contains("self.init_area_change(3, 128, 5, 255);"));
        assert!(source.contains("self.init_area_change(7, 127, 9, 2);"));
        assert!(source.contains("self.try_change_area();"));

        for length in 1..5 {
            let truncated = &[0x17, 0, 2, 1, 0][..length];
            assert!(
                format_test_body(&mut String::new(), truncated, None).is_err(),
                "{truncated:02x?}"
            );
        }
    }

    #[test]
    fn meter_replenishment_round_trips_without_normalizing_native_operands() {
        for (opcode, statement) in [
            (0x4e, "self.replenish_recovery_meter(0x50);"),
            (0x4f, "self.replenish_foraging_meter();"),
        ] {
            let bytes: Vec<u8> = (0..=u8::MAX)
                .flat_map(|operand| [opcode, operand])
                .chain([0x50, 0])
                .collect();
            let source = round_trip_body(&bytes);
            assert_eq!(source.matches(statement).count(), 1);
            assert!(!source.contains(&format!("native(0x{opcode:02x}, 0x00);")));
            for operand in 1..=u8::MAX {
                assert!(
                    source.contains(&format!("native(0x{opcode:02x}, 0x{operand:02x});")),
                    "opcode {opcode:02x}, operand {operand}"
                );
            }
            assert!(source.contains("native(0x50, 0x00);"));
            assert!(format_test_body(&mut String::new(), &[opcode], None).is_err());
        }

        let source = round_trip_body(&[
            0x35, 0, 0x4e, 0, 0x4f, 0, 0x35, 1, 0x4e, 255, 0x4f, 255, 0x35, 2,
        ]);
        assert!(source.contains("if self.enraged {"));
        assert!(source.contains("self.replenish_recovery_meter(0x50);"));
        assert!(source.contains("self.replenish_foraging_meter();"));
        assert!(source.contains("native(0x4e, 0xff);"));
        assert!(source.contains("native(0x4f, 0xff);"));
    }

    #[test]
    fn tracking_timer_clear_round_trips_without_normalizing_nonzero_selectors() {
        let bytes: Vec<u8> = (0..=u8::MAX)
            .flat_map(|selector| [0x5b, selector])
            .collect();
        let source = round_trip_body(&bytes);
        assert_eq!(
            source
                .matches("self.clear_undetected_player_tracking_timers();")
                .count(),
            1
        );
        assert!(!source.contains("native(0x5b, 0x00);"));
        for selector in 1..=u8::MAX {
            assert!(
                source.contains(&format!("native(0x5b, 0x{selector:02x});")),
                "selector {selector}"
            );
        }

        let source = round_trip_body(&[
            0x03, 0, 0x5b, 0, 0x03, 1, 0x5b, 255, 0x03, 2, 0x02, 0, 0x92, 0x02, 2,
        ]);
        assert!(source.contains("if self.check_pending_area() {"));
        assert!(source.contains("self.clear_undetected_player_tracking_timers();"));
        assert!(source.contains("native(0x5b, 0xff);"));
        assert!(source.contains("if self.check_tracked_players() {"));
        assert!(format_test_body(&mut String::new(), &[0x5b], None).is_err());
    }

    #[test]
    fn mode_target_and_random_methods_round_trip() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x11, 0x13, 0x40, 0, 0x40, 1, 0x4d, 0x18, 0x2e, 1, 0x2d, 0x84, 0x7b, 0x40, 2, 0xff,
                0,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for method in [
            "self.bind_awareness_target();",
            "self.bind_current_target();",
            "self.set_mode(Mode::Normal);",
            "self.set_mode(Mode::Attack);",
            "self.resolve_target();",
            "self.try_change_area();",
            "self.select_perception_profile(1);",
            "self.bind_scanned_object();",
            "self.increment_random_value();",
            "native(0x40, 0x02);",
        ] {
            assert!(result.source.contains(method), "{}", result.source);
        }
        assert_eq!(
            result
                .source
                .matches("self.increment_random_value();")
                .count(),
            2
        );
        for opcode in [0x11, 0x13, 0x7b] {
            assert!(!result.source.contains(&format!("native(0x{opcode:02x}")));
        }
    }

    #[test]
    fn weighted_choices_round_trip_nested_blocks_and_preserve_unusual_weights() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x35, 0, 0x80, 0, 2, 0x80, 1, 16, 0x80, 0, 1, 0x80, 1, 32, 0x92, 0x80, 0xff, 0x80,
                2, 16, 0x39, 0, 0xff, 0xf7, 0x39, 2, 0x80, 0xff, 0x35, 2, 0xff, 0,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(
            result.source.matches("random {").count(),
            2,
            "{}",
            result.source
        );
        assert!(result.source.contains("32 => nop();"));
        assert!(result.source.contains("end forget_target;"));
        image.put(0x300, &[0x80, 0, 1, 0x80, 1, 31, 0x92, 0x80, 0xff, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(!result.source.contains("random {"));
        assert!(result.source.contains("native(0x80"));
    }

    #[test]
    fn distance_matches_round_trip_with_nested_random_and_distance_blocks() {
        let mut image = main_image();
        for count in 1..=4 {
            let mut bytes = vec![0x83, 0, count];
            for index in 1..=count + 1 {
                bytes.extend_from_slice(&[0x83, index, 0x80, 0, 1, 0x80, 1, 32, 0x92, 0x80, 0xff]);
            }
            bytes.extend_from_slice(&[0x83, 0xff, 0xff, 0]);
            image.put(0x300, &bytes);
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert!(
                result
                    .source
                    .contains("match self.target_distance_group() {")
            );
            assert!(result.source.contains("else => {"));
            assert!(!result.source.contains("native(0x83"));
        }
        image.put(
            0x300,
            &[
                0x83, 0, 1, 0x83, 1, 0x83, 0, 1, 0x83, 1, 0x92, 0x83, 2, 0x4d, 0x83, 0xff, 0x83, 2,
                0x92, 0x83, 0xff, 0xff, 0,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(
            result
                .source
                .matches("match self.target_distance_group()")
                .count(),
            2
        );
        image.put(
            0x300,
            &[
                0x83, 0, 1, 0x83, 1, 0x92, 0x83, 3, 0x4d, 0x83, 0xff, 0xff, 0,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("native(0x83"));
        assert!(!result.source.contains("match self.target_distance_group()"));
    }

    #[test]
    fn area_binding_round_trips_the_full_u16_range() {
        // Keep each native script below its size limit while covering every ID.
        for hi in 0..=u8::MAX {
            let bytes: Vec<_> = (0..=u8::MAX).flat_map(|lo| [0x1a, hi, lo]).collect();
            let source = round_trip_body(&bytes);
            assert_eq!(source.matches("self.bind_target_area(").count(), 256);
            for lo in [0, 1, 0x2c, 0xff] {
                let area = u16::from_be_bytes([hi, lo]);
                assert!(source.contains(&format!("self.bind_target_area({area});")));
            }
            assert!(!source.contains("native(0x1a"));
        }

        let source = round_trip_body(&[
            0x09, 0, 0x1a, 1, 0x2c, 0x09, 1, 0x06, 3, 0, 1, 0x2c, 0x09, 2, 0x19,
        ]);
        assert!(source.contains("if self.airborne {"));
        assert!(source.contains("self.bind_target_area(300);"));
        assert!(source.contains("self.select_target_area(300);"));
        assert!(source.contains("native(0x19);"));

        for bytes in [&[0x1a][..], &[0x1a, 1]] {
            assert!(format_test_body(&mut String::new(), bytes, None).is_err());
        }
    }

    #[test]
    fn area_and_monster_targets_round_trip() {
        let mut image = main_image();
        let mut bytes = vec![6, 3, 0, 1, 70, 6, 3, 0, 255, 255, 6, 10, 0, 0];
        for subtype in 0..4 {
            bytes.extend_from_slice(&[6, 13, subtype, 0]);
        }
        // Opaque operands and unsupported subtypes must retain their exact bytes.
        bytes.extend_from_slice(&[
            6, 3, 1, 0, 1, 6, 10, 1, 0, 6, 10, 0, 1, 6, 13, 2, 1, 6, 13, 4, 0, 0xff, 0,
        ]);
        image.put(0x300, &bytes);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        for text in [
            "self.select_target_area(326);",
            "self.select_target_area(65535);",
            "self.select_target_area(AreaTarget::TargetPlayer);",
            "EntityTarget::CurrentOrLargeMonster",
            "EntityTarget::LargeMonster",
            "EntityTarget::OtherMonster",
            "EntityTarget::OtherLargeMonster",
            "native(0x06, 0x03, 0x01, 0x00, 0x01);",
            "native(0x06, 0x0a, 0x01, 0x00);",
            "native(0x06, 0x0a, 0x00, 0x01);",
            "native(0x06, 0x0d, 0x02, 0x01);",
            "native(0x06, 0x0d, 0x04, 0x00);",
        ] {
            assert!(result.source.contains(text), "{text}: {}", result.source);
        }
        assert!(
            dsl::parse(
                "mhf_ai 1; species 6; base native; fn main() { self.select_target_area(0); }"
            )
            .is_ok()
        );
        for expression in [
            "self.select_target_area(65536);",
            "self.select_target_area(-1);",
            "self.select_target_area(AreaTarget::Unknown);",
            "self.select_target_area();",
            "self.select_target_entity(EntityTarget::EligibleOtherMonster);",
        ] {
            assert!(
                dsl::parse(&format!(
                    "mhf_ai 1; species 6; base native; fn main() {{ {expression} }}"
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn waypoint_selection_round_trips_without_reinterpreting_other_target_types() {
        let mut image = main_image();
        image.put(
            0x300,
            &[6, 2, 1, 0, 0x4d, 6, 2, 1, 255, 6, 2, 2, 0, 0xff, 0],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("self.select_target_point(0);"));
        assert!(result.source.contains("self.select_target_point(255);"));
        assert!(result.source.contains("native(0x06, 0x02, 0x02, 0x00);"));
    }

    #[test]
    fn preparation_points_round_trip_all_indices_and_preserve_other_groups() {
        let kinds = [(3, "Landing"), (4, "Departure")];
        let mut bytes = Vec::new();
        for (group, _) in kinds {
            for index in 0..=u8::MAX {
                bytes.extend_from_slice(&[0x06, 2, group, index]);
            }
        }
        for group in [0, 2, 5, 6, 7, 255] {
            bytes.extend_from_slice(&[0x06, 2, group, 255]);
        }
        let source = round_trip_body(&bytes);
        for (group, name) in kinds {
            assert_eq!(
                source.matches(&format!("PointTarget::{name},")).count(),
                256
            );
            for index in 0..=u8::MAX {
                assert!(source.contains(&format!(
                    "self.select_target_point(PointTarget::{name}, {index});"
                )));
            }
            assert!(format_test_body(&mut String::new(), &[0x06, 2, group], None).is_err());
        }
        for group in [0, 2, 5, 6, 7, 255] {
            assert!(source.contains(&format!("native(0x06, 0x02, 0x{group:02x}, 0xff);")));
        }

        // The native airborne sequence still selects, flies and lands separately.
        let source = round_trip_body(&[
            0x09, 0, 0x06, 2, 3, 0, 0x05, 2, 7, 0, 0x05, 2, 1, 0, 0x09, 2,
        ]);
        assert!(source.contains("if self.airborne {"));
        assert!(source.contains("self.select_target_point(PointTarget::Landing, 0);"));
        assert!(source.contains("self.action(2:7, 0);"));
        assert!(source.contains("self.action(2:1, 0);"));

        // Departure selection, approach, destination and flight remain separate.
        let source = round_trip_body(&[
            0x06, 2, 4, 0, 0x05, 1, 3, 0, 0x05, 1, 0, 0, 0x1a, 1, 0x2c, 0x05, 2, 0, 0, 0x05, 2, 8,
            0,
        ]);
        assert!(source.contains("self.select_target_point(PointTarget::Departure, 0);"));
        assert!(source.contains("self.action(1:3, 0);"));
        assert!(source.contains("self.action(1:0, 0);"));
        assert!(source.contains("self.bind_target_area(300);"));
        assert!(source.contains("self.action(2:0, 0);"));
        assert!(source.contains("self.action(2:8, 0);"));
    }

    #[test]
    fn player_slots_round_trip_without_reinterpreting_other_target_groups() {
        let mut image = main_image();
        let mut body = Vec::new();
        for slot in 0..=255 {
            body.extend_from_slice(&[6, 1, 0, slot]);
        }
        body.extend_from_slice(&[6, 1, 1, 3, 0x4d, 0xff, 0]);
        image.put(0x300, &body);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        for slot in 0..=255 {
            assert!(
                result
                    .source
                    .contains(&format!("self.select_target_entity({slot});"))
            );
        }
        assert_eq!(result.source.matches("select_target_entity(").count(), 256);
        assert!(result.source.contains("native(0x06, 0x01, 0x01, 0x03);"));
        assert!(result.source.contains("self.resolve_target();"));
    }

    #[test]
    fn relative_target_points_round_trip_and_preserve_other_encodings() {
        let mut image = main_image();
        let mut body = Vec::new();
        // Exercise all direction selectors, including the second distance band,
        // and nonzero trailing operands that must remain byte-preserving escapes.
        for selector in 0..=255 {
            for trailing in [0, 1, 255] {
                body.extend_from_slice(&[6, 6, selector, trailing]);
            }
        }
        body.extend_from_slice(&[0x4d, 0xff, 0]);
        image.put(0x300, &body);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        for name in [
            "Forward500",
            "Left500",
            "Right500",
            "Backward500",
            "Forward1000",
            "Left1000",
            "Right1000",
            "Backward1000",
        ] {
            assert!(
                result
                    .source
                    .contains(&format!("self.select_target_point(Direction::{name});"))
            );
        }
        assert_eq!(result.source.matches("select_target_point(").count(), 8);
        assert!(result.source.contains("native(0x06, 0x06, 0x04, 0x00);"));
        assert!(result.source.contains("native(0x06, 0x06, 0x00, 0x01);"));
        assert!(result.source.contains("self.resolve_target();"));
    }

    #[test]
    fn active_conditions_decompile_and_recompile_losslessly() {
        let mut image = main_image();
        image.put(0x300, &[0x08, 0, 0x92, 0x08, 1, 0x4d, 0x08, 2, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("if self.active {"));
        assert!(!result.source.contains("native(0x08"));
    }

    #[test]
    fn angle_conditions_round_trip_every_native_boundary_and_reversed_bounds() {
        let mut image = main_image();
        for boundary in 0..=255 {
            image.put(
                0x300,
                &[
                    0x78, 0, 0, boundary, 0x78, 0, boundary, 255, 0x92, 0x78, 1, 0x4d, 0x78, 2,
                    0x78, 2, 0xff, 0,
                ],
            );
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert_eq!(result.source.matches("if self.target_angle_in(").count(), 2);
            assert!(!result.source.contains("native(0x78"));
        }
        image.put(0x300, &[0x78, 0, 200, 20, 0x92, 0x78, 2, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("native(0x78, 0x00, 0xc8, 0x14)"));
    }

    #[test]
    fn byte_condition_operands_round_trip_the_full_range_with_optional_else() {
        for (opcode, method) in [
            (0x14, "target_angle_at_least"),
            (0x5a, "target_ground_is"),
            (0x22, "near_target_2d"),
            (0x36, "near_target_3d"),
        ] {
            for value in 0..=u8::MAX {
                for with_else in [false, true] {
                    let mut bytes = vec![opcode, 0, value, 0x92];
                    if with_else {
                        bytes.extend_from_slice(&[opcode, 1, 0x48, 1]);
                    }
                    bytes.extend_from_slice(&[opcode, 2]);
                    let source = round_trip_body(&bytes);
                    assert!(
                        source.contains(&format!("if self.{method}({value}) {{")),
                        "{source}"
                    );
                    assert_eq!(source.contains("} else {"), with_else, "{source}");
                    assert!(
                        !source.contains(&format!("native(0x{opcode:02x}")),
                        "{source}"
                    );
                }
            }
        }
    }

    #[test]
    fn target_angle_thresholds_preserve_nested_and_irregular_layouts() {
        let source = round_trip_body(&[
            0x14, 0, 45, 0x14, 0, 255, 0x92, 0x14, 1, 0x78, 0, 32, 96, 0x92, 0x78, 2, 0x14, 2,
            0x14, 1, 0x5a, 0, 1, 0x92, 0x5a, 2, 0x14, 2,
        ]);
        assert!(source.contains("if self.target_angle_at_least(45) {"));
        assert!(source.contains("if self.target_angle_at_least(255) {"));
        assert!(source.contains("if self.target_angle_in(45, 135) {"));
        assert!(source.contains("if self.target_ground_is(1) {"));
        assert!(!source.contains("native("));

        let source = round_trip_body(&[0x14, 0, 255, 0x92, 0x14, 1, 0x14, 1, 0x14, 2]);
        assert!(!source.contains("self.target_angle_at_least("));
        assert!(source.contains("native(0x14, 0x00, 0xff);"));
        assert_eq!(source.matches("native(0x14, 0x01);").count(), 2);

        // A partial body stays raw; the enclosing compiler rejects its missing end.
        let mut source = String::new();
        format_test_body(&mut source, &[0x14, 0, 45, 0x92], None).unwrap();
        assert!(source.contains("native(0x14, 0x00, 0x2d);"));
        assert!(
            super::super::dsl::parse(&format!("mhf_ai 1; species 6; fn main() {{ {source} }}"))
                .unwrap()
                .compile()
                .is_err()
        );

        for bytes in [
            &[0x14, 0][..],
            &[0x14, 1, 0x92, 0x14, 2],
            &[0x14, 0, 45, 0x5a, 2],
            &[0x14, 3],
        ] {
            assert!(
                format_test_body(&mut String::new(), bytes, None).is_err(),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn zero_weight_random_branches_round_trip_at_every_position() {
        for weights in [[0, 16, 16], [16, 0, 16], [16, 16, 0]] {
            let mut image = main_image();
            let mut bytes = vec![0x80, 0, 3];
            for (index, weight) in weights.into_iter().enumerate() {
                bytes.extend_from_slice(&[0x80, index as u8 + 1, weight, 0x92]);
            }
            bytes.extend_from_slice(&[0x80, 0xff, 0xff, 0]);
            image.put(0x300, &bytes);
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert!(result.source.contains("0 => nop();"));
            assert!(!result.source.contains("native(0x80"));
        }
    }

    #[test]
    fn target_forgetting_reset_round_trips_as_control_flow() {
        let mut image = main_image();
        image.put(0x300, &[0xff, 0xf7]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("end forget_target;"));
        assert!(!result.source.contains("native(0xff, 0xf7)"));
    }

    #[test]
    fn target_ground_point_profiles_round_trip_the_full_byte_range() {
        let mut bytes = Vec::new();
        for profile in 0..=u8::MAX {
            bytes.extend_from_slice(&[0x49, profile]);
        }
        // A raw kind-11 target uses a different encoding and does not have
        // 0x49's immediate saved-context synchronization. Keep it native.
        bytes.extend_from_slice(&[0x06, 11, 0, 2, 0x4d]);
        let source = round_trip_body(&bytes);
        for profile in 0..=u8::MAX {
            assert!(
                source.contains(&format!("self.bind_target_ground_point({profile});")),
                "missing profile {profile}: {source}"
            );
        }
        assert!(source.contains("native(0x06, 0x0b, 0x00, 0x02);"));
        assert_eq!(source.matches("self.resolve_target();").count(), 1);
    }

    #[test]
    fn mode_is_round_trips_operands_nested_blocks_and_events() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x0b, 0, 0, 0x0b, 0, 1, 0x92, 0x0b, 2, 0x0b, 1, 4, 0x0b, 2, 0xff, 0,
            ],
        );
        image.pointer(0x100 + EVENT_SLOTS[3].root_index as u32 * 4, 0x400);
        image.pointer(0x400, 0x500);
        image.put(
            0x500,
            &[
                0x0b,
                0,
                1,
                0x39,
                0,
                0x92,
                0x39,
                2,
                0x0b,
                2,
                0xff,
                EVENT_SLOTS[3].ending,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for value in ["Mode::Normal", "Mode::Attack"] {
            assert!(
                result
                    .source
                    .contains(&format!("if self.mode_is({value}) {{"))
            );
        }
        assert!(!result.source.contains("native(0x0b"));
        // Unknown byte values remain lossless escapes, including nested markers.
        image.put(
            0x300,
            &[
                0x0b, 0, 255, 0x0b, 0, 0, 0x92, 0x0b, 2, 0x0b, 1, 4, 0x0b, 2, 0xff, 0,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("native(0x0b, 0x00, 0xff);"));
        assert!(result.source.contains("self.mode_is(Mode::Attack)"));
    }

    #[test]
    fn pending_area_checks_round_trip_optional_else_and_nested_blocks() {
        for bytes in [
            vec![0x03, 0, 0x03, 2],
            vec![0x03, 0, 0x03, 1, 0x03, 2],
            vec![0x03, 0, 0x92, 0x03, 1, 0x48, 1, 0x03, 2],
            vec![0x03, 0, 0x03, 0, 0x92, 0x03, 2, 0x03, 1, 0x92, 0x03, 2],
            vec![
                0x17, 1, 3, 5, 7, 0x03, 0, 0x14, 0, 45, 0x92, 0x14, 2, 0x03, 1, 0x02, 0, 0x92,
                0x02, 2, 0x03, 2,
            ],
        ] {
            let source = round_trip_body(&bytes);
            assert!(
                source.contains("if self.check_pending_area() {"),
                "{source}"
            );
            assert!(!source.contains("native(0x03"), "{source}");
        }

        // Repeated else markers remain raw instead of acquiring structured semantics.
        let source = round_trip_body(&[0x03, 0, 0x92, 0x03, 1, 0x03, 1, 0x03, 2]);
        assert!(!source.contains("self.check_pending_area()"));
        assert!(source.contains("native(0x03, 0x00);"));
        assert_eq!(source.matches("native(0x03, 0x01);").count(), 2);

        for bytes in [
            &[0x03][..],
            &[0x03, 1, 0x92, 0x03, 2],
            &[0x03, 0, 0x14, 2],
            &[0x03, 3],
        ] {
            assert!(
                format_test_body(&mut String::new(), bytes, None).is_err(),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn target_query_conditions_round_trip_nested_state_and_event_bodies() {
        for (opcode, condition, nested, nested_condition) in [
            (
                0x02,
                "self.check_tracked_players()",
                0x54,
                "self.target_available()",
            ),
            (0x54, "self.target_available()", 0x39, "self.flashed"),
        ] {
            let mut image = main_image();
            image.put(
                0x300,
                &[
                    opcode, 0, nested, 0, 0x92, nested, 2, opcode, 1, 4, opcode, 2, 0xff, 0,
                ],
            );
            let event = EVENT_SLOTS[3];
            image.pointer(0x100 + event.root_index as u32 * 4, 0x400);
            image.pointer(0x400, 0x500);
            image.put(0x500, &[opcode, 0, 0x92, opcode, 2, 0xff, event.ending]);
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert_eq!(
                result.source.matches(&format!("if {condition} {{")).count(),
                2
            );
            assert!(result.source.contains(&format!("if {nested_condition} {{")));
            assert!(!result.source.contains(&format!("native(0x{opcode:02x}")));
        }
    }

    /// Render `bytes` as a script body, recompile the rendered source, and check
    /// that the round trip reproduces those bytes plus the entry return.
    fn round_trip_body(bytes: &[u8]) -> String {
        let mut source = String::new();
        format_test_body(&mut source, bytes, None).unwrap();
        let compiled = dsl::parse(&format!("mhf_ai 1; species 6; fn main() {{ {source} }}"))
            .unwrap()
            .compile()
            .unwrap();
        let mut expected = bytes.to_vec();
        expected.extend_from_slice(&[0xff, 0]);
        assert!(
            compiled
                .program
                .nodes
                .iter()
                .any(|node| matches!(node, Node::Script(actual) if *actual == expected)),
            "{source}"
        );
        source
    }

    #[test]
    fn condition_markers_round_trip_nested_else_and_irregular_layouts() {
        for (opcode, condition) in [
            (0x2f, "context.any_player_carrying()"),
            (0x28, "self.has_player_in_same_area()"),
            (0x4a, "self.target_detected"),
            (0x77, "context.is_daytime"),
            (0x09, "self.airborne"),
            (0x2a, "self.attack_timer_active"),
            (0x29, "self.area_timer_expired"),
        ] {
            for bytes in [
                vec![opcode, 0, 0x92, opcode, 2],
                vec![opcode, 0, 0x07, 4, opcode, 1, 0x07, 9, opcode, 2],
                vec![opcode, 0, opcode, 0, 0x92, opcode, 2, opcode, 2],
                vec![
                    opcode, 0, opcode, 0, 0x92, opcode, 1, 0x48, 1, opcode, 2, opcode, 1, 0x92,
                    opcode, 2,
                ],
            ] {
                let source = round_trip_body(&bytes);
                assert!(source.contains(&format!("if {condition} {{")), "{source}");
                assert!(
                    !source.contains(&format!("native(0x{opcode:02x}")),
                    "{source}"
                );
            }
            for nested in [0x2f, 0x39] {
                let source = round_trip_body(&[opcode, 0, nested, 0, 0x92, nested, 2, opcode, 2]);
                assert!(source.contains(&format!("if {condition} {{")), "{source}");
                assert!(!source.contains("native("), "{source}");
            }
            let source = round_trip_body(&[opcode, 0, opcode, 1, opcode, 1, opcode, 2]);
            assert!(!source.contains(condition), "{source}");
            assert_eq!(source.matches("native(").count(), 4, "{source}");
        }
        // Unknown selectors have no verified instruction boundary.
        assert!(format_test_body(&mut String::new(), &[0x77, 3], None).is_err());
    }

    #[test]
    fn area_matches_round_trip() {
        for (bytes, structured) in [
            (
                vec![0x15, 0, 2, 0x15, 1, 1, 70, 0x92, 0x15, 1, 0, 1, 0x15, 3],
                true,
            ),
            (
                vec![
                    0x15, 0, 2, 0x15, 1, 255, 255, 0x15, 1, 255, 255, 0x15, 2, 0x92, 0x15, 3,
                ],
                true,
            ),
            (
                vec![
                    0x15, 0, 1, 0x15, 1, 0, 1, 0x15, 0, 1, 0x15, 1, 1, 70, 0xff, 0, 0x15, 3, 0x15,
                    3,
                ],
                true,
            ),
            (vec![0x15, 0, 2, 0x15, 1, 0, 1, 0x15, 3], false),
            (
                vec![0x15, 0, 1, 0x15, 1, 0, 1, 0x15, 2, 0x15, 2, 0x15, 3],
                false,
            ),
        ] {
            let source = round_trip_body(&bytes);
            assert_eq!(source.contains("match self.area {"), structured, "{source}");
        }
        for body in [
            "match self.area {}",
            "match self.area { 65536 => {} }",
            "match context.area { 1 => {} }",
            "match self.area { else => {} }",
        ] {
            assert!(dsl::parse(&format!("mhf_ai 1; species 6; fn main() {{ {body} }}")).is_err());
        }
    }

    #[test]
    fn target_angle_matches_round_trip_every_native_threshold_as_degrees() {
        for threshold in 0..=u8::MAX {
            for with_else in [false, true] {
                let mut bytes = vec![0x20, 0, 1, 0x20, 1, threshold, 0x92];
                if with_else {
                    bytes.extend_from_slice(&[0x20, 2, 0x48, 1]);
                }
                bytes.extend_from_slice(&[0x20, 3]);
                let source = round_trip_body(&bytes);
                assert!(source.contains("match self.target_angle() {"), "{source}");
                assert!(
                    source.contains(&format!(
                        "{} => {{",
                        Degrees::from_native(threshold).value()
                    )),
                    "{source}"
                );
                assert_eq!(source.contains("else => {"), with_else, "{source}");
                assert!(!source.contains("native(0x20"), "{source}");
            }
        }
    }

    #[test]
    fn target_angle_matches_preserve_duplicate_and_unsorted_thresholds() {
        let mut bytes = vec![0x20, 0, 255];
        for index in 0..255 {
            bytes.extend_from_slice(&[0x20, 1, 2 - index % 3, 0x92]);
        }
        bytes.extend_from_slice(&[0x20, 3]);
        let source = round_trip_body(&bytes);
        assert_eq!(source.matches(" => {").count(), 255);
        assert_eq!(source.matches("2.8125 => {").count(), 85);
        assert!(!source.contains("native(0x20"), "{source}");
    }

    #[test]
    fn target_angle_matches_nest_with_other_angle_encodings() {
        let source = round_trip_body(&[
            0x20, 0, 2, 0x20, 1, 32, 0x14, 0, 45, 0x20, 0, 1, 0x20, 1, 1, 0x78, 0, 32, 96, 0x92,
            0x78, 2, 0x20, 2, 0x48, 1, 0x20, 3, 0x14, 2, 0x20, 1, 0, 0x92, 0x20, 2, 0x92, 0x20, 3,
        ]);
        assert_eq!(source.matches("match self.target_angle() {").count(), 2);
        assert!(source.contains("45 => {"), "{source}");
        assert!(source.contains("1.40625 => {"), "{source}");
        assert!(source.contains("if self.target_angle_in(45, 135) {"));
        assert!(source.contains("if self.target_angle_at_least(45) {"));
        assert!(!source.contains("native("), "{source}");
    }

    #[test]
    fn target_angle_matches_keep_noncanonical_layouts_native() {
        for bytes in [
            vec![0x20, 0, 0, 0x20, 1, 32, 0x92, 0x20, 3],
            vec![0x20, 0, 2, 0x20, 1, 32, 0x92, 0x20, 3],
            vec![0x20, 0, 1, 0x20, 1, 32, 0x20, 2, 0x20, 2, 0x20, 3],
            vec![0x20, 0, 2, 0x20, 1, 32, 0x20, 2, 0x20, 1, 64, 0x20, 3],
        ] {
            let source = round_trip_body(&bytes);
            assert!(!source.contains("match self.target_angle()"), "{source}");
            assert!(source.contains("native(0x20"), "{source}");
        }

        // A fragment stays raw while the complete-script compiler rejects its
        // missing end marker; the renderer does not invent a closing match.
        let mut source = String::new();
        format_test_body(&mut source, &[0x20, 0, 1, 0x20, 1, 32, 0x92], None).unwrap();
        assert!(!source.contains("match self.target_angle()"), "{source}");
        assert!(
            dsl::parse(&format!("mhf_ai 1; species 6; fn main() {{ {source} }}"))
                .unwrap()
                .compile()
                .is_err()
        );

        for bytes in [
            &[0x20, 0][..],
            &[0x20, 0, 1, 0x20, 1],
            &[0x20, 0, 1, 0x92, 0x20, 3],
            &[0x20, 0, 1, 0x20, 1, 32, 0x20, 4],
            &[0x20, 0, 1, 0x20, 1, 32, 0x14, 2],
        ] {
            assert!(
                format_test_body(&mut String::new(), bytes, None).is_err(),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn target_ground_conditions_preserve_nested_and_irregular_layouts() {
        let source = round_trip_body(&[
            0x49, 2, 0x5a, 0, 1, 0x5a, 0, 255, 0x13, 0x5a, 1, 0x92, 0x5a, 2, 0x5a, 1, 0x39, 0,
            0x92, 0x39, 2, 0x5a, 2,
        ]);
        assert_eq!(source.matches("if self.target_ground_is(").count(), 2);
        assert!(source.contains("self.bind_target_ground_point(2);"));
        assert!(source.contains("if self.flashed {"));
        assert!(!source.contains("native("));

        let source = round_trip_body(&[0x5a, 0, 255, 0x92, 0x5a, 1, 0x5a, 1, 0x5a, 2]);
        assert!(!source.contains("self.target_ground_is("));
        assert!(source.contains("native(0x5a, 0x00, 0xff);"));
        assert_eq!(source.matches("native(0x5a, 0x01);").count(), 2);

        for bytes in [&[0x5a, 0][..], &[0x5a, 1, 0x92, 0x5a, 2], &[0x5a, 3]] {
            assert!(
                format_test_body(&mut String::new(), bytes, None).is_err(),
                "{bytes:02x?}"
            );
        }
    }

    #[test]
    fn default_point_round_trip() {
        let source = round_trip_body(&[0x06, 2, 0, 0]);
        assert!(source.contains("self.select_target_point(PointTarget::Default);"));
        let mut native = String::new();
        format_test_body(&mut native, &[0x06, 2, 0, 1], None).unwrap();
        assert!(native.contains("native("));
    }

    #[test]
    fn in_action_conditions_round_trip() {
        for (group, id) in [(0, 0), (2, 16), (255, 255)] {
            let bytes = vec![0x34, 0, group, id, 0x92, 0x34, 1, 0x48, 1, 0x34, 2];
            let source = round_trip_body(&bytes);
            assert!(source.contains(&format!("self.in_action({group}:{id})")));
        }
    }

    #[test]
    fn target_position_available_round_trips() {
        // 5d 00 resolves the target once and enters the body only when all three
        // reference-point components are nonzero.
        let source = round_trip_body(&[0x5d, 0, 0x92, 0x5d, 1, 0x92, 0x5d, 2]);
        assert!(
            source.contains("if self.target_position_available() {"),
            "{source}"
        );
        assert!(source.contains("} else {"), "{source}");
        // A stray else marker has no matching condition block and is an error,
        // never a silently accepted native escape.
        assert!(format_test_body(&mut String::new(), &[0x5d, 1, 0x92, 0x5d, 2], None).is_err());
    }

    #[test]
    fn in_area_conditions_round_trip() {
        for area in [0u16, 326, 65535] {
            let [hi, lo] = area.to_be_bytes();
            let bytes = vec![0x0e, 0, hi, lo, 0x92, 0x0e, 1, 0x48, 1, 0x0e, 2];
            let source = round_trip_body(&bytes);
            assert!(source.contains(&format!("self.in_area({area})")));
        }
    }

    #[test]
    fn mixed_rage_and_flash_conditions_round_trip() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x35, 0, 0x39, 0, 0xff, 0, 0x39, 1, 0x92, 0x39, 2, 0x35, 1, 4, 0x35, 2, 0xff, 0,
            ],
        );
        image.pointer(0x100 + EVENT_SLOTS[4].root_index as u32 * 4, 0x400);
        image.pointer(0x400, 0x500);
        image.put(
            0x500,
            &[0x35, 0, 0x92, 0x35, 2, 0xff, EVENT_SLOTS[4].ending],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.source.matches("if self.enraged {").count(), 2);
        assert_eq!(result.source.matches("if self.flashed {").count(), 1);
        assert!(!result.source.contains("native(0x35"));
        assert!(!result.source.contains("native(0x39"));
    }

    #[test]
    fn ordered_matches_preserve_canonical_nested_and_irregular_layouts() {
        for (opcode, selector, first, last, nested) in [
            (0x70, "self.species", 1, 11, 11),
            (0x94, "context.debug_mode", 0, 255, 1),
        ] {
            for (bytes, recovered) in [
                (
                    vec![
                        opcode, 0, 2, opcode, 1, first, 0x92, opcode, 1, last, opcode, 3,
                    ],
                    true,
                ),
                (
                    vec![opcode, 0, 1, opcode, 1, first, opcode, 2, 0x92, opcode, 3],
                    true,
                ),
                // A nested match and an early return retain their exact bytes.
                (
                    vec![
                        opcode, 0, 1, opcode, 1, first, opcode, 0, 1, opcode, 1, nested, 0xff, 0,
                        opcode, 3, opcode, 3,
                    ],
                    true,
                ),
                (vec![opcode, 0, 0, opcode, 1, first, opcode, 3], false),
                (vec![opcode, 0, 2, opcode, 1, first, opcode, 3], false),
                (
                    vec![opcode, 0, 2, opcode, 1, last, opcode, 1, first, opcode, 3],
                    false,
                ),
                (
                    vec![opcode, 0, 2, opcode, 1, first, opcode, 1, first, opcode, 3],
                    false,
                ),
            ] {
                let mut source = String::new();
                format_test_body(&mut source, &bytes, None).unwrap();
                assert_eq!(
                    source.contains(&format!("match {selector}")),
                    recovered,
                    "{source}"
                );
                let compiled =
                    dsl::parse(&format!("mhf_ai 1; species 1; fn main() {{ {source} }}"))
                        .unwrap()
                        .compile()
                        .unwrap();
                let mut expected = bytes;
                expected.extend([0xff, 0]);
                assert!(
                    compiled
                        .program
                        .nodes
                        .iter()
                        .any(|node| matches!(node, Node::Script(actual) if *actual == expected)),
                    "{source}"
                );
            }
        }
        // Species 255 remains a case ID regardless of the project species.
        let source = round_trip_body(&[0x70, 0, 1, 0x70, 1, 255, 0x70, 2, 0x92, 0x70, 3]);
        assert!(source.contains("255 => {"));
    }

    #[test]
    fn species_group_recovers_source_order_without_adding_else() {
        let bytes = [
            0x2c, 0, 3, 0x2c, 1, 42, 0x92, 0x2c, 1, 1, 0x2c, 1, 42, 0x48, 2, 0x2c, 3, 0xff, 0,
        ];
        let mut image = main_image();
        image.put(0x300, &bytes);
        let result = decompile(&image, 0x100, 1, 0, None).unwrap();
        assert!(result.source.contains("match self.species_group"));
        assert!(result.source.contains("42 =>"));
        assert!(result.source.contains("1 =>"));
        assert!(!result.source.contains("else =>"));
        assert!(!result.source.contains("native(0x2c"));
        assert!(result.warnings.is_empty());
    }

    #[test]
    fn area_route_profile_preserves_noncanonical_layouts() {
        for bytes in [
            vec![0x57, 0, 1, 0x57, 1, 1, 0, 0x57, 2, 0x57, 3],
            vec![0x57, 0, 2, 0x57, 1, 0, 1, 0x57, 2, 0x57, 3],
            vec![0x57, 0, 2, 0x57, 1, 0, 2, 0x57, 1, 0, 1, 0x57, 2, 0x57, 3],
            vec![0x57, 0, 2, 0x57, 1, 0, 1, 0x57, 1, 0, 1, 0x57, 2, 0x57, 3],
            vec![0x57, 0, 1, 0x57, 1, 0, 1, 0x57, 3],
        ] {
            let source = round_trip_body(&bytes);
            assert!(!source.contains("self.area_route_profile"), "{source}");
        }
    }

    #[test]
    fn area_route_profile_recovers_nested_matches() {
        let bytes = [
            0x57, 0, 2, 0x57, 1, 0, 0, 0x57, 0, 1, 0x57, 1, 0, 255, 0x92, 0x57, 2, 0x57, 3, 0x57,
            1, 0, 255, 0x57, 2, 0x57, 3, 0xff, 0,
        ];
        let mut image = main_image();
        image.put(0x300, &bytes);
        let result = decompile(&image, 0x100, 1, 0, None).unwrap();
        assert_eq!(
            result
                .source
                .matches("match self.area_route_profile")
                .count(),
            2
        );
        assert!(result.warnings.is_empty());
        assert!(!result.source.contains("native(0x57"));
    }

    #[test]
    fn context_query_preserves_noncanonical_layouts() {
        for bytes in [
            vec![0x79, 0, 2, 4, 0x79, 1, 1, 0x79, 2, 0x79, 3],
            vec![0x79, 0, 2, 4, 0x79, 1, 2, 0x79, 1, 1, 0x79, 2, 0x79, 3],
            vec![0x79, 0, 2, 4, 0x79, 1, 1, 0x79, 1, 1, 0x79, 2, 0x79, 3],
            vec![0x79, 0, 1, 4, 0x79, 1, 1, 0x79, 3],
            vec![0x79, 0, 1, 4, 0x79, 1, 1, 0x79, 2, 0x79, 2, 0x79, 3],
        ] {
            let mut source = String::new();
            format_test_body(&mut source, &bytes, None).unwrap();
            assert!(!source.contains("context.query"), "{source}");
            assert!(source.contains("native(0x79"));
        }
    }

    #[test]
    fn context_queries_round_trip_nested_and_multiple_cases_for_any_species() {
        let bytes = [
            0x79, 0, 2, 4, 0x79, 1, 0, 0x79, 0, 1, 255, 0x79, 1, 255, 0x92, 0x79, 2, 0x79, 3, 0x79,
            1, 255, 0x35, 0, 0x92, 0x35, 2, 0x79, 2, 0x92, 0x79, 3, 0xff, 0,
        ];
        for species in [11, 14, 110] {
            let mut image = main_image();
            image.put(0x300, &bytes);
            let result = decompile(&image, 0x100, species, 0, None).unwrap();
            assert_eq!(result.source.matches("context.query").count(), 2);
            assert!(!result.source.contains("zenith"));
            assert!(!result.source.contains("native(0x79"));
            assert!(result.warnings.is_empty());
        }
    }

    #[test]
    fn context_query_decompiles_and_round_trips() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x79, 0, 1, 4, 0x79, 1, 1, 0x92, 0x79, 2, 0xff, 0, 0x79, 3, 0xff, 0,
            ],
        );
        let result = decompile(&image, 0x100, 11, 0, None).unwrap();
        assert!(result.source.contains("match context.query(4) {"));
        assert!(result.source.contains("else => {\n            end;"));
        assert!(!result.source.contains("native(0x79"));

        let mut source = String::new();
        format_test_body(
            &mut source,
            &[0x79, 0, 1, 3, 0x79, 1, 1, 0x92, 0x79, 2, 0x79, 3],
            None,
        )
        .unwrap();
        assert!(!source.contains("self.zenith"));
        assert!(source.contains("match context.query(3)"));
    }

    #[test]
    fn flashed_blocks_decompile_and_round_trip_in_states_and_events() {
        let mut image = main_image();
        image.put(
            0x300,
            &[
                0x39, 0, 0x39, 0, 0xff, 0, 0x39, 1, 4, 0x39, 2, 0x39, 1, 0x92, 0x39, 2, 0xff, 0,
            ],
        );
        image.pointer(0x100 + EVENT_SLOTS[3].root_index as u32 * 4, 0x400);
        image.pointer(0x400, 0x500);
        image.put(
            0x500,
            &[0x39, 0, 0x92, 0x39, 2, 0xff, EVENT_SLOTS[3].ending],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.source.matches("if self.flashed {").count(), 3);
        assert_eq!(result.source.matches("} else {").count(), 2);
        assert!(!result.source.contains("native(0x39"));
        assert!(
            result
                .source
                .contains("        if self.flashed {\n            end;")
        );
        // Repeated native else markers cannot be expressed as an ordinary if.
        let mut source = String::new();
        format_test_body(&mut source, &[0x39, 0, 0x39, 1, 0x39, 1, 0x39, 2], None).unwrap();
        assert!(!source.contains("if self.flashed"));
        assert_eq!(source.matches("native(").count(), 4);
    }

    #[test]
    fn request_protocol_round_trips_as_a_handler_and_stays_native_otherwise() {
        // root[0] state 0 dispatches to root[1] slot 3, like the Rathian main
        // script does with 81 07 and its handler.
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.put(
            0x300,
            &[
                0x1b, 0, 1, 0x0c, 4, 1, 0x81, 3, 0x2b, 0, 4, 1, 0xff, 0, 0x2b, 2, 0x1b, 2, 0xff, 0,
            ],
        );
        image.pointer(0x80c, 0x400);
        image.put(0x400, &[0x1e, 0x0d, 0x04, 0xff, 0x01]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("handle sub_1_3() then {"));
        assert!(result.source.contains("handler fn sub_1_3()"));
        assert!(result.source.contains("clear_requests();"));
        assert!(result.source.contains("pass;"));
        assert!(!result.source.contains("mark_unhandled();"));
        assert!(!result.source.contains("native(0x1b"));
        assert!(!result.source.contains("native(0x0c"));

        // Other MIND values and a protocol with an else are not this protocol.
        for body in [
            vec![0x1b, 0, 0, 0x92, 0x1b, 2, 0xff, 0],
            vec![
                0x1b, 0, 1, 0x0c, 4, 1, 0x81, 3, 0x2b, 0, 4, 1, 0x2b, 1, 0x92, 0x2b, 2, 0x1b, 2,
                0xff, 0,
            ],
        ] {
            let mut source = String::new();
            format_test_body(&mut source, &body, None).unwrap();
            assert!(!source.contains("handle "), "{source}");
            assert!(source.contains("native(0x1b"), "{source}");
        }

        // A final automatic return has already been removed; retain the clear
        // as a statement rather than infer pass from a nested body boundary.
        image.put(0x300, &[0x81, 3, 0xff, 0]);
        image.pointer(0x80c, 0x400);
        image.pointer(0x808, 0x600);
        image.put(0x600, &[0x81, 3, 0xff, 1]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(!result.source.contains("handle "));
        assert!(!result.source.contains("handler fn"));
        assert!(result.source.contains("mark_unhandled();"));
    }

    #[test]
    fn target_angle_matches_preserve_shared_subscripts_and_event_returns() {
        let event = &EVENT_SLOTS[3];
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.pointer(0x80c, 0x400);
        image.pointer(0x100 + event.root_index as u32 * 4, 0x500);
        image.pointer(0x500, 0x600);
        image.put(0x300, &[0x20, 0, 1, 0x20, 1, 1, 0x81, 3, 0x20, 3, 0xff, 0]);
        image.put(
            0x400,
            &[0x20, 0, 1, 0x20, 1, 32, 0xff, 1, 0x20, 3, 0x92, 0xff, 1],
        );
        image.put(
            0x600,
            &[
                0x20,
                0,
                1,
                0x20,
                1,
                255,
                0xff,
                event.ending,
                0x20,
                2,
                0x81,
                3,
                0x20,
                3,
                0xff,
                event.ending,
            ],
        );
        // Decompilation recompiles every state, event, and shared subscript.
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(
            result.source.matches("match self.target_angle() {").count(),
            3
        );
        assert_eq!(result.source.matches("sub_1_3();").count(), 2);
        assert_eq!(result.source.matches("return;").count(), 2);
        assert!(!result.source.contains("native("), "{}", result.source);
    }

    #[test]
    fn target_angle_matches_preserve_nested_request_handlers_and_pass() {
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.pointer(0x80c, 0x400);
        image.put(
            0x300,
            &[
                0x20, 0, 1, 0x20, 1, 32, 0x1b, 0, 1, 0x0c, 4, 1, 0x81, 3, 0x2b, 0, 4, 1, 0xff, 0,
                0x2b, 2, 0x1b, 2, 0x20, 3, 0xff, 0,
            ],
        );
        image.put(
            0x400,
            &[0x20, 0, 1, 0x20, 1, 1, 0x92, 0x20, 3, 0x0d, 4, 0xff, 1],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(
            result.source.matches("match self.target_angle() {").count(),
            2
        );
        assert!(result.source.contains("handle sub_1_3() then {"));
        assert!(result.source.contains("handler fn sub_1_3()"));
        assert!(result.source.contains("pass;"));
        assert!(!result.source.contains("native("), "{}", result.source);

        // Handler recovery requires a clear's return path to finish with only
        // closing markers. The extra outer return keeps this callee ordinary
        // and its caller's request protocol raw, preserving both blocks' bytes.
        image.put(
            0x400,
            &[0x20, 0, 1, 0x20, 1, 1, 0x0d, 4, 0xff, 1, 0x20, 3, 0xff, 1],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        assert_eq!(
            result.source.matches("match self.target_angle() {").count(),
            1
        );
        assert!(result.source.contains("native(0x20, 0x00, 0x01);"));
        assert!(result.source.contains("native(0x1b, 0x00, 0x01);"));
        assert!(result.source.contains("sub_1_3();"));
        assert!(result.source.contains("pass;"));
        assert!(!result.source.contains("handle "));
        assert!(!result.source.contains("handler fn"));
    }

    #[test]
    fn raw_request_guard_prevents_handler_declaration() {
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.pointer(0x80c, 0x400);
        image.put(0x400, &[0x1e, 0x0d, 4, 0xff, 1]);
        // A valid standalone MIND check forces the same body as the canonical
        // protocol to remain raw. Its callee must remain an ordinary function.
        image.put(
            0x300,
            &[
                0x1b, 0, 0, 0x1b, 2, 0x1b, 0, 1, 0x0c, 4, 1, 0x81, 3, 0x2b, 0, 4, 1, 0xff, 0, 0x2b,
                2, 0x1b, 2, 0xff, 0,
            ],
        );
        // decompile also recompiles the result and verifies the original bytes.
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(!result.source.contains("handler fn"));
        assert!(!result.source.contains("handle "));
        assert!(result.source.contains("sub_1_3();"));
        assert!(result.source.contains("mark_unhandled();"));
    }

    #[test]
    fn rathian_request_dispatch_folds_into_handle_without_touching_dispatch_bytes() {
        // Real layout: 0x11854C88 guards 81 07, and 0x11855184 plus 0x118564A0
        // are the dispatch subscript and one request subscript.
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.put(
            0x300,
            &[
                0x1b, 0, 1, 0x0c, 4, 1, 0x81, 7, 0x2b, 0, 4, 1, 0xff, 0, 0x2b, 2, 0x1b, 2, 0x48, 9,
                0xff, 0,
            ],
        );
        image.pointer(0x81c, 0x400);
        image.put(
            0x400,
            &[
                0x1d, 0, 7, 0x1d, 1, 1, 0x82, 7, 1, 0x1d, 1, 2, 0x82, 7, 2, 0x1d, 3, 0xff, 1,
            ],
        );
        // Table 22 is 15 + 7; request 2 returns early after clearing.
        image.pointer(0x100 + 22 * 4, 0x700);
        image.pointer(0x704, 0x500);
        image.pointer(0x708, 0x600);
        image.put(0x500, &[0x92, 0xff, 0x02]);
        image.put(
            0x600,
            &[
                0x35, 0, 0x1e, 0x0d, 0x04, 0xff, 0x02, 0x35, 2, 0x92, 0xff, 0x02,
            ],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(
            result
                .source
                .contains("handle sub_1_7() then {\n        end;\n    }")
        );
        assert!(result.source.contains("handler fn sub_1_7()"));
        assert!(!result.source.contains("native(0x0c"));
        // This fixture keeps the real count (7) with only two cases, so the
        // dispatcher stays native instead of recovering a partial match.
        assert!(result.source.contains("native(0x1d, 0x00, 0x07);"));
        assert!(!result.source.contains("match self.request"));
        assert!(result.source.contains("sub_22_1();"));
        // This branch returns after the clear; other paths keep their code.
        assert!(result.source.contains("fn sub_22_2()"));
        assert!(result.source.contains("pass;"));
        assert!(!result.source.contains("native(0x0d, 0x04);"));
    }

    #[test]
    fn request_handlers_preserve_unhandled_marking_in_nested_blocks() {
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.pointer(0x80c, 0x400);
        image.put(
            0x300,
            &[
                0x1b, 0, 1, 0x0c, 4, 1, 0x81, 3, 0x2b, 0, 4, 1, 0x92, 0x2b, 2, 0x1b, 2, 0xff, 0,
            ],
        );
        for body in [
            vec![0x0d, 4, 0x48, 7, 0xff, 1],
            vec![0x35, 0, 0x0d, 4, 0x35, 2, 0x92, 0xff, 1],
            vec![0x20, 0, 1, 0x20, 1, 1, 0x0d, 4, 0x20, 3, 0x92, 0xff, 1],
            vec![
                0x20, 0, 1, 0x20, 1, 1, 0x92, 0x20, 2, 0x0d, 4, 0x20, 3, 0x92, 0xff, 1,
            ],
            vec![0x80, 0, 1, 0x80, 1, 32, 0x0d, 4, 0x80, 0xff, 0x92, 0xff, 1],
            vec![
                0x83, 0, 1, 0x83, 1, 0x0d, 4, 0x83, 2, 0x0d, 4, 0x83, 0xff, 0x92, 0xff, 1,
            ],
            vec![0x0d, 4, 0x0c, 4, 1, 0x92, 0xff, 1],
        ] {
            image.put(0x400, &body);
            // decompile recompiles every script and compares its bytes.
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert!(result.warnings.is_empty());
            assert!(
                result.source.contains("handle sub_1_3() then {"),
                "{}",
                result.source
            );
            assert!(
                result.source.contains("handler fn sub_1_3()"),
                "{}",
                result.source
            );
            assert!(
                result.source.contains("mark_unhandled();"),
                "{}",
                result.source
            );
            assert!(!result.source.contains("pass;"), "{}", result.source);
        }
    }

    #[test]
    fn ordinary_unhandled_marking_preserves_continuation() {
        for body in [
            vec![0x0d, 4, 0x92, 0xff, 1],
            vec![0x35, 0, 0x0d, 4, 0x35, 2, 0xff, 1],
        ] {
            let mut image = Image::default();
            image.put(0x100, &[0; 64]);
            image.pointer(0x100, 0x200);
            image.pointer(0x104, 0x800);
            image.pointer(0x200, 0x300);
            image.put(0x300, &[0x81, 3, 0xff, 0]);
            image.pointer(0x80c, 0x400);
            image.put(0x400, &body);
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert!(result.source.contains("mark_unhandled();"));
            assert!(!result.source.contains("pass;"));
        }
    }

    #[test]
    fn unhandled_marking_preserves_other_clear_selectors() {
        let bytes: Vec<u8> = (0..=u8::MAX)
            .flat_map(|selector| [0x0d, selector])
            .collect();
        let source = round_trip_body(&bytes);
        assert_eq!(source.matches("mark_unhandled();").count(), 1);
        assert!(!source.contains("pass;"));
        assert!(!source.contains("native(0x0d, 0x04);"));
        for selector in (0..=u8::MAX).filter(|selector| *selector != 4) {
            assert!(source.contains(&format!("native(0x0d, 0x{selector:02x});")));
        }
        assert!(format_test_body(&mut String::new(), &[0x0d], None).is_err());
    }

    #[test]
    fn state_reset_tails_are_implicit_but_early_resets_remain_lossless() {
        let mut image = main_image();
        image.pointer(0x204, 0x400);
        image.put(0x300, &[0x39, 0, 7, 1, 0x39, 2, 0xff, 0]);
        image.put(0x400, &[0x92, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("fn main()"));
        assert!(
            result
                .source
                .contains("state_1 = 1 => {\n        nop();\n    }")
        );
        assert!(!result.source.contains("native(0xff, 0x00)"));

        image.put(0x300, &[0x39, 0, 0xff, 0, 0x39, 2, 0x92, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.source.matches("end;").count(), 1);

        image.put(0x300, &[0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("fn main() {\n}"));
        assert_eq!(
            without_automatic_tail(&[5, 0, 0xff, 0], 0).unwrap(),
            [5, 0, 0xff, 0]
        );
    }

    #[test]
    fn named_events_omit_only_their_matching_tail_and_round_trip() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x500);
        image.pointer(0x500, 0x600);
        image.put(0x600, &[4]);
        for (index, event) in EVENT_SLOTS.iter().enumerate() {
            let cell = 0x200 + index as u32 * 4;
            let body = 0x300 + index as u32 * 16;
            image.pointer(0x100 + event.root_index as u32 * 4, cell);
            image.pointer(cell, body);
            image.put(body, &[0x92, 0xff, event.ending]);
        }
        // decompile checks the compiled bytes of every exported body itself.
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for event in EVENT_SLOTS {
            assert!(result.source.contains(&format!("{} => {{", event.name)));
        }
        assert!(!result.source.contains("native(0xff"));
        assert!(!result.source.contains("_end()"));

        // A different native ending must not be silently replaced.
        image.put(0x300, &[0x92, 0xff, 0xfd]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("native(0xff, 0xfd);"));

        // An early exit inside a structured conditional keeps its `return;`;
        // only the outer tail is reconstructed by the compiler.
        image.put(0x300, &[0x39, 0, 0xff, 0xf5, 0x39, 2, 0x92, 0xff, 0xf5]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains(
            "dung_reaction => {\n        if self.flashed {\n            return;\n        }\n        nop();\n    }"
        ));

        // Both structured and raw outer blocks retain the event's early return
        // and preserve all enclosing markers.
        for (opcode, expected) in [
            (
                0x03,
                "if self.check_pending_area() {\n            if self.flashed {\n                return;\n            }\n        }",
            ),
            (
                0x14,
                "if self.target_angle_at_least(32) {\n            if self.flashed {\n                return;\n            }\n        }",
            ),
            (
                0x42,
                "native(0x42, 0x00, 0x20);\n        if self.flashed {\n            return;\n        }\n        native(0x42, 0x02);",
            ),
        ] {
            let mut bytes = vec![opcode, 0];
            if opcode != 0x03 {
                bytes.push(0x20);
            }
            bytes.extend_from_slice(&[0x39, 0, 0xff, 0xf5, 0x39, 2, opcode, 2, 0xff, 0xf5]);
            image.put(0x300, &bytes);
            let result = decompile(&image, 0x100, 6, 0, None).unwrap();
            assert!(result.source.contains(expected), "{}", result.source);
        }

        image.put(0x300, &[0xff, 0xf5]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("dung_reaction => {\n    }"));
    }

    #[test]
    fn follows_sparse_cyclic_states_without_reading_a_guessed_table_length() {
        let mut image = main_image();
        image.pointer(0x200 + 9 * 4, 0x400);
        image.put(0x300, &[5, 3, 6, 0, 7, 9]);
        image.put(0x400, &[0x99, 2, 4]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        assert!(result.source.contains("self.action(3:6, 0);"));
        assert!(result.source.contains("transition state_9;"));
        assert!(result.source.contains("state_9 = 9"));
        assert!(result.source.contains("native(0x99, 0x02);"));
    }

    #[test]
    fn contents_calls_are_not_main_state_references() {
        let mut image = main_image();
        image.put(0x300, &[0x81, 200, 4]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.warnings.len(), 1);
        assert!(result.source.contains("fn main()"));
        assert!(!result.source.contains("state_200"));
        assert!(result.source.contains("native(0x81, 0xc8)"));
    }

    #[test]
    fn recovers_nested_sparse_subscripts_and_same_level_cycles() {
        let mut image = Image::default();
        image.put(0x100, &[0; 64]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x1000);
        image.pointer(0x13c, 0x2000);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0x81, 3, 0xff, 0]);
        image.pointer(0x100c, 0x400);
        image.put(0x400, &[0x82, 0, 200, 0xff, 1]);
        image.pointer(0x2320, 0x500);
        image.put(0x500, &[0x39, 0, 0xff, 2, 0x39, 2, 0x82, 0, 201]);
        image.pointer(0x2324, 0x600);
        image.put(0x600, &[0x82, 0, 200]);
        let result = decompile(&image, 0x100, 6, 0, Some(31)).unwrap();
        assert!(result.warnings.is_empty());
        assert!(result.source.contains("@slot(table = 1, index = 3)"));
        assert!(result.source.contains("@slot(table = 15, index = 200)"));
        assert!(result.source.contains("sub_15_201();"));
        assert!(result.source.contains("return;"));
        assert!(!result.source.contains("native(0x81"));
        assert!(!result.source.contains("native(0x82"));
        assert!(!result.source.contains("import "));
        super::super::dsl::Project::single(Some(31), 6, result.source)
            .compile()
            .unwrap();
    }

    #[test]
    fn returns_inside_subscript_blocks_preserve_closing_markers() {
        for table in [1, 9, 15] {
            let slot = NativeSlot { table, index: 1 };
            let ending = slot.ending();
            for body in [
                // The 03 condition must stay distinct from a table-9 FF 03 return.
                vec![0x03, 0, 0xff, ending, 0x03, 2, 0xff, ending],
                // Structured angle checks retain the slot-specific early return.
                vec![0x14, 0, 45, 0xff, ending, 0x14, 2, 0xff, ending],
                // Recovered angle-match bodies keep the same return scope.
                vec![0x20, 0, 1, 0x20, 1, 32, 0xff, ending, 0x20, 3, 0xff, ending],
                // This condition has no structured DSL equivalent.
                vec![0x42, 0, 45, 0xff, ending, 0x42, 2, 0xff, ending],
                // Noncanonical weights keep this random block as native bytes.
                vec![
                    0x80, 0, 1, 0x80, 1, 1, 0xff, ending, 0x80, 0xff, 0xff, ending,
                ],
                // Nested conditions preserve the early return and subsequent bytes.
                vec![
                    0x5d, 0, 0x09, 0, 0xff, ending, 0x09, 2, 0x92, 0x5d, 2, 0xff, ending,
                ],
                // A different return convention must stay native.
                vec![
                    0x5d, 0, 0xff, 0xfc, 0xff, ending, 0x92, 0x5d, 2, 0xff, ending,
                ],
            ] {
                let mut image = Image::default();
                image.put(0x100, &[0; 64]);
                image.pointer(0x100, 0x200);
                image.pointer(0x100 + table as u32 * 4, 0x800);
                image.pointer(0x200, 0x300);
                let mut entry = slot.call();
                entry.extend_from_slice(&[0xff, 0]);
                image.put(0x300, &entry);
                image.pointer(0x804, 0x400);
                image.put(0x400, &body);
                // decompile checks every recovered script against its original bytes.
                let result = decompile(&image, 0x100, 2, 0, None).unwrap();
                assert!(result.warnings.is_empty());
                assert!(result.source.contains("return;"));
                assert!(
                    !result
                        .source
                        .contains(&format!("native(0xff, 0x{ending:02x});"))
                );
                if body.windows(2).any(|bytes| bytes == [0xff, 0xfc]) {
                    assert!(result.source.contains("native(0xff, 0xfc);"));
                }
            }
        }
    }

    #[test]
    fn discovers_states_referenced_by_subscripts() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x104, 0x800);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0x81, 1, 0xff, 0]);
        image.pointer(0x804, 0x400);
        image.put(0x400, &[0x07, 9]);
        image.pointer(0x224, 0x500);
        image.put(0x500, &[4]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("state_9 = 9"));
    }

    #[test]
    fn native_sample_keeps_nested_random_branches_and_zero_operands() {
        // ZZ HD 1179CEA4..1179CEDB, species 6 main[0]. The first FF 00
        // is INSIDE 39/00: its close and fallback must also be copied.
        let bytes = [
            0x39, 0, 0x0b, 0, 0, 5, 0, 6, 0, 0x0b, 1, 0x80, 0, 3, 0x80, 1, 16, 5, 0, 6, 0, 0x80, 2,
            8, 5, 3, 6, 0, 0x80, 3, 8, 5, 3, 3, 0, 0x80, 0xff, 0x0b, 2, 0xff, 0, 0x39, 2, 0x0b, 0,
            0, 7, 1, 0x0b, 1, 7, 2, 0x0b, 2, 0xff, 0,
        ];
        let mut image = Image::default();
        image.put(0x100, &bytes);
        assert_eq!(script(&image, 0x100).unwrap(), bytes);
        assert!(bytecode::validate_structure(&bytes).is_ok());
        image.put(0x200, &[0; 60]);
        image.pointer(0x200, 0x300);
        image.pointer(0x300, 0x100);
        image.pointer(0x304, 0x400);
        image.pointer(0x308, 0x400);
        image.put(0x400, &[4]);
        // Includes transitions inside native blocks; function lowering must
        // retain the closing markers and fallback, not truncate at transition.
        assert!(decompile(&image, 0x200, 6, 0, None).is_ok());
        // Exactly the old export: it passed the width/round-trip checks, but
        // native marker scanning cannot escape the zero padding after it.
        assert!(bytecode::decode(&bytes[..41]).is_ok());
        assert!(
            bytecode::validate_structure(&bytes[..41])
                .unwrap_err()
                .to_string()
                .contains("0x39")
        );
    }

    #[test]
    fn terminal_in_a_conditional_does_not_truncate_the_other_branch() {
        let bytes = [
            0x34, 0, 3, 4, 5, 3, 7, 0, 0xff, 0xfd, 0x34, 2, 5, 0, 1, 0, 0xff, 0xfd,
        ];
        let mut image = Image::default();
        image.put(0x100, &bytes);
        assert_eq!(script(&image, 0x100).unwrap(), bytes);
        // List-family selector 2 is an else marker, not the end marker (3).
        let bytes = [
            0x1c, 0, 1, 0x1c, 1, 50, 4, 0x1c, 2, 5, 0, 6, 0, 0x1c, 3, 0xff, 0,
        ];
        image.put(0x100, &bytes);
        assert_eq!(script(&image, 0x100).unwrap(), bytes);
    }

    #[test]
    fn ambiguous_and_unreadable_entries_are_inherited_not_cleared() {
        let mut image = main_image();
        image.put(0x300, &[5, 0, 6, 0, 0]);
        image.pointer(0x200 + 4, 0x400);
        image.put(0x400, &[0x24, 0, 2]);
        let result = decompile(&image, 0x100, 6, 1, Some(31)).unwrap();
        super::super::dsl::Project::single(Some(31), 6, result.source.clone())
            .compile()
            .unwrap();
        assert!(result.source.contains("\nmap 31;\n"));
        assert_eq!(result.warnings.len(), 1);
        assert!(!result.source.contains("state_1 ="));
        image.put(0x300, &[5]);
        image.0.remove(&0x301);
        assert!(decompile(&image, 0x100, 6, 1, Some(31)).is_err());
    }

    #[test]
    fn stops_at_proven_end_and_rejects_unclosed_or_redispatch_forms() {
        for bytes in [vec![0], vec![4], vec![7, 0], vec![0xff, 0]] {
            let mut image = Image::default();
            image.put(0x100, &bytes);
            assert_eq!(script(&image, 0x100).unwrap(), bytes);
        }
        for bytes in [
            vec![5, 0],
            vec![0x0b, 0, 0, 0],
            vec![0xff, 5],
            vec![0xff, 0x48, 0],
        ] {
            let mut image = Image::default();
            image.put(0x100, &bytes);
            assert!(script(&image, 0x100).is_err());
        }
    }

    #[test]
    fn public_extraction_uses_tail_calls_only_with_a_known_level() {
        for (table, call, ending) in [(1, vec![0x81, 7], 1), (18, vec![0x82, 3, 9], 2)] {
            let mut bytes = call.clone();
            bytes.extend_from_slice(&[0xff, ending]);
            let mut image = Image::default();
            image.put(0x100, &bytes);
            assert_eq!(extract_script(&image, 0x100, None).unwrap(), bytes);
            assert_eq!(
                extract_script(&image, 0x100, Some(NativeSlot { table, index: 0 })).unwrap(),
                call
            );
        }
    }

    #[test]
    fn public_extraction_keeps_nested_returns_and_enforces_the_search_budget() {
        let mut image = Image::default();
        let bytes = [0x35, 0, 0xff, 1, 0x35, 2, 0xff, 2];
        image.put(0x100, &bytes);
        assert_eq!(extract_script(&image, 0x100, None).unwrap(), bytes);

        image.put(0x100, &vec![0x92; MAX_SCRIPT_BYTES]);
        assert!(
            extract_script(&image, 0x100, None)
                .unwrap_err()
                .to_string()
                .contains("64 KiB")
        );
    }
}
