//! Bounded extraction of reachable state scripts and event entry scripts.
//!
//! Native tables have no lengths. Follow explicit state references instead of
//! guessing a table end from adjacent pointers. Explicit 81/82 references
//! recover annotated functions. Other tables remain
//! inherited through `base native`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::dsl::{
    condition::{Condition, Degrees, Mode},
    slot::NativeSlot,
    target::{Direction, TargetStrategy},
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
            script_in(memory, address, slot)
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
    source.push_str("\nstates {\n");
    for &index in states.keys() {
        if index == 0 {
            continue;
        }
        writeln!(source, "    state_{index} = {index} => state_{index};").unwrap();
    }
    source.push_str("}\n\nevents {\n");
    for index in events.keys() {
        let name = EVENT_SLOTS[*index].name;
        writeln!(source, "    {name} => on_{name};").unwrap();
    }
    source.push_str("}\n");
    for (&index, bytes) in &states {
        if index == 0 {
            writeln!(source, "\nfn main() {{").unwrap();
        } else {
            writeln!(source, "\nfn state_{index}() {{").unwrap();
        }
        format_recovered_body(
            &mut source,
            without_automatic_tail(bytes, 0x00)?,
            Some(&states),
            &subs,
            None,
        )?;
        source.push_str("}\n");
    }
    for (&index, bytes) in &events {
        let name = EVENT_SLOTS[index].name;
        writeln!(source, "\nfn on_{name}() {{").unwrap();
        let body = without_automatic_tail(bytes, EVENT_SLOTS[index].ending)?;
        format_recovered_body(&mut source, body, None, &subs, None)?;
        source.push_str("}\n");
    }
    for (slot, bytes) in &subs {
        writeln!(
            source,
            "\n@slot(table = {}, index = {})\nfn {}() {{",
            slot.table,
            slot.index,
            sub_name(*slot)
        )
        .unwrap();
        format_recovered_body(
            &mut source,
            without_automatic_tail(bytes, slot.ending())?,
            None,
            &subs,
            Some(*slot),
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
    for (index, bytes) in &states {
        let super::Node::Table(table) = &compiled.program.nodes[root.get(0).unwrap()] else {
            unreachable!()
        };
        if !matches!(
            &compiled.program.nodes[table.get(usize::from(*index)).unwrap()],
            super::Node::Script(actual) if matches_export(actual, bytes)?
        ) {
            return Err(Error::new("state round-trip mismatch"));
        }
    }
    for (index, bytes) in &events {
        let super::Node::Table(cell) =
            &compiled.program.nodes[root.get(EVENT_SLOTS[*index].root_index).unwrap()]
        else {
            unreachable!()
        };
        if !matches!(
            &compiled.program.nodes[cell.get(0).unwrap()],
            super::Node::Script(actual) if matches_export(actual, bytes)?
        ) {
            return Err(Error::new("event round-trip mismatch"));
        }
    }
    for (slot, bytes) in &subs {
        let super::Node::Table(table) = &compiled.program.nodes[root.get(slot.table).unwrap()]
        else {
            unreachable!()
        };
        if !matches!(&compiled.program.nodes[table.get(usize::from(slot.index)).unwrap()], super::Node::Script(actual) if matches_export(actual, bytes)?)
        {
            return Err(Error::new(format!(
                "subscript round-trip mismatch: {}:{}",
                slot.table, slot.index
            )));
        }
    }
    Ok(Decompiled { source, warnings })
}

/// The only canonicalization permitted on export is the equivalent RNG opcode.
/// Decode boundaries so operand bytes with value 7B are never rewritten.
fn matches_export(actual: &[u8], original: &[u8]) -> Result<bool> {
    let mut expected = original.to_vec();
    for instruction in bytecode::decode(original)? {
        if instruction.bytes == [0x7b] {
            expected[instruction.offset] = 0x84;
        }
    }
    Ok(actual == expected)
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
    script_in(memory, address, None)
}

fn script_in(memory: &impl Memory, address: u32, slot: Option<NativeSlot>) -> Result<Vec<u8>> {
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
fn format_body(
    out: &mut String,
    bytes: &[u8],
    states: Option<&BTreeMap<u8, Vec<u8>>>,
) -> Result<()> {
    format_recovered_body(out, bytes, states, &BTreeMap::new(), None)
}

fn sub_name(slot: NativeSlot) -> String {
    format!("sub_{}_{}", slot.table, slot.index)
}

type BranchBodies = Vec<(u8, Vec<u8>)>;

fn distance_branches(instructions: &[bytecode::Instruction]) -> Option<(usize, BranchBodies)> {
    let [0x83, 0, count, 0x83, 1] = instructions.first()?.bytes.as_slice() else {
        return None;
    };
    if !(1..=4).contains(count) {
        return None;
    }
    let mut branches = vec![(1, Vec::new())];
    let mut structure = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate().skip(1) {
        if structure.is_closed() {
            match instruction.bytes.as_slice() {
                [0x83, 0xff] => {
                    return (branches.len() == usize::from(*count) + 1)
                        .then_some((index + 1, branches));
                }
                [0x83, selector] => {
                    if usize::from(*selector) != branches.len() + 1 || *selector > count + 1 {
                        return None;
                    }
                    branches.push((*selector, Vec::new()));
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

/// Recover only canonical, exhaustive weighted choices; all other layouts stay raw.
fn random_branches(instructions: &[bytecode::Instruction]) -> Option<(usize, BranchBodies)> {
    let [0x80, 0, count, 0x80, 1, first_weight] = instructions.first()?.bytes.as_slice() else {
        return None;
    };
    if !(1..=31).contains(count) {
        return None;
    }
    if *first_weight > 32 {
        return None;
    }
    let mut branches: Vec<(u8, Vec<u8>)> = vec![(*first_weight, Vec::new())];
    let mut structure = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate().skip(1) {
        if structure.is_closed() {
            match instruction.bytes.as_slice() {
                [0x80, 0xff] => {
                    return (branches.len() == usize::from(*count)
                        && branches
                            .iter()
                            .map(|(weight, _)| usize::from(*weight))
                            .sum::<usize>()
                            == 32)
                        .then_some((index + 1, branches));
                }
                [0x80, selector, weight] if *selector != 0 => {
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

fn format_recovered_body(
    out: &mut String,
    bytes: &[u8],
    states: Option<&BTreeMap<u8, Vec<u8>>>,
    subs: &BTreeMap<NativeSlot, Vec<u8>>,
    scope: Option<NativeSlot>,
) -> Result<()> {
    let instructions = bytecode::decode(bytes)?;
    // Only raise complete, single-else known blocks. Unusual native structures
    // remain byte-preserving escapes rather than acquiring new DSL semantics.
    let mut blocks = Vec::new();
    let mut structured = true;
    for instruction in &instructions {
        if Condition::from_opcode(instruction.opcode).is_none() {
            continue;
        }
        match instruction.bytes.as_slice() {
            [0x78, 0, min, max] => {
                structured &= min <= max;
                blocks.push((0x78, false));
            }
            [0x0b, 0, value] => {
                structured &= Mode::from_native(*value).is_some();
                blocks.push((0x0b, false));
            }
            [opcode, 0] => blocks.push((*opcode, false)),
            [opcode, 1] => match blocks.last_mut() {
                Some((open, seen_else)) if open == opcode && !*seen_else => *seen_else = true,
                _ => structured = false,
            },
            [opcode, 2] => {
                if !blocks.pop().is_some_and(|(open, _)| open == *opcode) {
                    structured = false;
                }
            }
            _ => structured = false,
        }
    }
    structured &= blocks.is_empty();
    let mut indent = 1;
    let mut skip_until = 0;
    let mut raw_blocks = bytecode::ScriptStructure::default();
    for (index, instruction) in instructions.iter().enumerate() {
        if index < skip_until {
            continue;
        }
        let choice = if structured {
            random_branches(&instructions[index..])
                .map(|(length, branches)| (length, branches, false))
                .or_else(|| {
                    distance_branches(&instructions[index..])
                        .map(|(length, branches)| (length, branches, true))
                })
        } else {
            None
        };
        if let Some((length, branches, distance)) = choice {
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
                format_recovered_body(&mut rendered, &body, states, subs, scope)?;
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
        if structured && let Some(condition) = Condition::from_opcode(instruction.opcode) {
            let marker = match b.as_slice() {
                [0x78, 0, min, max] => Some(format!(
                    "if {} {{",
                    Condition::TargetAngleIn {
                        min: Degrees::from_native(*min),
                        max: Degrees::from_native(*max)
                    }
                    .name()
                )),
                [0x0b, 0, value] => Mode::from_native(*value)
                    .map(|mode| format!("if {} {{", Condition::ModeIs(mode).name())),
                [_, 0] => Some(format!("if {} {{", condition.name())),
                [_, 1] => Some("} else {".into()),
                [_, 2] => Some("}".into()),
                _ => None,
            };
            if let Some(marker) = marker {
                if b[1] != 0 {
                    indent -= 1;
                }
                writeln!(out, "{}{marker}", "    ".repeat(indent)).unwrap();
                if b[1] != 2 {
                    indent += 1;
                }
                continue;
            }
        }
        // Structured branches own their closing markers. Raw blocks do not:
        // raising an early exit to `return` would discard their remaining bytes.
        raw_blocks.push(b)?;
        let statement = match b.as_slice() {
            bytes
                if let Some(slot) = NativeSlot::from_call(bytes)
                    && subs.contains_key(&slot) =>
            {
                format!("{}();", sub_name(slot))
            }
            [0xff, ending]
                if structured
                    && raw_blocks.is_closed()
                    && scope.is_some_and(|slot| slot.ending() == *ending) =>
            {
                "return;".into()
            }
            [0x11] => "self.bind_awareness_target();".into(),
            [0x13] => "self.bind_current_target();".into(),
            [0x40, value] if let Some(mode) = Mode::from_native(*value) => {
                format!("self.set_mode({});", mode.name())
            }
            [0x4d] => "self.update_target_position();".into(),
            [0x7b] | [0x84] => "self.increment_random_value();".into(),
            [opcode] if let Some(strategy) = TargetStrategy::from_opcode(*opcode) => {
                format!(
                    "self.select_target_entity(TargetStrategy::{});",
                    strategy.name()
                )
            }
            [0x05, group, id, arg] => format!("action[{group}:{id}]({arg});"),
            [0x06, 1, 0, slot] => format!("self.select_target_entity({slot});"),
            [0x06, 2, 1, index] => format!("self.select_target_point({index});"),
            [0x06, 6, value, 0] if let Some(direction) = Direction::from_native(*value) => {
                format!("self.select_target_point(Direction::{});", direction.name())
            }
            [0x07, index]
                if *index != 0 && states.is_some_and(|states| states.contains_key(index)) =>
            {
                format!("transition state_{index};")
            }
            [0x04] if states.is_some() => "restart;".into(),
            [0x68] => "stop();".into(),
            [0xff, 0x00] => "reset;".into(),
            [0xff, 0xf7] => "reset forget_target;".into(),
            [0x1e] => "clear_target();".into(),
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
    fn target_strategies_decompile_and_recompile_losslessly() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0x52, 0x53, 0x5f, 0x7e, 0x12, 0x58, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for name in [
            "AllowedAreas",
            "SameArea",
            "GroundFiltered",
            "PlayerOrMonster",
            "TrackedBySlot",
            "LeaderTarget",
        ] {
            assert!(result.source.contains(&format!(
                "self.select_target_entity(TargetStrategy::{name});"
            )));
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
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn mode_target_and_random_methods_round_trip() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(
            0x300,
            &[0x40, 0, 0x40, 1, 0x4d, 0x84, 0x7b, 0x40, 2, 0xff, 0],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        for method in [
            "self.set_mode(Mode::Normal);",
            "self.set_mode(Mode::Attack);",
            "self.update_target_position();",
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
        assert!(!result.source.contains("native(0x7b)"));
    }

    #[test]
    fn weighted_choices_round_trip_nested_blocks_and_preserve_unusual_weights() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
        assert!(result.source.contains("reset forget_target;"));
        image.put(0x300, &[0x80, 0, 1, 0x80, 1, 31, 0x92, 0x80, 0xff, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(!result.source.contains("random {"));
        assert!(result.source.contains("native(0x80"));
    }

    #[test]
    fn distance_matches_round_trip_with_nested_random_and_distance_blocks() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn waypoint_selection_round_trips_without_reinterpreting_other_target_types() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn player_slots_round_trip_without_reinterpreting_other_target_groups() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
        assert!(result.source.contains("self.update_target_position();"));
    }

    #[test]
    fn relative_target_points_round_trip_and_preserve_other_encodings() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
        assert!(result.source.contains("self.update_target_position();"));
    }

    #[test]
    fn active_conditions_decompile_and_recompile_losslessly() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0x08, 0, 0x92, 0x08, 1, 0x4d, 0x08, 2, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("if self.active {"));
        assert!(!result.source.contains("native(0x08"));
    }

    #[test]
    fn angle_conditions_round_trip_every_native_boundary_and_reversed_bounds() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn zero_weight_random_branches_round_trip_at_every_position() {
        for weights in [[0, 16, 16], [16, 0, 16], [16, 16, 0]] {
            let mut image = Image::default();
            image.put(0x100, &[0; 60]);
            image.pointer(0x100, 0x200);
            image.pointer(0x200, 0x300);
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
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0xff, 0xf7]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("reset forget_target;"));
        assert!(!result.source.contains("native(0xff, 0xf7)"));
    }

    #[test]
    fn target_binding_methods_decompile_and_recompile_losslessly() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(0x300, &[0x11, 0x13, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("self.bind_awareness_target();"));
        assert!(result.source.contains("self.bind_current_target();"));
        assert!(!result.source.contains("native(0x11"));
        assert!(!result.source.contains("native(0x13"));
    }

    #[test]
    fn mode_is_round_trips_operands_nested_blocks_and_events() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn tracked_players_method_round_trips_with_optional_else() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(
            0x300,
            &[2, 0, 0x54, 0, 0x92, 0x54, 2, 2, 1, 4, 2, 2, 0xff, 0],
        );
        image.pointer(0x100 + EVENT_SLOTS[3].root_index as u32 * 4, 0x400);
        image.pointer(0x400, 0x500);
        image.put(0x500, &[2, 0, 0x92, 2, 2, 0xff, EVENT_SLOTS[3].ending]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(
            result
                .source
                .matches("if self.check_tracked_players() {")
                .count(),
            2
        );
        assert!(result.source.contains("if self.target.available {"));
        assert!(!result.source.contains("native(0x02"));
    }

    #[test]
    fn target_available_conditions_round_trip() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.put(
            0x300,
            &[
                0x54, 0, 0x39, 0, 0x92, 0x39, 2, 0x54, 1, 4, 0x54, 2, 0xff, 0,
            ],
        );
        image.pointer(0x100 + EVENT_SLOTS[3].root_index as u32 * 4, 0x400);
        image.pointer(0x400, 0x500);
        image.put(
            0x500,
            &[0x54, 0, 0x92, 0x54, 2, 0xff, EVENT_SLOTS[3].ending],
        );
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(
            result.source.matches("if self.target.available {").count(),
            2
        );
        assert!(result.source.contains("if self.flashed {"));
        assert!(!result.source.contains("native(0x54"));
    }

    #[test]
    fn mixed_rage_and_flash_conditions_round_trip() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn flashed_blocks_decompile_and_round_trip_in_states_and_events() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
                .contains("        if self.flashed {\n            reset;")
        );
        // Repeated native else markers cannot be expressed as an ordinary if.
        let mut source = String::new();
        format_body(&mut source, &[0x39, 0, 0x39, 1, 0x39, 1, 0x39, 2], None).unwrap();
        assert!(!source.contains("if self.flashed"));
        assert_eq!(source.matches("native(").count(), 4);
    }

    #[test]
    fn state_reset_tails_are_implicit_but_early_resets_remain_lossless() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.pointer(0x204, 0x400);
        image.put(0x300, &[0x39, 0, 7, 1, 0x39, 2, 0xff, 0]);
        image.put(0x400, &[0x92, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("fn main()"));
        assert!(result.source.contains("fn state_1() {\n    nop();\n}"));
        assert!(!result.source.contains("native(0xff, 0x00)"));

        image.put(0x300, &[0x39, 0, 0xff, 0, 0x39, 2, 0x92, 0xff, 0]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.source.matches("reset;").count(), 1);

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
            assert!(
                result
                    .source
                    .contains(&format!("{} => on_{};", event.name, event.name))
            );
        }
        assert!(!result.source.contains("native(0xff"));
        assert!(!result.source.contains("_end()"));

        // A different native ending must not be silently replaced.
        image.put(0x300, &[0x92, 0xff, 0xfd]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("native(0xff, 0xfd);"));

        // An early exit inside a conditional is retained; only the outer tail
        // can be reconstructed by the compiler.
        image.put(0x300, &[0x39, 0, 0xff, 0xf5, 0x39, 2, 0x92, 0xff, 0xf5]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert_eq!(result.source.matches("native(0xff, 0xf5);").count(), 1);

        image.put(0x300, &[0xff, 0xf5]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.source.contains("fn on_dung_reaction() {\n}"));
    }

    #[test]
    fn follows_sparse_cyclic_states_without_reading_a_guessed_table_length() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
        image.pointer(0x200 + 9 * 4, 0x400);
        image.put(0x300, &[5, 3, 6, 0, 7, 9]);
        image.put(0x400, &[0x99, 2, 4]);
        let result = decompile(&image, 0x100, 6, 0, None).unwrap();
        assert!(result.warnings.is_empty());
        assert!(result.source.contains("action[3:6](0);"));
        assert!(result.source.contains("transition state_9;"));
        assert!(result.source.contains("state_9 = 9"));
        assert!(result.source.contains("native(0x99, 0x02);"));
    }

    #[test]
    fn contents_calls_are_not_main_state_references() {
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
    fn returns_inside_raw_subscript_blocks_preserve_closing_markers() {
        for body in [
            // This condition has no structured DSL equivalent.
            vec![0x14, 0, 45, 0xff, 1, 0x14, 2, 0xff, 1],
            // Noncanonical weights keep this random block as native bytes.
            vec![0x80, 0, 1, 0x80, 1, 1, 0xff, 1, 0x80, 0xff, 0xff, 1],
        ] {
            let mut image = Image::default();
            image.put(0x100, &[0; 60]);
            image.pointer(0x100, 0x200);
            image.pointer(0x104, 0x800);
            image.pointer(0x200, 0x300);
            image.put(0x300, &[0x81, 1, 0xff, 0]);
            image.pointer(0x804, 0x400);
            image.put(0x400, &body);
            // decompile checks every recovered script against its original bytes.
            let result = decompile(&image, 0x100, 2, 0, None).unwrap();
            assert!(result.warnings.is_empty());
            assert!(result.source.contains("native(0xff, 0x01);"));
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
        let mut image = Image::default();
        image.put(0x100, &[0; 60]);
        image.pointer(0x100, 0x200);
        image.pointer(0x200, 0x300);
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
}
