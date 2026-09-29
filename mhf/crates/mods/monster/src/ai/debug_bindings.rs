//! Bind inspected source to existing native bytes without allocating or
//! publishing a replacement. Only declared table cells are read.

use std::collections::BTreeSet;

use super::{
    Error, MAX_NODES, MAX_PAYLOAD, Node, Program, Result,
    bind::{DESCRIPTOR_WORDS, ScriptBinding},
    decompile::{MAX_SCRIPT_BYTES, Memory},
};

/// Find script ranges whose declared bytes exactly match the corresponding
/// live table entry. Null, unreadable and differing entries remain unbound;
/// undeclared native entries are never inspected and native lengths are never
/// inferred. Each distinct `(node, address)` alias is returned once.
///
/// This compares the exact declared byte range, not an inferred native script
/// extent. A caller must retain the compiled source revision and invalidate
/// these bindings when the descriptor or source changes. Provisional automatic
/// slots cannot be matched until their native positions have been resolved.
pub fn matched_script_bindings(
    program: &Program,
    descriptor: u32,
    memory: &impl Memory,
) -> Result<Vec<ScriptBinding>> {
    program.validate_lossless()?;
    if !program.automatic_slots.is_empty() || !program.relocations.is_empty() {
        return Err(Error::new(
            "resolve automatic function slots before matching native scripts",
        ));
    }
    let Node::Table(root) = &program.nodes[program.root] else {
        unreachable!("program validation checked the root kind")
    };
    let mut bindings = Vec::new();
    let mut visited = BTreeSet::new();
    let mut compared = 0usize;
    let mut entries = 0usize;
    for (root_index, declared) in root.iter() {
        let Some(table_node) = declared else { continue };
        if root_index >= DESCRIPTOR_WORDS {
            return Err(Error::new(
                "declared root slot exceeds the native index range",
            ));
        }
        let Node::Table(table) = &program.nodes[table_node] else {
            return Err(Error::new(
                "declared root slots must reference pointer tables",
            ));
        };
        let Some(address) = pointer(memory, descriptor, root_index) else {
            continue;
        };
        for (index, declared) in table.iter() {
            let Some(node) = declared else { continue };
            if index > usize::from(u8::MAX) {
                return Err(Error::new(
                    "declared script index exceeds a native byte index",
                ));
            }
            entries += 1;
            if entries > MAX_NODES {
                return Err(Error::new(
                    "native source binding exceeds the table-read budget",
                ));
            }
            let Node::Script(bytes) = &program.nodes[node] else {
                return Err(Error::new(
                    "declared script cells must reference script nodes",
                ));
            };
            if bytes.len() > MAX_SCRIPT_BYTES {
                return Err(Error::new(
                    "native source binding exceeds the script-size limit",
                ));
            }
            if bytes.is_empty() {
                continue;
            }
            let Some(script) = pointer(memory, address, index) else {
                continue;
            };
            if !visited.insert((node, script)) {
                continue;
            }
            compared += bytes.len();
            if compared > MAX_PAYLOAD {
                return Err(Error::new(
                    "native source binding exceeds the byte-read budget",
                ));
            }
            if script.checked_add(bytes.len() as u32 - 1).is_none() {
                continue;
            }
            if memory
                .bytes(script, bytes.len())
                .is_ok_and(|live| live == *bytes)
            {
                bindings.push(ScriptBinding {
                    node,
                    address: script,
                    length: bytes.len(),
                });
            }
        }
    }
    Ok(bindings)
}

fn pointer(memory: &impl Memory, table: u32, index: usize) -> Option<u32> {
    if table == 0 || !table.is_multiple_of(4) {
        return None;
    }
    let offset = u32::try_from(index.checked_mul(4)?).ok()?;
    let address = table.checked_add(offset)?;
    address.checked_add(3)?;
    memory.word(address).ok().filter(|address| *address != 0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::{Base, Table};
    use std::{cell::RefCell, collections::BTreeMap};

    #[derive(Default)]
    struct Image {
        bytes: BTreeMap<u32, u8>,
        reads: RefCell<Vec<(u32, usize)>>,
    }

    impl Image {
        fn put(&mut self, address: u32, bytes: &[u8]) {
            self.bytes.extend(
                bytes
                    .iter()
                    .enumerate()
                    .map(|(offset, byte)| (address + offset as u32, *byte)),
            );
        }

        fn put_word(&mut self, address: u32, value: u32) {
            self.put(address, &value.to_le_bytes());
        }
    }

    impl Memory for Image {
        fn bytes(&self, address: u32, length: usize) -> Result<Vec<u8>> {
            self.reads.borrow_mut().push((address, length));
            (0..length)
                .map(|offset| {
                    self.bytes
                        .get(&(address + offset as u32))
                        .copied()
                        .ok_or_else(|| Error::new("unreadable image"))
                })
                .collect()
        }
    }

    fn program() -> Program {
        Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([
                    (0, Some(1)),
                    (4, Some(3)),
                    (9, Some(5)),
                    (14, None),
                ])),
                Node::Table(Table::from_entries([(0, Some(2)), (3, Some(2)), (7, None)])),
                Node::Script(vec![0x92, 0x04]),
                Node::Table(Table::from_entries([(0, Some(4))])),
                Node::Script(vec![0x92, 0xff, 0xfc]),
                Node::Table(Table::from_entries([(2, Some(6))])),
                Node::Script(vec![0x48, 2, 0xff, 3]),
            ],
            automatic_slots: vec![],
            relocations: vec![],
        }
    }

    fn image() -> Image {
        let mut memory = Image::default();
        memory.put_word(0x1000, 0x2000);
        memory.put_word(0x1010, 0x3000);
        memory.put_word(0x1024, 0x4000);
        memory.put_word(0x2000, 0x5001);
        memory.put_word(0x200c, 0x6003);
        memory.put_word(0x3000, 0x7000);
        memory.put_word(0x4008, 0x8000);
        memory.put(0x5001, &[0x92, 0x04]);
        memory.put(0x6003, &[0x92, 0x04]);
        memory.put(0x7000, &[0x92, 0xff, 0xfc]);
        memory.put(0x8000, &[0x48, 2, 0xff, 3]);
        memory
    }

    #[test]
    fn matches_main_event_and_explicit_subscript_without_reading_inherited_slots() {
        let memory = image();
        let matches = matched_script_bindings(&program(), 0x1000, &memory).unwrap();
        assert_eq!(
            matches,
            [
                ScriptBinding {
                    node: 2,
                    address: 0x5001,
                    length: 2
                },
                ScriptBinding {
                    node: 2,
                    address: 0x6003,
                    length: 2
                },
                ScriptBinding {
                    node: 4,
                    address: 0x7000,
                    length: 3
                },
                ScriptBinding {
                    node: 6,
                    address: 0x8000,
                    length: 4
                },
            ]
        );
        let reads = memory.reads.borrow();
        assert_eq!(reads.len(), 11);
        assert!(
            !reads
                .iter()
                .any(|(address, _)| matches!(*address, 0x1004 | 0x1038 | 0x201c))
        );
    }

    #[test]
    fn skips_differing_unreadable_and_null_live_scripts() {
        let mut memory = image();
        memory.put(0x5001, &[0x93]);
        memory.put_word(0x200c, 0);
        memory.bytes.remove(&0x7001);
        assert_eq!(
            matched_script_bindings(&program(), 0x1000, &memory).unwrap(),
            [ScriptBinding {
                node: 6,
                address: 0x8000,
                length: 4
            },]
        );
    }

    #[test]
    fn deduplicates_identical_aliases_but_keeps_distinct_source_nodes() {
        let mut program = program();
        let Node::Table(main) = &mut program.nodes[1] else {
            panic!()
        };
        main.insert(4, 4);
        program.nodes[4] = program.nodes[2].clone();
        let mut memory = image();
        memory.put_word(0x200c, 0x5001);
        memory.put_word(0x2010, 0x5001);
        memory.put_word(0x3000, 0x5001);
        let matches = matched_script_bindings(&program, 0x1000, &memory).unwrap();
        assert_eq!(
            matches
                .iter()
                .filter(|binding| binding.address == 0x5001)
                .map(|binding| binding.node)
                .collect::<Vec<_>>(),
            [2, 4]
        );
        assert_eq!(
            memory
                .reads
                .borrow()
                .iter()
                .filter(|(address, _)| *address == 0x5001)
                .count(),
            2
        );
    }

    #[test]
    fn empty_inherited_document_performs_no_reads_and_overflow_does_not_wrap() {
        let memory = Image::default();
        let mut program = program();
        program.nodes = vec![Node::Table(Table::new())];
        assert!(
            matched_script_bindings(&program, 0x1000, &memory)
                .unwrap()
                .is_empty()
        );
        assert!(memory.reads.borrow().is_empty());
        program.nodes = vec![
            Node::Table(Table::from_entries([(4, Some(1))])),
            Node::Table(Table::from_entries([(0, Some(2))])),
            Node::Script(vec![0x92]),
        ];
        assert!(
            matched_script_bindings(&program, 0xffff_fff0, &memory)
                .unwrap()
                .is_empty()
        );
        assert!(memory.reads.borrow().is_empty());
    }
}
