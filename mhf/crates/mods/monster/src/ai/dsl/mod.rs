//! Monster-AI source projects: parsing, scoped imports, function expansion,
//! native bytecode emission and pointer-free graph assembly.
//!
//! Project::load resolves map/common entry files and preserves editable sources.
//! Project::compile resolves module names before Document::compile emits entries.
//! The binder publishes the graph only after all compilation and validation pass.
//! See docs/dsl-spec.md for syntax, automatic endings and current limitations.

mod compile;
pub(crate) mod condition;
mod lexer;
pub(crate) mod parser;
mod project;
#[cfg(test)]
mod project_tests;
pub(crate) mod slot;
pub(crate) mod target;

pub use parser::parse;
pub use project::{Project, SourceFile};

// A `.mhai` file is loaded through `parse` and `compile` alone; the game build
// never names the AST or the compiler's result type, so re-exporting them there
// would only be an unused import.  They are still part of the API the crate
// offers everywhere else.
#[cfg(not(all(feature = "provider", windows, target_arch = "x86")))]
pub use self::{compile::Compiled, parser::Document};

use crate::ai::control::EVENT_SLOTS;

/// Text version accepted by [`parse`]. The header exists so a future change can
/// fail loudly instead of being read as the current grammar.
pub const VERSION: u32 = 1;

/// Number of native event slots an `events` block can address (spec §9).
pub const EVENT_SLOT_COUNT: usize = EVENT_SLOTS.len();

// The suite drives the whole module: text -> `Document` -> `Program`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ai::control::EVENT_SLOTS;
    use crate::ai::dsl::parser::StateDecl;
    use crate::ai::{Base, Node, Program, Table};

    /// Legacy inline syntax remains byte-preserving for existing drafts.
    const SPEC_EXAMPLE: &str = "\
mhf_ai 1;
species 6;

actions {
    slash = [3:6];
    bash  = [4:1];
}

events {
    dung_reaction {
        slash(0);
        native(0xff, 0xfd);
    }

    player_detected {
        bash(1);
    }
}

states {
    idle = 0 {
        slash(0);
        transition combat;
    }

    combat {
        bash(1);
        transition idle;
    }
}
";

    fn script_at(program: &Program, index: usize) -> &[u8] {
        match &program.nodes[index] {
            Node::Script(bytes) => bytes,
            other => panic!("node {index} is not a script: {other:?}"),
        }
    }

    /// Node index of the first script node holding exactly `bytes`.
    fn script_node(program: &Program, bytes: &[u8]) -> usize {
        program
            .nodes
            .iter()
            .position(|node| matches!(node, Node::Script(script) if script.as_slice() == bytes))
            .unwrap_or_else(|| panic!("no script node holds {bytes:02x?}"))
    }

    fn root_table(program: &Program) -> &Table {
        match &program.nodes[program.root] {
            Node::Table(table) => table,
            other => panic!("root is not a table: {other:?}"),
        }
    }

    fn main_table(program: &Program) -> &Table {
        let main = root_table(program)
            .get(0)
            .expect("root[0] is the state table");
        match &program.nodes[main] {
            Node::Table(table) => table,
            other => panic!("main is not a table: {other:?}"),
        }
    }

    /// `root[slot] -> cell -> cell[0]`, the shape the event selector dereferences.
    fn event_script(program: &Program, slot: usize) -> Option<&[u8]> {
        let cell = root_table(program).get(slot)?;
        let Node::Table(cell) = &program.nodes[cell] else {
            panic!("descriptor slot {slot} is not a pointer cell");
        };
        cell.get(0).map(|node| script_at(program, node))
    }

    /// Parse a document and compile it, returning the error text of whichever
    /// stage refuses the source.
    fn failure(source: &str) -> String {
        let document = match parse(source) {
            Ok(document) => document,
            Err(error) => return error.to_string(),
        };
        match document.compile() {
            Ok(compiled) => panic!("source was accepted: {compiled:?}"),
            Err(error) => error.to_string(),
        }
    }

    #[test]
    fn legacy_inline_example_preserves_declared_bytes() {
        let document = parse(SPEC_EXAMPLE).unwrap();
        assert_eq!(document.version, VERSION);
        assert_eq!(document.species, 6);
        assert_eq!(document.base, Base::Empty);
        assert_eq!(
            document
                .actions
                .iter()
                .map(|decl| (decl.name.as_str(), decl.group, decl.id))
                .collect::<Vec<_>>(),
            [("slash", 3, 6), ("bash", 4, 1)]
        );
        assert_eq!(
            document
                .events
                .iter()
                .map(|decl| (EVENT_SLOTS[usize::from(decl.slot)].name, decl.slot))
                .collect::<Vec<_>>(),
            [("dung_reaction", 0), ("player_detected", 2)]
        );
        assert_eq!(
            document
                .states
                .iter()
                .map(|decl| (decl.name.as_str(), decl.index))
                .collect::<Vec<_>>(),
            [("idle", 0), ("combat", 1)]
        );

        let compiled = document.compile().unwrap();
        let program = &compiled.program;
        assert_eq!(program.species, 6);
        assert_eq!(program.root, 0);

        let root = root_table(program);
        // Only the state table and the two declared event slots are written;
        // how far the descriptor reaches is the materialiser's business.
        assert_eq!(
            root.iter().map(|(index, _)| index).collect::<Vec<_>>(),
            [0, EVENT_SLOTS[2].root_index, EVENT_SLOTS[0].root_index]
        );
        let main = main_table(program);
        assert!(main.declares(0) && main.declares(1) && !main.declares(2));
        assert_eq!(
            script_at(program, main.get(0).unwrap()),
            [0x05, 0x03, 0x06, 0x00, 0x07, 0x01]
        );
        assert_eq!(
            script_at(program, main.get(1).unwrap()),
            [0x05, 0x04, 0x01, 0x01, 0x07, 0x00]
        );

        // Each fixed event name selects its native descriptor position.
        assert_eq!(
            event_script(program, EVENT_SLOTS[0].root_index),
            Some([0x05, 0x03, 0x06, 0x00, 0xff, 0xfd].as_slice())
        );
        assert_eq!(
            event_script(program, EVENT_SLOTS[2].root_index),
            Some([0x05, 0x04, 0x01, 0x01].as_slice())
        );
        for slot in EVENT_SLOTS
            .iter()
            .skip(1)
            .take(1)
            .chain(EVENT_SLOTS.iter().skip(3))
        {
            assert_eq!(event_script(program, slot.root_index), None);
        }

        // A deliberate low-level escape remains byte-preserving.
        assert_eq!(compiled.warnings.len(), 1);
    }

    #[test]
    fn state_indices_follow_declaration_order_and_anchors() {
        let document = parse(
            "\
mhf_ai 1;
species 6;
states {
    idle = 0 { nop(); }
    combat = 3 { nop(); }
    flee { nop(); }
}
",
        )
        .unwrap();
        assert_eq!(
            document
                .states
                .iter()
                .map(|decl| (decl.name.as_str(), decl.index))
                .collect::<Vec<_>>(),
            [("idle", 0), ("combat", 3), ("flee", 4)]
        );

        let compiled = document.compile().unwrap();
        let main = main_table(&compiled.program);
        assert!(main.get(0).is_some());
        assert!(!main.declares(1));
        assert!(!main.declares(2));
        assert!(main.get(3).is_some());
        assert!(main.get(4).is_some());
        assert!(!main.declares(5));
    }

    #[test]
    fn literals_and_escapes_reach_the_byte_stream() {
        let document = parse(
            "\
mhf_ai 1;
species 6;

// hexadecimal coordinates and comments are accepted
actions {
    slash = [0x03:0x06]; // slash
}

events {
    dung_reaction {
        action[4:1](0x02);
        native(0x11, 0x92);
    }
}

states {
    idle = 0 {
        slash(3);
        wait(0x10);
        nop();
        stop();
        clear_behavior_requests();
        restart;
    }
}
",
        )
        .unwrap();
        assert_eq!(document.actions[0].group, 3);
        assert_eq!(document.actions[0].id, 6);

        let compiled = document.compile().unwrap();
        let program = &compiled.program;
        assert_eq!(
            script_at(program, main_table(program).get(0).unwrap()),
            [
                0x05, 0x03, 0x06, 0x03, // slash(3)
                0x48, 0x10, // wait(0x10)
                0x92, // nop()
                0x68, // stop()
                0x1e, // clear_behavior_requests()
                0x04, // restart
            ]
        );
        assert_eq!(
            event_script(program, EVENT_SLOTS[0].root_index),
            Some([0x05, 0x04, 0x01, 0x02, 0x11, 0x92].as_slice())
        );

        assert_eq!(compiled.warnings.len(), 1);
        assert!(
            compiled.warnings[0]
                .to_string()
                .contains("0x11 is dispatched by the interpreter but has no name")
        );
    }

    #[test]
    fn native_escape_reports_the_opcode_class() {
        let document = parse(
            "\
mhf_ai 1;
species 6;
states {
    idle = 0 {
        native(0x11);
        native(0x00);
    }
}
",
        )
        .unwrap();
        let compiled = document.compile().unwrap();
        assert_eq!(
            script_at(
                &compiled.program,
                main_table(&compiled.program).get(0).unwrap()
            ),
            [0x11, 0x00]
        );
        assert_eq!(compiled.warnings.len(), 2);
        assert!(compiled.warnings[0].to_string().contains("has no name"));
        assert!(
            compiled.warnings[1]
                .to_string()
                .contains("reaches the interpreter's switch default")
        );
    }

    #[test]
    fn a_native_base_declaration_writes_only_the_indices_it_names() {
        let document = parse(
            "\
mhf_ai 1;
species 6;
base native;

events {
    dung_reaction {
        nop();
    }

    awareness;
}

states {
    combat = 1 {
        stop();
    }
    extra = 3 {
        restart;
    }
    roam = 5;
}
",
        )
        .unwrap();
        let compiled = document.compile().unwrap();
        let program = &compiled.program;
        assert!(compiled.warnings.is_empty());

        // The declaration records what an undeclared index means instead of
        // writing a marker for every one of them.
        assert_eq!(program.base, Base::Native);
        assert_eq!(program.validate_lossless(), Ok(()));

        // The state table names three indices and has no opinion about the
        // other 253 the interpreter can address.
        let main = main_table(program);
        assert_eq!(
            main.iter().map(|(index, _)| index).collect::<Vec<_>>(),
            [1, 3, 5]
        );
        assert!(!main.declares(0));
        assert_eq!(main.get(1), Some(script_node(program, &[0x68])));
        assert_eq!(main.get(3), Some(script_node(program, &[0x04])));
        assert!(main.declares(5) && main.get(5).is_none());

        // A declared event body is one entry in a pointer cell; a bare entry
        // clears the slot; everything else stays with the live block.
        let root = root_table(program);
        assert_eq!(root.get(0), Some(1));
        assert_eq!(
            event_script(program, EVENT_SLOTS[0].root_index),
            Some([0x92].as_slice())
        );
        assert!(root.declares(EVENT_SLOTS[3].root_index));
        assert!(root.get(EVENT_SLOTS[3].root_index).is_none());
        assert!(!root.declares(EVENT_SLOTS[1].root_index));
        assert!(!root.declares(1));
    }

    #[test]
    fn empty_base_refuses_a_graph_the_interpreter_cannot_enter() {
        let no_states = parse("mhf_ai 1;\nspecies 6;\n").unwrap();
        assert!(
            no_states
                .compile()
                .unwrap_err()
                .to_string()
                .contains("needs a states block")
        );

        let cleared_entry = parse(
            "\
mhf_ai 1;
species 6;
states {
    idle;
}
",
        )
        .unwrap();
        assert!(
            cleared_entry
                .compile()
                .unwrap_err()
                .to_string()
                .contains("state index 0 needs a script")
        );
    }

    #[test]
    fn rejects_invalid_documents() {
        let cases: &[(&str, &str)] = &[
            ("species 6;\n", "expected 'mhf_ai', found 'species'"),
            (
                "mhf_ai 2;\nspecies 6;\n",
                "unsupported monster-AI DSL version 2",
            ),
            (
                "mhf_ai 1;\nspecies 300;\n",
                "species must be a number from 0 to 255",
            ),
            ("mhf_ai 1;\nspecies 6;\nbase lua;\n", "unknown base 'lua'"),
            ("mhf_ai 1;\nspecies 6;\nfoo { }\n", "unknown block 'foo'"),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(); } }\nstates { idle = 0 { nop(); } }\n",
                "duplicate 'states' block",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nactions { wait = [3:6]; }\n",
                "collides with a reserved name",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nactions { transition = [3:6]; }\n",
                "collides with a reserved name",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nactions { self = [1:1]; }\n",
                "collides with a reserved name",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nactions { slash = [3:6]; slash = [4:1]; }\nstates { idle = 0 { nop(); } }\n",
                "action 'slash' is declared twice",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(); } other = 0 { nop(); } }\n",
                "state index 0 is declared twice",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(); } idle = 2 { nop(); } }\n",
                "state 'idle' is declared twice",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nevents { dung_reaction { nop(); } dung_reaction { nop(); } }\n",
                "event 'dung_reaction' is declared twice",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nevents { 7 { nop(); } }\n",
                "expected built-in event name",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 256 { nop(); } }\n",
                "state index must be a number from 0 to 255",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 255 { nop(); } next { nop(); } }\n",
                "outside 0..=255",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop() } }\n",
                "expected ';', found '}'",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { 7; } }\n",
                "expected a statement",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(); } }\n@\n",
                "expected 'slot'",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { slash(0); } }\n",
                "unknown name 'slash'",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nactions { slash = [3:6]; }\nstates { idle = 0 { slash(0, 1); } }\n",
                "exactly 1 argument(s)",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(1); } }\n",
                "'nop' takes exactly 0 argument(s)",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { wait(); } }\n",
                "'wait' takes exactly 1 argument(s)",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { reset(); } }\n",
                "reset is a keyword, not a call",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { transition(combat); } combat { nop(); } }\n",
                "transition is a keyword, not a call",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { restart(); } }\n",
                "restart is a keyword, not a call",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { transition combat; } }\n",
                "transition target 'combat' is not declared",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nevents { dung_reaction { transition idle; } }\nstates { idle = 0 { nop(); } }\n",
                "transition is only valid inside a states block",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nevents { dung_reaction { restart; } }\nstates { idle = 0 { nop(); } }\n",
                "restart is only valid inside a states block",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { native(); } }\n",
                "native() needs at least one byte",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { native(0x05); } }\n",
                "truncated opcode 0x05",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { self.distance_to_reference(); } }\n",
                "unknown self method 'distance_to_reference'",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nstates { idle = 0 { if self.hate[1] { nop(); } } }\n",
                "unknown condition self.hate",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nevents { dung_reaction { nop(); } }\n",
                "an empty base needs a states block",
            ),
            (
                "mhf_ai 1;\nspecies 6;\nbase native;\nstates { idle = 0; }\n",
                "clearing state index 0 would leave the state table unenterable",
            ),
        ];

        for (source, expected) in cases {
            let reported = failure(source);
            assert!(
                reported.contains(expected),
                "expected {expected:?}, got {reported:?} for source:\n{source}"
            );
        }
    }

    #[test]
    fn errors_carry_the_source_position() {
        let document = parse(
            "\
mhf_ai 1;
species 6;
states {
    idle = 0 {
        nop();
        slash(0);
    }
}
",
        )
        .unwrap();
        let error = document.compile().unwrap_err();
        assert_eq!(
            error.to_string(),
            "6:9: unknown name 'slash': not a reserved command and not declared in the actions block (spec §6)"
        );
    }

    /// Legacy action aliases remain valid in inline drafts.
    #[test]
    fn legacy_action_alias_example_compiles() {
        let document = parse(
            "\
mhf_ai 1;
species 6;
base native;

actions {
    slash = [3:6];
    bash  = [4:1];
}

events {
    dung_reaction { slash(0); native(0xff, 0xfd); }
}

states {
    idle = 0 { slash(0); transition combat; }
    combat { bash(1);  transition idle; }
}
",
        )
        .unwrap();
        let compiled = document.compile().unwrap();
        assert_eq!(
            script_at(
                &compiled.program,
                main_table(&compiled.program).get(0).unwrap()
            ),
            [0x05, 0x03, 0x06, 0x00, 0x07, 0x01]
        );
        assert_eq!(compiled.warnings.len(), 1);
    }

    #[test]
    fn compile_checks_a_hand_built_document_again() {
        let mut document =
            parse("mhf_ai 1;\nspecies 6;\nstates { idle = 0 { nop(); } }\n").unwrap();
        document.states.push(StateDecl {
            index: 0,
            name: "idle".to_owned(),
            body: None,
            line: 1,
            column: 1,
        });
        assert!(
            document
                .compile()
                .unwrap_err()
                .to_string()
                .contains("state index 0 is declared twice")
        );
    }
}
