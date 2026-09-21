//! Materialisation of a `base native;` declaration onto the live descriptor.
//!
//! The compiler never reads the client's tables: explicit slots are fixed,
//! automatic function slots are provisional, and other entries are inherited.
//! This module resolves automatic slots against the live tables before applying
//! the declaration, then fills caller-owned storage with a private descriptor
//! plus the tables and scripts that declaration owns.
//!
//! Only two windows are fixed by the client, and both come from the width of
//! the index the client uses:
//!
//! * the state table behind descriptor word 0 is indexed with a byte
//!   (`0x10860340`, `0x108697C6`), so a binding covers [`STATE_WORDS`] entries;
//! * the descriptor's sub-content tail is indexed with a byte operand
//!   (`0x10866F97` reads `[descriptor + 0x3C + index * 4]`), so a binding covers
//!   [`DESCRIPTOR_WORDS`] words.
//!
//! Everything else follows the declaration. A table the document does not write
//! is never copied: the private descriptor keeps pointing at the native table,
//! so a native reader of an unwritten slot sees exactly what it saw before.
//!
//! The client reads these blocks without a length field, so a block has to be at
//! least as long as the widest index the reader can form. How long a *native*
//! block really is belongs to its content — the inspected descriptors are 24
//! words, and reading past one returns the next block's words, which is what the
//! client would read on its own. Copying the full window therefore reproduces
//! the native reader's view instead of inventing a shorter one.

use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap},
};

use crate::ai::control::{EVENT_SLOTS, MAIN_ROOT_INDEX};
use crate::ai::{
    Base, Error, NATIVE_DESCRIPTOR_SLOTS, NativeSlot, Node, Program, Result, Table, bytecode,
};

/// Words read from the live descriptor.
///
/// Fifteen fixed slots plus every tail index a byte operand can name.
pub const DESCRIPTOR_WORDS: usize = NATIVE_DESCRIPTOR_SLOTS + 256;

/// Words read from the live state table.
///
/// Both readers of descriptor word 0 widen a byte before indexing it.
pub const STATE_WORDS: usize = 256;

/// Read-only access to the memory the binding runs against.
pub trait NativeMemory {
    /// Read `words` little-endian words starting at `address`.
    ///
    /// A live implementation runs inside the game, so `address` is trusted to
    /// come from the actor's own descriptor. Implementations still have to
    /// reject a null or unaligned address instead of dereferencing it.
    fn read(&self, address: u32, words: usize) -> Result<Vec<u32>>;
}

/// Storage the caller keeps alive for as long as the game can read the binding.
pub trait Arena {
    /// Reserve `words` words and report the address the game will dereference.
    ///
    /// One allocation must not move afterwards, because the descriptor the game
    /// holds points into it.
    fn allocate(&mut self, words: usize) -> Result<u32>;

    /// Write one block. `words` must be the block [`Self::allocate`] reserved.
    fn write(&mut self, address: u32, words: &[u32]) -> Result<()>;
}

/// Where a materialised binding put its descriptor and state table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Overlay {
    /// Address to store in the actor's descriptor field.
    pub descriptor: u32,
    /// Address of the state table the descriptor's word 0 holds. It is the
    /// private copy when the document declares states, and the native table
    /// otherwise.
    pub state_table: u32,
}

/// Overlay a `base native;` declaration onto the live descriptor at `address`.
///
/// The declaration's own scripts, cells and state table go into `arena`; every
/// entry it does not write is copied from live memory, so a native table stays
/// native. `address` is not modified.
pub fn materialize(
    program: &Program,
    address: u32,
    memory: &impl NativeMemory,
    arena: &mut impl Arena,
) -> Result<Overlay> {
    if address == 0 {
        return Err(Error::new(
            "the actor has no live descriptor to bind a `base native;` document onto",
        ));
    }
    if program.base != Base::Native {
        return Err(Error::new(
            "this program was compiled over an empty base: it declares every slot itself and has no live block to inherit from",
        ));
    }
    program.validate_lossless()?;
    // Validate before allocating or publishing anything. DSL native(...) only
    // guarantees byte preservation; it does not prove native scans terminate.
    for (index, node) in program.nodes.iter().enumerate() {
        if let Node::Script(bytes) = node {
            super::bytecode::validate_structure(bytes)
                .map_err(|error| Error::new(format!("script node {index}: {error}")))?;
        }
    }
    let words = read_exact(memory, address, DESCRIPTOR_WORDS, "descriptor")?;
    let resolved = resolve_automatic(program, &words, memory)?;
    let program = resolved.as_ref();
    let Node::Table(root) = &program.nodes[program.root] else {
        unreachable!("validate_lossless checked the root kind")
    };

    let mut blocks = Blocks::default();
    let descriptor = blocks.push(words, Vec::new());
    let mut scripts = HashMap::new();

    if root.declares(MAIN_ROOT_INDEX) {
        let node = root.get(MAIN_ROOT_INDEX).ok_or_else(|| {
            Error::new(format!(
                "descriptor slot {MAIN_ROOT_INDEX} cannot be cleared: the selector reads the state table through it"
            ))
        })?;
        let Node::Table(declaration) = &program.nodes[node] else {
            return Err(Error::new("root[0] must reference a table"));
        };
        let native = blocks.words()[descriptor][MAIN_ROOT_INDEX];
        if native == 0 {
            return Err(Error::new(
                "the live descriptor has no state table, so declared states have nothing to overlay",
            ));
        }
        let mut words = read_exact(memory, native, STATE_WORDS, "state table")?;
        let entries = layer(
            program,
            declaration,
            &mut words,
            "state index",
            "state table",
        )?;
        let block = blocks.push(words, Vec::new());
        for (slot, node) in entries {
            let target = script(&mut blocks, &mut scripts, program, node)?;
            blocks.link(block, slot, target);
        }
        blocks.link(descriptor, MAIN_ROOT_INDEX, block);
    }

    for slot in EVENT_SLOTS {
        if !root.declares(slot.root_index) {
            continue;
        }
        let native = blocks.words()[descriptor][slot.root_index];
        let Some(node) = root.get(slot.root_index) else {
            blocks.set(descriptor, slot.root_index, 0);
            continue;
        };
        let Node::Table(declaration) = &program.nodes[node] else {
            return Err(Error::new(format!(
                "descriptor event slot {:#04x} must reference a pointer cell table",
                slot.mask
            )));
        };
        // Event dispatch reads cell[0]; route_ptr_set uses a separate root[2].
        let window = declaration.extent().max(1);
        let mut words = if native == 0 {
            vec![0; window]
        } else {
            read_exact(memory, native, window, "event cell")?
        };
        let entries = layer(program, declaration, &mut words, "cell index", "event cell")?;
        let block = blocks.push(words, Vec::new());
        for (slot, node) in entries {
            let target = script(&mut blocks, &mut scripts, program, node)?;
            blocks.link(block, slot, target);
        }
        blocks.link(descriptor, slot.root_index, block);
    }

    for (index, node) in root.iter() {
        if matches!(index, 1 | 9) || (15..DESCRIPTOR_WORDS).contains(&index) {
            let node = node.ok_or_else(|| Error::new("cannot clear a subscript table"))?;
            let Node::Table(declaration) = &program.nodes[node] else {
                return Err(Error::new("subscript binding must reference a table"));
            };
            let native = blocks.words()[descriptor][index];
            let mut words = if native == 0 {
                vec![0; 256]
            } else {
                read_exact(memory, native, 256, "subscript table")?
            };
            let entries = layer(
                program,
                declaration,
                &mut words,
                "subscript index",
                "subscript table",
            )?;
            let block = blocks.push(words, Vec::new());
            for (slot, node) in entries {
                let target = script(&mut blocks, &mut scripts, program, node)?;
                blocks.link(block, slot, target);
            }
            blocks.link(descriptor, index, block);
            continue;
        }
        let known =
            index == MAIN_ROOT_INDEX || EVENT_SLOTS.iter().any(|slot| slot.root_index == index);
        if !known {
            return Err(Error::new(format!(
                "descriptor slot {index} has no binding window: only state, event and subscript tables can be overlaid"
            )));
        }
    }

    let addresses = blocks
        .words()
        .iter()
        .map(|words| arena.allocate(words.len()))
        .collect::<Result<Vec<_>>>()?;
    let mut words = blocks.words().to_vec();
    for (block, links) in words.iter_mut().zip(blocks.links()) {
        for &(slot, target) in links {
            block[slot] = addresses[target];
        }
    }
    for (address, block) in addresses.iter().zip(&words) {
        arena.write(*address, block)?;
    }
    Ok(Overlay {
        descriptor: addresses[descriptor],
        state_table: words[descriptor][MAIN_ROOT_INDEX],
    })
}

/// Resolve provisional slots against the actual native tables before any arena
/// allocation. Nonzero native entries, explicit declarations and literal calls
/// are all reserved. Only call sites emitted by the compiler are rewritten.
fn resolve_automatic<'a>(
    program: &'a Program,
    descriptor: &[u32],
    memory: &impl NativeMemory,
) -> Result<Cow<'a, Program>> {
    if program.automatic_slots.is_empty() {
        return Ok(Cow::Borrowed(program));
    }
    let automatic: BTreeSet<_> = program.automatic_slots.iter().copied().collect();
    let generated_calls: BTreeSet<_> = program
        .relocations
        .iter()
        .map(|site| (site.script, site.offset))
        .collect();
    let mut reserved = BTreeSet::new();
    for (script, node) in program.nodes.iter().enumerate() {
        let Node::Script(bytes) = node else {
            continue;
        };
        for instruction in bytecode::decode(bytes)? {
            if !generated_calls.contains(&(script, instruction.offset))
                && let Some(slot) = NativeSlot::from_call(&instruction.bytes)
            {
                reserved.insert(slot);
            }
        }
    }
    let Node::Table(root) = &program.nodes[program.root] else {
        unreachable!()
    };
    let mut resolved = program.clone();
    let mut relocated_slots = BTreeMap::new();
    let tables: BTreeSet<_> = automatic.iter().map(|slot| slot.table).collect();
    for table_index in tables {
        let node = root.get(table_index).expect("validated automatic table");
        let Node::Table(table) = &program.nodes[node] else {
            unreachable!()
        };
        let address = descriptor[table_index];
        let native = if address == 0 {
            vec![0; 256]
        } else {
            read_exact(memory, address, 256, "automatic subscript table")?
        };
        let mut entries = Table::new();
        for (index, script) in table.iter() {
            if index > 255 {
                return Err(Error::new("subscript index exceeds 255"));
            }
            let slot = NativeSlot {
                table: table_index,
                index: index as u8,
            };
            if !automatic.contains(&slot) {
                reserved.insert(slot);
                match script {
                    Some(script) => entries.insert(index, script),
                    None => entries.clear(index),
                }
            }
        }
        for &slot in automatic.iter().filter(|slot| slot.table == table_index) {
            let target = std::iter::once(slot.index)
                .chain(0..=u8::MAX)
                .map(|index| NativeSlot {
                    table: table_index,
                    index,
                })
                .find(|target| native[usize::from(target.index)] == 0 && !reserved.contains(target))
                .ok_or_else(|| {
                    Error::new(format!(
                        "native subscript table {table_index} has no empty slot for automatic functions; specify @slot to replace a known entry"
                    ))
                })?;
            reserved.insert(target);
            relocated_slots.insert(slot, target);
            entries.insert(
                usize::from(target.index),
                table
                    .get(usize::from(slot.index))
                    .expect("validated automatic script"),
            );
        }
        resolved.nodes[node] = Node::Table(entries);
    }
    for site in &program.relocations {
        let Node::Script(bytes) = &mut resolved.nodes[site.script] else {
            unreachable!()
        };
        let call = relocated_slots[&site.target].call();
        bytes[site.offset..site.offset + call.len()].copy_from_slice(&call);
    }
    resolved.automatic_slots.clear();
    resolved.relocations.clear();
    Ok(Cow::Owned(resolved))
}

/// Overlay one declaration's entries onto a window read from the live block.
///
/// An index the declaration writes is decided by the declaration; every other
/// index keeps the word that was already in `words`. The returned pairs are the
/// declared script entries, whose blocks are created after the block itself so
/// that allocation order stays tables first, scripts last.
fn layer(
    program: &Program,
    declaration: &Table,
    words: &mut [u32],
    what: &str,
    table: &str,
) -> Result<Vec<(usize, usize)>> {
    let mut entries = Vec::new();
    for (index, entry) in declaration.iter() {
        if index >= words.len() {
            return Err(Error::new(format!(
                "{what} {index} is outside the {}-word window the client indexes in the live {table}",
                words.len()
            )));
        }
        match entry {
            Some(node) => {
                if !matches!(program.nodes.get(node), Some(Node::Script(_))) {
                    return Err(Error::new(format!("node {node} is not a script")));
                }
                entries.push((index, node));
            }
            // A bare entry clears the slot it names.
            None => words[index] = 0,
        }
    }
    Ok(entries)
}

/// Copy one declared script into its own block.
fn script(
    blocks: &mut Blocks,
    scripts: &mut HashMap<usize, usize>,
    program: &Program,
    node: usize,
) -> Result<usize> {
    if let Some(&block) = scripts.get(&node) {
        return Ok(block);
    }
    let Node::Script(bytes) = program
        .nodes
        .get(node)
        .ok_or_else(|| Error::new(format!("script node {node} is missing")))?
    else {
        return Err(Error::new(format!("node {node} is not a script")));
    };
    let mut guarded = bytes.clone();
    // A body that does not end on a terminator would otherwise run into the
    // next block. `0x00` reaches the interpreter's switch default, which stops
    // the script; native blocks are padded with the same byte.
    guarded.resize(align4(guarded.len() + 1), 0);
    let block = blocks.push(pack(&guarded), Vec::new());
    scripts.insert(node, block);
    Ok(block)
}

/// Read the window behind an `act`-indexed route table.
///
/// A descriptor with no route table yet still has to produce a full window: the
/// declaration may be the first thing that gives that slot a script, and the
/// client can index it with any act byte afterwards.
fn read_exact(
    memory: &impl NativeMemory,
    address: u32,
    words: usize,
    what: &str,
) -> Result<Vec<u32>> {
    let read = memory.read(address, words)?;
    if read.len() != words {
        return Err(Error::new(format!(
            "live {what} at {address:#010x} returned {} words; the client reads {words}",
            read.len()
        )));
    }
    Ok(read)
}

fn align4(value: usize) -> usize {
    (value + 3) & !3
}

fn pack(bytes: &[u8]) -> Vec<u32> {
    let mut words = vec![0u32; align4(bytes.len()) / 4];
    for (index, byte) in bytes.iter().enumerate() {
        words[index / 4] |= u32::from(*byte) << (8 * (index % 4));
    }
    words
}

/// Blocks in the order the arena has to allocate them.
#[derive(Default)]
struct Blocks {
    words: Vec<Vec<u32>>,
    links: Vec<Vec<(usize, usize)>>,
}

impl Blocks {
    fn push(&mut self, words: Vec<u32>, links: Vec<(usize, usize)>) -> usize {
        self.words.push(words);
        self.links.push(links);
        self.words.len() - 1
    }

    fn words(&self) -> &[Vec<u32>] {
        &self.words
    }

    fn links(&self) -> &[Vec<(usize, usize)>] {
        &self.links
    }

    /// Remember that `slot` of `block` holds the address of `target`.
    fn link(&mut self, block: usize, slot: usize, target: usize) {
        self.links[block].push((slot, target));
    }

    fn set(&mut self, block: usize, slot: usize, value: u32) {
        self.words[block][slot] = value;
    }
}

#[cfg(test)]
mod tests {
    use super::{DESCRIPTOR_WORDS, NativeMemory, STATE_WORDS, materialize};
    use crate::ai::control::{EVENT_SLOTS, ROUTE_ROOT_INDEX};
    use crate::ai::{Base, Error, Node, Program, Result, Table};
    use std::collections::HashMap;

    const DESCRIPTOR: u32 = 0x0100_0000;
    const STATE_TABLE: u32 = 0x0100_1000;
    const FIRST_CELL: u32 = 0x0100_2000;
    const ROUTE_TABLE: u32 = 0x0100_4000;
    const NATIVE_SCRIPTS: [u32; 3] = [0x0100_3000, 0x0100_3100, 0x0100_3200];
    /// A route slot the document does not name, so it has to survive untouched.
    const NATIVE_ROUTE_ACT: usize = 9;

    /// A native address space with one descriptor, its state table and one event
    /// cell for the `0x40` slot.
    struct Memory(HashMap<u32, Vec<u32>>);

    impl Memory {
        fn live() -> Self {
            let mut memory = HashMap::new();
            let mut descriptor = vec![0u32; DESCRIPTOR_WORDS];
            descriptor[0] = STATE_TABLE;
            descriptor[EVENT_SLOTS[0].root_index] = FIRST_CELL;
            descriptor[ROUTE_ROOT_INDEX] = ROUTE_TABLE;
            // The tail of a real block holds per-content tables.
            descriptor[crate::ai::NATIVE_DESCRIPTOR_SLOTS] = 0x0100_5000;
            memory.insert(DESCRIPTOR, descriptor);
            let mut state = vec![0u32; STATE_WORDS];
            state[..NATIVE_SCRIPTS.len()].copy_from_slice(&NATIVE_SCRIPTS);
            memory.insert(STATE_TABLE, state);
            memory.insert(FIRST_CELL, vec![NATIVE_SCRIPTS[2]]);
            let mut route = vec![0u32; 256];
            route[0] = NATIVE_SCRIPTS[2];
            route[NATIVE_ROUTE_ACT] = NATIVE_SCRIPTS[2];
            memory.insert(ROUTE_TABLE, route);
            Self(memory)
        }
    }

    impl NativeMemory for Memory {
        fn read(&self, address: u32, words: usize) -> Result<Vec<u32>> {
            let block = self.0.get(&address).ok_or_else(|| {
                Error::new(format!("test memory has no block at {address:#010x}"))
            })?;
            if block.len() < words {
                return Err(Error::new(format!(
                    "test block at {address:#010x} has {} words, not {words}",
                    block.len()
                )));
            }
            Ok(block[..words].to_vec())
        }
    }

    #[test]
    fn overlays_subscript_slots_without_changing_inherited_entries() {
        let mut memory = Memory::live();
        let address = 0x0100_6000;
        memory.0.get_mut(&DESCRIPTOR).unwrap()[1] = address;
        memory.0.insert(address, vec![NATIVE_SCRIPTS[1]; 256]);
        memory.0.insert(0x0100_5000, vec![NATIVE_SCRIPTS[2]; 256]);
        memory.0.get_mut(&DESCRIPTOR).unwrap()[9] = 0x0100_7000;
        memory.0.insert(0x0100_7000, vec![NATIVE_SCRIPTS[0]; 256]);
        let program = crate::ai::dsl::parse("mhf_ai 1; species 6; base native; fn main() { outer(); } @slot(table = 1, index = 200) fn outer() { inner(); } @slot(table = 15, index = 7) fn inner() { special(); } @slot(table = 9, index = 9) fn special() { nop(); }")
            .unwrap().compile().unwrap().program;
        let mut arena = TestArena::default();
        materialize(&program, DESCRIPTOR, &memory, &mut arena).unwrap();
        for (root_slot, changed, original) in [
            (1, 200, NATIVE_SCRIPTS[1]),
            (15, 7, NATIVE_SCRIPTS[2]),
            (9, 9, NATIVE_SCRIPTS[0]),
        ] {
            let table_address = arena.block(0)[root_slot];
            let table = &arena
                .blocks
                .iter()
                .find(|(address, _)| *address == table_address)
                .unwrap()
                .1;
            assert_eq!(table.len(), 256);
            for (index, &value) in table.iter().enumerate() {
                if index == changed {
                    assert_ne!(value, original);
                } else {
                    assert_eq!(value, original);
                }
            }
        }
        let before = arena.blocks.len();
        memory.0.remove(&address);
        assert!(materialize(&program, DESCRIPTOR, &memory, &mut arena).is_err());
        assert_eq!(arena.blocks.len(), before);
    }

    #[test]
    fn automatic_slots_relocate_only_generated_calls_and_preserve_native_tables() {
        let mut memory = Memory::live();
        let primary = 0x0100_6000;
        let special = 0x0100_7000;
        memory.0.get_mut(&DESCRIPTOR).unwrap()[1] = primary;
        memory.0.get_mut(&DESCRIPTOR).unwrap()[9] = special;
        for (address, free) in [
            (primary, vec![0, 3, 5, 200]),
            (0x0100_5000, vec![7]),
            (special, vec![42]),
        ] {
            let mut words = vec![NATIVE_SCRIPTS[1]; 256];
            for index in free {
                words[index] = 0;
            }
            memory.0.insert(address, words);
        }
        let before = memory.0.clone();
        let program = crate::ai::dsl::parse(
            "mhf_ai 1; species 6; base native;
            fn main() { native(0x81, 0); first(); }
            @slot(table = 1, index = 3) fn fixed() {}
            fn first() { second(); }
            fn second() { third(); }
            fn third() { nop(); }
            fn unused() { wait(4); }",
        )
        .unwrap()
        .compile()
        .unwrap()
        .program;
        let mut arena = TestArena::default();
        materialize(&program, DESCRIPTOR, &memory, &mut arena).unwrap();
        let at = |address| {
            arena
                .blocks
                .iter()
                .find(|(a, _)| *a == address)
                .unwrap()
                .1
                .as_slice()
        };
        let descriptor = arena.block(0);
        let assert_script = |address, bytes: &[u8]| {
            // Materialized scripts include a zero guard after the bytecode.
            let guarded = [bytes, &[0]].concat();
            assert_eq!(at(address), super::pack(&guarded));
        };
        let states = at(descriptor[0]);
        assert_script(states[0], &[0x81, 0, 0x81, 5, 0xff, 0]);
        let primary = at(descriptor[1]);
        assert_eq!(primary[0], 0); // Literal native call is not rebound to the new function.
        assert_script(primary[3], &[0xff, 1]);
        assert_script(primary[5], &[0x82, 0, 7, 0xff, 1]);
        assert_script(primary[200], &[0x48, 4, 0xff, 1]);
        assert_script(at(descriptor[15])[7], &[0x16, 42, 0xff, 2]);
        assert_script(at(descriptor[9])[42], &[0x92, 0xff, 3]);
        for (table, changed) in [(1, vec![0, 3, 5, 200]), (15, vec![7]), (9, vec![42])] {
            for (index, &value) in at(descriptor[table]).iter().enumerate() {
                if !changed.contains(&index) {
                    assert_eq!(value, NATIVE_SCRIPTS[1]);
                }
            }
        }
        assert_eq!(memory.0, before);
    }

    #[test]
    fn automatic_slot_exhaustion_or_unreadable_tables_fail_before_allocation() {
        let program = crate::ai::dsl::parse(
            "mhf_ai 1; species 6; base native; fn main() { helper(); } fn helper() {}",
        )
        .unwrap()
        .compile()
        .unwrap()
        .program;
        let mut memory = Memory::live();
        let address = 0x0100_6000;
        memory.0.get_mut(&DESCRIPTOR).unwrap()[1] = address;
        memory.0.insert(address, vec![NATIVE_SCRIPTS[1]; 256]);
        let mut arena = TestArena::default();
        assert!(
            materialize(&program, DESCRIPTOR, &memory, &mut arena)
                .unwrap_err()
                .to_string()
                .contains("no empty slot")
        );
        assert!(arena.blocks.is_empty());
        memory.0.remove(&address);
        assert!(materialize(&program, DESCRIPTOR, &memory, &mut arena).is_err());
        assert!(arena.blocks.is_empty());
    }

    #[test]
    fn automatic_slots_can_create_missing_native_tables() {
        let program = crate::ai::dsl::parse(
            "mhf_ai 1; species 6; base native; fn main() { helper(); } fn helper() {}",
        )
        .unwrap()
        .compile()
        .unwrap()
        .program;
        let memory = Memory::live();
        let mut arena = TestArena::default();
        materialize(&program, DESCRIPTOR, &memory, &mut arena).unwrap();
        let address = arena.block(0)[1];
        let table = &arena.blocks.iter().find(|(a, _)| *a == address).unwrap().1;
        assert_ne!(table[0], 0);
        assert!(table[1..].iter().all(|&value| value == 0));
    }

    /// Handing out one fixed address per allocation, so a test can name them.
    #[derive(Default)]
    struct TestArena {
        blocks: Vec<(u32, Vec<u32>)>,
    }

    impl TestArena {
        fn address(&self, index: usize) -> u32 {
            0x0200_0000 + (index as u32) * 0x1000
        }

        fn block(&self, index: usize) -> &[u32] {
            &self.blocks[index].1
        }
    }

    impl super::Arena for TestArena {
        fn allocate(&mut self, words: usize) -> Result<u32> {
            let address = self.address(self.blocks.len());
            self.blocks.push((address, vec![0; words]));
            Ok(address)
        }

        fn write(&mut self, address: u32, words: &[u32]) -> Result<()> {
            let block = self
                .blocks
                .iter_mut()
                .find(|(candidate, _)| *candidate == address)
                .ok_or_else(|| Error::new("write to an address the arena never allocated"))?;
            block.1.copy_from_slice(words);
            Ok(())
        }
    }

    /// `states { 0 { ... } }` and one event body on the `0x40` slot.
    fn declaration() -> Program {
        Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([
                    (0, Some(1)),
                    (EVENT_SLOTS[0].root_index, Some(3)),
                ])),
                Node::Table(Table::from_entries([(0, Some(2))])),
                Node::Script(vec![0x05, 3, 6, 0]),
                Node::Table(Table::from_entries([(0, Some(4))])),
                Node::Script(vec![0xff, 0xfd]),
            ],
            automatic_slots: Vec::new(),
            relocations: Vec::new(),
        }
    }

    /// Block 0 is the descriptor, 1 the state table, 2 the state script,
    /// 3 the event cell and 4 the event script.
    #[test]
    fn overlays_declarations_and_inherits_every_other_word() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let overlay = materialize(&declaration(), DESCRIPTOR, &memory, &mut arena).unwrap();

        assert_eq!(arena.blocks.len(), 5);
        assert_eq!(overlay.descriptor, arena.address(0));
        assert_eq!(overlay.state_table, arena.address(1));

        let descriptor = arena.block(0);
        assert_eq!(descriptor[0], arena.address(1));
        assert_eq!(descriptor[EVENT_SLOTS[0].root_index], arena.address(3));
        // The per-content tail is inherited, not rebuilt.
        assert_eq!(descriptor[crate::ai::NATIVE_DESCRIPTOR_SLOTS], 0x0100_5000);

        let state = arena.block(1);
        assert_eq!(state[0], arena.address(2));
        // State 1 keeps the native script; the declaration wrote only index 0.
        assert_eq!(state[1], NATIVE_SCRIPTS[1]);
        assert_eq!(state[NATIVE_SCRIPTS.len() - 1], NATIVE_SCRIPTS[2]);
        assert_eq!(state[STATE_WORDS - 1], 0);

        // A script keeps its bytes and gains a stop byte the interpreter reads
        // when a body runs off its end.
        assert_eq!(arena.block(2)[0], 0x0006_0305);
        assert_eq!(arena.block(2)[1], 0);
        assert_eq!(arena.block(4)[0], 0x0000_fdff);

        let cell = arena.block(3);
        assert_eq!(cell[0], arena.address(4));
    }

    /// The dispatcher reads `cell[0]`; a body therefore needs no native tail.
    #[test]
    fn an_event_body_writes_a_cell_whose_window_is_the_declaration() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        materialize(&declaration(), DESCRIPTOR, &memory, &mut arena).unwrap();
        assert_eq!(arena.block(3).len(), 1);
    }

    #[test]
    fn a_document_without_states_keeps_the_native_state_table() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let program = Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![Node::Table(Table::from_entries([(
                EVENT_SLOTS[0].root_index,
                Some(1),
            )]))],
            automatic_slots: Vec::new(),
            relocations: Vec::new(),
        };
        let mut nodes = program.nodes;
        nodes.push(Node::Table(Table::from_entries([(0, Some(2))])));
        nodes.push(Node::Script(vec![0x92]));
        let program = Program { nodes, ..program };

        let overlay = materialize(&program, DESCRIPTOR, &memory, &mut arena).unwrap();
        assert_eq!(overlay.state_table, STATE_TABLE);
        assert_eq!(arena.block(0)[0], STATE_TABLE);
    }

    #[test]
    fn a_bare_event_entry_clears_the_native_slot() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let program = Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![Node::Table(Table::from_entries([(
                EVENT_SLOTS[0].root_index,
                None,
            )]))],
            automatic_slots: Vec::new(),
            relocations: Vec::new(),
        };
        materialize(&program, DESCRIPTOR, &memory, &mut arena).unwrap();
        assert_eq!(arena.block(0)[EVENT_SLOTS[0].root_index], 0);
        assert_eq!(arena.blocks.len(), 1);
    }

    /// The mask-`0x02` slot is also the `act`-indexed route table, so a declared
    /// body replaces `cell[0]` and every other act keeps its native script.
    #[test]
    fn event_slot_six_does_not_replace_the_route_table() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let compiled = crate::ai::dsl::parse(
            "mhf_ai 1; species 6; base native; events { bait_detected => handler; } fn handler() { nop(); }"
        ).unwrap().compile().unwrap();
        materialize(&compiled.program, DESCRIPTOR, &memory, &mut arena).unwrap();
        assert_eq!(arena.block(0)[ROUTE_ROOT_INDEX], ROUTE_TABLE);
        assert_eq!(arena.block(0)[EVENT_SLOTS[6].root_index], arena.address(1));
        assert_eq!(arena.block(1).len(), 1);
        assert_eq!(arena.block(1)[0], arena.address(2));
    }

    /// A slot with no known reader window cannot be merged yet, and guessing one
    /// would rewrite data the client reads with a different index width.
    #[test]
    fn refuses_a_descriptor_slot_without_a_binding_window() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let program = Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([(6, Some(1))])),
                Node::Table(Table::from_entries([(0, Some(2))])),
                Node::Script(vec![0x92]),
            ],
            automatic_slots: Vec::new(),
            relocations: Vec::new(),
        };
        let error = materialize(&program, DESCRIPTOR, &memory, &mut arena)
            .unwrap_err()
            .to_string();
        assert!(error.contains("descriptor slot 6"), "{error}");
    }

    #[test]
    fn refuses_a_document_compiled_over_an_empty_base() {
        let memory = Memory::live();
        let mut arena = TestArena::default();
        let program = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![Node::Table(Table::from_entries([(0, Some(1))]))],
            automatic_slots: Vec::new(),
            relocations: Vec::new(),
        };
        let error = materialize(&program, DESCRIPTOR, &memory, &mut arena)
            .unwrap_err()
            .to_string();
        assert!(error.contains("empty base"), "{error}");
    }

    #[test]
    fn rejects_old_truncated_dsl_before_allocating_or_publishing() {
        let compiled = crate::ai::dsl::parse(
            "mhf_ai 1; species 6; base native; states { idle { native(0x39, 0); native(0xff, 0); } }"
        ).unwrap().compile().unwrap();
        let mut arena = TestArena::default();
        let error =
            materialize(&compiled.program, DESCRIPTOR, &Memory::live(), &mut arena).unwrap_err();
        assert!(error.to_string().contains("未闭合"));
        assert!(arena.blocks.is_empty());
    }
}
