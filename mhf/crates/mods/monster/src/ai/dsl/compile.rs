//! Compilation of a parsed document: name resolution, statement encoding, and
//! assembly of the pointer-free graph the native selector consumes.
//!
//! Both base kinds compile to the same shape: a table is a map from logical
//! index to node that holds only what the document declares.  An empty base
//! means an undeclared index is an empty slot; `base native;` means it keeps
//! the native entry once the binding merges the declaration onto the live
//! block.  The compiler never reads the client's tables, so it has nothing
//! to say about how far a table reaches.

use std::collections::HashMap;

use super::parser::{Callee, Document, Statement, StatementKind, check_document};
use super::slot::NativeSlot;
use crate::ai::control::EVENT_SLOTS;
use crate::ai::{Base, Diagnostic, Error, Node, Program, Result, Table, bytecode};

/// A compiled document plus the facts the compiler could not prove statically.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Compiled {
    pub program: Program,
    pub warnings: Vec<Diagnostic>,
}

/// A name the compiler owns (spec §6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Command {
    /// Author-writable: `opcode` followed by exactly `args` literal bytes.
    Fixed { opcode: &'static [u8], args: usize },
    /// `resume()`: `ff` plus a selector owned by the enclosing table family. No
    /// block in this language owns a cursor yet, so writing it is an error until
    /// contents/sub-contents/route blocks exist (spec §11.2).
    Resume,
}

fn reserved_command(name: &str) -> Option<Command> {
    let command = match name {
        "stop" => Command::Fixed {
            opcode: &[0x68],
            args: 0,
        },
        "clear_target" => Command::Fixed {
            opcode: &[0x1e],
            args: 0,
        },
        "nop" => Command::Fixed {
            opcode: &[0x92],
            args: 0,
        },
        "wait" => Command::Fixed {
            opcode: &[0x48],
            args: 1,
        },
        "resume" => Command::Resume,
        "area_end" => Command::Fixed {
            opcode: &[0xff, 0xfb],
            args: 0,
        },
        "route_move_end" => Command::Fixed {
            opcode: &[0xff, 0xfe],
            args: 0,
        },
        _ => return None,
    };
    Some(command)
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

struct Compiler<'a> {
    auto_finish: bool,
    names: Names<'a>,
    warnings: Vec<Diagnostic>,
    functions: &'a [super::parser::Function],
    stack: Vec<String>,
    native_functions: &'a HashMap<String, NativeSlot>,
    native_scope: Option<NativeSlot>,
}

impl Compiler<'_> {
    fn encode_branch(
        &mut self,
        statement: &Statement,
        name: &str,
        body: &[Statement],
        scope: Scope,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        let start = out.len();
        self.encode_body(body, scope, out)?;
        if !closed(&out[start..])? {
            return Err(statement.error(format!(
                "{name} branch contains an unclosed native conditional"
            )));
        }
        Ok(())
    }

    fn encode_body(&mut self, body: &[Statement], scope: Scope, out: &mut Vec<u8>) -> Result<bool> {
        for statement in body {
            if out.len() > super::super::decompile::MAX_SCRIPT_BYTES {
                return Err(statement.error("expanded function exceeds 64 KiB"));
            }
            if matches!(statement.kind, StatementKind::Return) {
                if let Some(slot) = self.native_scope
                    && self.stack.len() == 1
                {
                    out.extend_from_slice(&[0xff, slot.ending()]);
                    return Ok(true);
                }
                if !closed(out)? {
                    return Err(
                        statement.error("return inside a native conditional is not supported")
                    );
                }
                return Ok(false);
            }
            if let StatementKind::Call {
                callee: Callee::Name(name),
                args,
            } = &statement.kind
                && let Some(function) = self.functions.iter().find(|f| f.name == *name)
            {
                if name == "main" {
                    return Err(statement.error("main is an entry point, not a callable helper"));
                }
                self.require_args(statement, args, 0, name)?;
                if let Some(slot) = self.native_functions.get(name) {
                    let call = slot.call();
                    out.extend_from_slice(&call);
                    if self
                        .native_scope
                        .is_some_and(|current| current.is_same_level_call(&call))
                        && closed(out)?
                    {
                        return Ok(true);
                    }
                    continue;
                }
                if self.stack.contains(name) || self.stack.len() >= 64 {
                    return Err(statement.error(format!(
                        "recursive or excessively deep function call: {name}"
                    )));
                }
                self.stack.push(name.clone());
                let transferred = self
                    .encode_body(&function.body, scope, out)
                    .map_err(|error| Error::new(format!("{}: {error}", function.name)))?;
                self.stack.pop();
                if transferred {
                    return Ok(true);
                }
                continue;
            }
            self.encode_statement(statement, scope, out)?;
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

    fn encode_statement(
        &mut self,
        statement: &Statement,
        scope: Scope,
        out: &mut Vec<u8>,
    ) -> Result<()> {
        match &statement.kind {
            StatementKind::EntryBody(body) => {
                self.encode_body(body, scope, out)?;
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
                    self.encode_branch(statement, "random", body, scope, out)?;
                }
                out.extend_from_slice(&[0x80, 0xff]);
            }
            StatementKind::TargetDistanceGroups(branches) => {
                if !(2..=5).contains(&branches.len()) {
                    return Err(statement.error("distance match requires 1..4 groups and else"));
                }
                out.extend_from_slice(&[0x83, 0, branches.len() as u8 - 1]);
                for (index, body) in branches.iter().enumerate() {
                    out.extend_from_slice(&[0x83, index as u8 + 1]);
                    self.encode_branch(statement, "distance", body, scope, out)?;
                }
                out.extend_from_slice(&[0x83, 0xff]);
            }
            StatementKind::SelectTargetEntity(strategy) => out.push(strategy.opcode()),
            StatementKind::SelectPlayerSlot(slot) => out.extend_from_slice(&[0x06, 1, 0, *slot]),
            StatementKind::SelectWaypoint(index) => out.extend_from_slice(&[0x06, 2, 1, *index]),
            StatementKind::SelectRelativePoint(direction) => {
                out.extend_from_slice(&[0x06, 6, *direction as u8, 0]);
            }
            StatementKind::BindAwarenessTarget => out.push(0x11),
            StatementKind::BindCurrentTarget => out.push(0x13),
            StatementKind::SetMode(mode) => out.extend_from_slice(&[0x40, *mode as u8]),
            StatementKind::UpdateTargetPosition => out.push(0x4d),
            StatementKind::IncrementRandomValue => out.push(0x84),
            StatementKind::If {
                condition,
                then_body,
                else_body,
            } => {
                let encoding = condition.encoding();
                if let super::condition::Condition::TargetAngleIn { min, max } = condition {
                    let actual_min = super::condition::Degrees::from_native(min.native()).value();
                    let actual_max = super::condition::Degrees::from_native(max.native()).value();
                    if min.value() != actual_min || max.value() != actual_max {
                        self.warnings.push(Diagnostic::at(statement.line, statement.column, format!(
                            "target_angle_in({}, {}) quantized/clamped to [{actual_min}, {actual_max}] degrees; native maximum is 358.59375",
                            min.value(), max.value()
                        )));
                    }
                }
                out.extend_from_slice(&encoding.begin);
                self.encode_branch(statement, "if", then_body, scope, out)?;
                if let Some(body) = else_body {
                    out.extend_from_slice(encoding.otherwise);
                    self.encode_branch(statement, "else", body, scope, out)?;
                }
                out.extend_from_slice(encoding.end);
            }
            StatementKind::Return => unreachable!("handled by encode_body"),
            StatementKind::Call { callee, args } => match callee {
                Callee::Action { group, id } => {
                    self.require_args(statement, args, 1, "an action call")?;
                    out.extend_from_slice(&bytecode::encode_action(*group, *id, args[0]));
                }
                Callee::Name(name) => {
                    if let Some(command) = reserved_command(name) {
                        match command {
                            Command::Fixed {
                                opcode,
                                args: expected,
                            } => {
                                self.require_args(statement, args, expected, &format!("'{name}'"))?;
                                out.extend_from_slice(opcode);
                                out.extend_from_slice(args);
                            }
                            Command::Resume => {
                                return Err(statement.error(
                                    "resume() returns to a cursor owned by a contents, sub-contents, or route table; no block in this document owns one (spec §11.2)",
                                ));
                            }
                        }
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
            StatementKind::Repeat { .. } => {
                return Err(statement.error(
                    "repeat is parsed but not emitted yet: 0x24's handler (0x108628F0) and the width routine (0x10860730) disagree about the size of its body marker, so the closing sequence is unconfirmed (spec §11.9)",
                ));
            }
            StatementKind::Native { bytes } => {
                out.extend_from_slice(bytes);
                self.warnings.push(Diagnostic::at(
                statement.line,
                statement.column,
                format!(
                    "native(...) keeps its bytes verbatim: {}. Only instruction widths are checked (spec §5)",
                    describe_escape(bytes)
                ),
            ));
            }
        }
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
/// Functions are expanded at the call site, so a normal return emits no bytes.
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

impl Document {
    /// Compile the document into a pointer-free graph.
    ///
    /// A `base native;` document compiles to a declaration: its tables carry
    /// only the indices it writes, and the rest waits for the native block.
    /// The binding merges that declaration onto the block it will replace.
    pub fn compile(&self) -> Result<Compiled> {
        check_document(self)?;
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
        let mut compiler = Compiler {
            auto_finish: self.auto_finish,
            names: Names::collect(self),
            warnings: Vec::new(),
            functions: &self.functions,
            stack: Vec::new(),
            native_functions: &self.native_functions,
            native_scope: None,
        };
        // Check even unused helpers, so a broken imported file cannot be hidden
        // by the current entry bindings. Context-specific checks still happen
        // again while compiling each entry.
        for function in &self.functions {
            compiler.native_scope = self.native_functions.get(&function.name).copied();
            compiler.stack.push(function.name.clone());
            compiler
                .encode_body(&function.body, Scope::States, &mut Vec::new())
                .map_err(|error| Error::new(format!("{}: {error}", function.name)))?;
            compiler.stack.pop();
        }
        compiler.warnings.clear();
        compiler.native_scope = None;
        let states = self.encode_states(&mut compiler)?;
        let events = self.encode_events(&mut compiler)?;
        let mut program = match self.base {
            Base::Empty => assemble_empty(self, states, events)?,
            Base::Native => assemble_native(self, states, events)?,
        };
        self.encode_native_functions(&mut compiler, &mut program)?;
        program.validate_lossless()?;
        Ok(Compiled {
            program,
            warnings: compiler.warnings,
        })
    }

    fn encode_native_functions(
        &self,
        compiler: &mut Compiler<'_>,
        program: &mut Program,
    ) -> Result<()> {
        let mut bindings: Vec<_> = self.native_functions.iter().collect();
        bindings.sort_by_key(|(_, slot)| **slot);
        for (name, slot) in bindings {
            let function = self
                .functions
                .iter()
                .find(|f| &f.name == name)
                .ok_or_else(|| Error::new(format!("@slot function '{name}' is not declared")))?;
            compiler.native_scope = Some(*slot);
            compiler.stack.push(name.clone());
            let mut bytes = Vec::new();
            compiler.encode_body(&function.body, Scope::States, &mut bytes)?;
            compiler.stack.pop();
            finish(&mut bytes, slot.ending(), Some(*slot))?;

            let script = program.nodes.len();
            program.nodes.push(Node::Script(bytes));
            let table = ensure_root_table(program, slot.table);
            let Node::Table(table) = &mut program.nodes[table] else {
                unreachable!()
            };
            table.insert(usize::from(slot.index), script);
        }
        Ok(())
    }

    fn encode_states(&self, compiler: &mut Compiler<'_>) -> Result<Vec<(u8, Option<Vec<u8>>)>> {
        let mut encoded = Vec::with_capacity(self.states.len());
        if let Some(main) = self.functions.iter().find(|f| f.name == "main") {
            let mut bytes = Vec::new();
            compiler.stack.push(main.name.clone());
            compiler.encode_body(&main.body, Scope::States, &mut bytes)?;
            compiler.stack.pop();
            finish(&mut bytes, 0x00, None)?;
            check_decodes(&bytes, main.line, main.column, "main")?;
            encoded.push((0, Some(bytes)));
        }
        for decl in &self.states {
            let Some(body) = &decl.body else {
                encoded.push((decl.index, None));
                continue;
            };
            let mut bytes = Vec::new();
            compiler.encode_body(body, Scope::States, &mut bytes)?;
            if self.auto_finish {
                finish(&mut bytes, 0x00, None)?;
            }
            check_decodes(
                &bytes,
                decl.line,
                decl.column,
                &format!("state '{}'", decl.name),
            )?;
            encoded.push((decl.index, Some(bytes)));
        }
        Ok(encoded)
    }

    fn encode_events(&self, compiler: &mut Compiler<'_>) -> Result<Vec<(u8, Option<Vec<u8>>)>> {
        let mut encoded = Vec::with_capacity(self.events.len());
        for decl in &self.events {
            let Some(body) = &decl.body else {
                encoded.push((decl.slot, None));
                continue;
            };
            let mut bytes = Vec::new();
            compiler.encode_body(body, Scope::Events, &mut bytes)?;
            if self.auto_finish {
                finish(&mut bytes, EVENT_SLOTS[usize::from(decl.slot)].ending, None)?;
            }
            let what = format!("event '{}'", EVENT_SLOTS[usize::from(decl.slot)].name);
            check_decodes(&bytes, decl.line, decl.column, &what)?;
            encoded.push((decl.slot, Some(bytes)));
        }
        Ok(encoded)
    }
}

/// Build a self-contained graph: the descriptor, the state table, and one
/// script per declared entry.  An index the document does not write is empty.
fn assemble_empty(
    document: &Document,
    states: Vec<(u8, Option<Vec<u8>>)>,
    events: Vec<(u8, Option<Vec<u8>>)>,
) -> Result<Program> {
    if states.is_empty() {
        return Err(Error::new(
            "an empty base needs a states block: the interpreter enters through the state table's entry 0, and there is no native table to inherit (spec §8)",
        ));
    }

    let main_index = 1;
    let mut nodes = vec![Node::Table(Table::new()), Node::Table(Table::new())];
    let mut root = Table::new();
    root.insert(0, main_index);
    let mut main = Table::new();

    for (index, body) in &states {
        let Some(bytes) = body else { continue };
        let node = nodes.len();
        nodes.push(Node::Script(bytes.clone()));
        main.insert(usize::from(*index), node);
    }
    if main.get(0).is_none() {
        return Err(Error::new(
            "state index 0 needs a script: the interpreter enters through the state table's entry 0 (spec §8)",
        ));
    }

    for (slot, body) in &events {
        let Some(bytes) = body else { continue };
        let script = nodes.len();
        nodes.push(Node::Script(bytes.clone()));
        let cell = nodes.len();
        nodes.push(Node::Table(Table::from_entries([(0, Some(script))])));
        root.insert(EVENT_SLOTS[usize::from(*slot)].root_index, cell);
    }

    nodes[main_index] = Node::Table(main);
    nodes[0] = Node::Table(root);
    Ok(Program {
        species: document.species,
        base: Base::Empty,
        root: 0,
        nodes,
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
    states: Vec<(u8, Option<Vec<u8>>)>,
    events: Vec<(u8, Option<Vec<u8>>)>,
) -> Result<Program> {
    let mut nodes = vec![Node::Table(Table::new())];
    let mut root = Table::new();

    if !states.is_empty() {
        let main_index = nodes.len();
        nodes.push(Node::Table(Table::new()));
        let mut main = Table::new();
        for (index, body) in &states {
            let index = usize::from(*index);
            match body {
                Some(bytes) => {
                    let node = nodes.len();
                    nodes.push(Node::Script(bytes.clone()));
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

    for (slot, body) in &events {
        let root_index = EVENT_SLOTS[usize::from(*slot)].root_index;
        match body {
            Some(bytes) => {
                let script = nodes.len();
                nodes.push(Node::Script(bytes.clone()));
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
    })
}
