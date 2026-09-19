//! Bounded extraction of reachable state scripts and event entry scripts.
//!
//! Native tables have no lengths. Follow explicit state references instead of
//! guessing a table end from adjacent pointers. Undiscovered slots and the
//! contents/route tables remain inherited through `base native`.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

use super::{Error, Result, bytecode, control::EVENT_SLOTS};

pub const MAX_SCRIPT_BYTES: usize = 64 * 1024;
pub const MAX_SOURCE_BYTES: usize = 4 * 1024 * 1024;

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
    let mut pending = BTreeSet::from([0, current]);
    let mut visited = BTreeSet::new();
    let mut states = BTreeMap::new();
    let mut events = BTreeMap::new();
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
    while let Some(index) = pending.pop_first() {
        if !visited.insert(index) {
            continue;
        }
        let result = memory
            .word(indexed(table, usize::from(index))?)
            .and_then(|address| script(memory, address));
        match result {
            Ok(body) => {
                total += body.len();
                if total > super::MAX_PAYLOAD {
                    return Err(Error::new("AI extraction exceeds byte budget"));
                }
                references(&body, &mut pending);
                states.insert(index, body);
            }
            Err(error) => warnings.push(format!("状态 {index} 沿用原生：{error}")),
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
        "base native;\n\n// 部分导出：仅含已追踪状态和事件入口；未导出的表项沿用原生。\n",
    );
    for warning in &warnings {
        writeln!(source, "// {warning}").unwrap();
    }
    source.push_str("\nstates {\n");
    for &index in states.keys() {
        writeln!(source, "    state_{index} = {index} -> state_{index}_body;").unwrap();
    }
    source.push_str("}\n\nevents {\n");
    for index in events.keys() {
        let name = EVENT_SLOTS[*index].name;
        writeln!(source, "    {name} -> {name}_body;").unwrap();
    }
    source.push_str("}\n");
    for (&index, bytes) in &states {
        writeln!(source, "\nfn state_{index}_body() {{").unwrap();
        format_body(&mut source, bytes, Some(&states))?;
        source.push_str("}\n");
    }
    for (&index, bytes) in &events {
        let name = EVENT_SLOTS[index].name;
        writeln!(source, "\nfn {name}_body() {{").unwrap();
        // Only elide the matching, unconditional final instruction. Nested or
        // mismatched endings remain native escapes to preserve control flow.
        let instructions = bytecode::decode(bytes)?;
        let body = if instructions
            .last()
            .is_some_and(|instruction| instruction.bytes == [0xff, EVENT_SLOTS[index].ending])
        {
            &bytes[..bytes.len() - 2]
        } else {
            bytes.as_slice()
        };
        format_body(&mut source, body, None)?;
        source.push_str("}\n");
    }
    if source.len() > MAX_SOURCE_BYTES {
        return Err(Error::new("AI source exceeds size limit"));
    }
    // The exact bytes of every exported body must survive the existing compiler.
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
            super::Node::Script(actual) if actual == bytes
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
            super::Node::Script(actual) if actual == bytes
        ) {
            return Err(Error::new("event round-trip mismatch"));
        }
    }
    Ok(Decompiled { source, warnings })
}

fn references(bytes: &[u8], pending: &mut BTreeSet<u8>) {
    for instruction in bytecode::decode(bytes).expect("extracted instructions decode") {
        // 81 addresses root[1], not the main state table at root[0].
        if instruction.opcode == 0x07 {
            pending.insert(instruction.bytes[1]);
        }
    }
}

fn script(memory: &impl Memory, address: u32) -> Result<Vec<u8>> {
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
            || matches!(opcode, 0x04 | 0x07)
            || (opcode == 0xff && matches!(selector, Some(0..=3 | 0xf5..=0xff)));
        bytes.extend_from_slice(&instruction);
        if terminal && structure.is_closed() {
            return Ok(bytes);
        }
    }
    Err(Error::new("script has no proven end within 64 KiB"))
}

fn format_body(
    out: &mut String,
    bytes: &[u8],
    states: Option<&BTreeMap<u8, Vec<u8>>>,
) -> Result<()> {
    for instruction in bytecode::decode(bytes)? {
        let b = instruction.bytes;
        let statement = match b.as_slice() {
            [0x05, group, id, arg] => format!("action[{group}:{id}]({arg});"),
            [0x07, index] if states.is_some_and(|states| states.contains_key(index)) => {
                format!("transition state_{index};")
            }
            [0x04] if states.is_some() => "restart;".into(),
            [0x68] => "stop();".into(),
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
        writeln!(out, "    {statement}").unwrap();
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
                    .contains(&format!("{} -> {}_body;", event.name, event.name))
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
        assert!(result.source.contains("fn dung_reaction_body() {\n}"));
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
        assert!(result.warnings.is_empty());
        assert!(result.source.contains("fn state_0_body()"));
        assert!(!result.source.contains("state_200"));
        assert!(result.source.contains("native(0x81, 0xc8)"));
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
