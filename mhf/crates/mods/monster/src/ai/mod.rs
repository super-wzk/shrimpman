//! Monster AI: the pointer-free graph, the compiler that builds it, and the
//! code that installs it onto a live actor.
//!
//! The graph stores node references, never process pointers.  Publishing it
//! into the client is deliberately separate — the client keeps several cursors
//! into a graph — so the part that touches the client lives in `overlay`, which
//! only exists on the provider targets.
//!
//! The author-facing text form is [`dsl`]: `parse` reads a document and
//! `Document::compile` builds the graph the binding installs.
//! `docs/dsl-spec.md` owns the language.

use std::collections::BTreeMap;
use std::fmt;

pub mod bind;
pub mod bytecode;
pub mod control;
pub mod dsl;
#[cfg(all(feature = "provider", windows, target_arch = "x86"))]
pub(crate) mod overlay;

pub const MAX_NODES: usize = 65_536;
pub const MAX_PAYLOAD: usize = 16 * 1024 * 1024;

/// The descriptor used by the inspected ZZ HD client has fifteen fixed
/// pointer slots before its per-content tail starts.
///
/// The selector reads slots 8, 10, 11, 13 and 14 without reading a length
/// first, so the block the binding publishes has to cover them.  That is the
/// binding's obligation: the graph itself never claims a length.
pub const NATIVE_DESCRIPTOR_SLOTS: usize = 15;

/// What a graph's undeclared slots mean.
///
/// The distinction belongs to the whole graph, not to one table:
///
/// * [`Base::Empty`] — an undeclared slot is an empty slot.  A document without
///   `base native;` compiles to this shape: the client reads every slot from
///   the graph itself.
/// * [`Base::Native`] — an undeclared slot keeps the native entry at the same
///   index.  A `base native;` document compiles to this shape, and it is the
///   only shape the binding accepts: the binding copies the live block and
///   writes the declared indices over it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Base {
    /// Undeclared slots are empty; the graph stands on its own.
    #[default]
    Empty,
    /// Undeclared slots wait for the native block at the same index.
    Native,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Program {
    pub species: u8,
    /// What undeclared slots mean; see [`Base`].
    pub base: Base,
    pub root: usize,
    pub nodes: Vec<Node>,
}

/// A pointer table keyed by logical index.
///
/// The map holds only the slots this graph writes.  A missing key means "no
/// declaration for this index", and what that means is decided by the graph's
/// [`Base`]:
///
/// * for a [`Base::Native`] declaration the binding installs: the index keeps
///   the live block's entry;
/// * for a [`Base::Empty`] graph read as it stands: the index is empty, which
///   the client reads as a null pointer.
///
/// A value of `None` is an explicit clear: the binding deletes the block's
/// entry at that index, so a declaration can empty a slot it does not fill.
/// An empty base has nothing to clear, so only a `base native;` graph holds
/// `None`.
///
/// A table has no length.  The client indexes pointer arrays without a length
/// field, so how far a table reaches is a property of the layer that reads or
/// writes that memory, not of the compiled graph.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Table {
    entries: BTreeMap<usize, Option<usize>>,
}

impl Table {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a table from `(index, declared)` pairs.
    pub fn from_entries(entries: impl IntoIterator<Item = (usize, Option<usize>)>) -> Self {
        Self {
            entries: entries.into_iter().collect(),
        }
    }

    /// Declare `index` as a pointer to `node`.
    pub fn insert(&mut self, index: usize, node: usize) {
        self.entries.insert(index, Some(node));
    }

    /// Declare `index` empty, so merging drops the block's entry there.
    pub fn clear(&mut self, index: usize) {
        self.entries.insert(index, None);
    }

    /// The node `index` points at, if this table declares one.
    pub fn get(&self, index: usize) -> Option<usize> {
        self.entries.get(&index).copied().flatten()
    }

    /// Whether this table writes `index` at all, empty or not.
    pub fn declares(&self, index: usize) -> bool {
        self.entries.contains_key(&index)
    }

    /// Number of declared indices, not a table length.
    pub fn count(&self) -> usize {
        self.entries.len()
    }

    /// Declared indices in order, including explicit clears.
    pub fn iter(&self) -> impl Iterator<Item = (usize, Option<usize>)> + '_ {
        self.entries.iter().map(|(&index, &node)| (index, node))
    }

    /// One past the largest declared index, or 0 for an empty table.
    ///
    /// The binding uses it as the window of an event cell the document does
    /// not fill: the dispatcher reads `cell[0]` only, so a cell needs no more
    /// words than the declaration names.  It is not a table length.
    pub fn extent(&self) -> usize {
        self.entries.keys().next_back().map_or(0, |index| index + 1)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Node {
    Table(Table),
    Script(Vec<u8>),
}

impl Program {
    /// Validate the graph as a lossless, pointer-free resource.
    ///
    /// This deliberately accepts a declaration that only names part of the
    /// descriptor: a `base native;` document is held as written and the
    /// undeclared indices stay with the live block.
    pub fn validate_lossless(&self) -> Result<()> {
        if self.nodes.is_empty() {
            return Err(Error::new("program has no nodes"));
        }
        if self.nodes.len() > MAX_NODES {
            return Err(Error::new(format!(
                "program has {} nodes; maximum is {MAX_NODES}",
                self.nodes.len()
            )));
        }
        if self.root >= self.nodes.len() {
            return Err(Error::new(format!(
                "root node {} is outside {} nodes",
                self.root,
                self.nodes.len()
            )));
        }

        let mut payload = 0usize;
        for (node_index, node) in self.nodes.iter().enumerate() {
            match node {
                Node::Table(table) => {
                    payload = payload
                        .checked_add(table.count().checked_mul(4).ok_or_else(|| {
                            Error::new(format!("table node {node_index} is too large"))
                        })?)
                        .ok_or_else(|| Error::new("program payload size overflows"))?;
                    for (index, entry) in table.iter() {
                        let Some(target) = entry else {
                            // An explicit clear only means something where a
                            // block supplies the entry it removes.
                            if self.base == Base::Empty {
                                return Err(Error::new(format!(
                                    "table node {node_index} clears index {index}, but this graph already treats undeclared indices as empty"
                                )));
                            }
                            continue;
                        };
                        if target >= self.nodes.len() {
                            return Err(Error::new(format!(
                                "table node {node_index} index {index} references missing node {target}"
                            )));
                        }
                    }
                }
                Node::Script(bytes) => {
                    payload = payload
                        .checked_add(bytes.len())
                        .ok_or_else(|| Error::new("program payload size overflows"))?;
                    bytecode::decode(bytes).map_err(|message| {
                        Error::new(format!("script node {node_index}: {message}"))
                    })?;
                }
            }
            if payload > MAX_PAYLOAD {
                return Err(Error::new(format!(
                    "program payload exceeds {MAX_PAYLOAD} bytes"
                )));
            }
        }

        let root = match &self.nodes[self.root] {
            Node::Table(root) => root,
            _ => return Err(Error::new("root node must be a table")),
        };
        if root.declares(0) && root.get(0).is_none() {
            return Err(Error::new(
                "root[0] must not be cleared: the selector reads the state table through it",
            ));
        }
        let main = match root.get(0) {
            Some(main_index) => {
                let Node::Table(main) = &self.nodes[main_index] else {
                    return Err(Error::new("root[0] must reference a table"));
                };
                Some(main)
            }
            // A `base native;` document that declares no state keeps whatever
            // the native descriptor holds; a self-contained graph cannot.
            None if self.base == Base::Native => None,
            None => return Err(Error::new("root table has no main-script table")),
        };
        let Some(main) = main else {
            return Ok(());
        };

        if main.declares(0) && main.get(0).is_none() {
            return Err(Error::new(
                "main-script entry 0 must not be cleared: the interpreter enters through it",
            ));
        }
        if self.base == Base::Empty && main.get(0).is_none() {
            return Err(Error::new("main-script table has no entry 0"));
        }
        for (index, entry) in main.iter() {
            let Some(node) = entry else { continue };
            if !matches!(self.nodes[node], Node::Script(_)) {
                return Err(Error::new(format!(
                    "main-script entry {index} does not reference a script"
                )));
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    message: String,
    line: Option<usize>,
    column: Option<usize>,
}

impl Error {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: None,
            column: None,
        }
    }

    pub fn at(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: Some(line),
            column: Some(column),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, "{line}:{column}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

impl std::error::Error for Error {}

/// A non-fatal note produced while compiling a document.
///
/// The DSL spec makes the compiler report facts it cannot prove statically
/// (a `native(...)` escape, a command whose effect depends on a runtime lane
/// mask) instead of silently accepting or rejecting them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    message: String,
    line: Option<usize>,
    column: Option<usize>,
}

impl Diagnostic {
    pub fn at(line: usize, column: usize, message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            line: Some(line),
            column: Some(column),
        }
    }
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (self.line, self.column) {
            (Some(line), Some(column)) => write!(f, "{line}:{column}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::{Base, Node, Program, Table};

    #[test]
    fn validates_minimal_graph_and_aliases() {
        let program = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([(0, Some(1))])),
                Node::Table(Table::from_entries([(0, Some(2)), (1, Some(2))])),
                Node::Script(vec![7, 0]),
            ],
        };
        program.validate_lossless().unwrap();
        let Node::Table(main) = &program.nodes[1] else {
            unreachable!("the test builds a table at node 1")
        };
        assert_eq!(main.get(0), Some(2));
        assert_eq!(main.get(1), Some(2));
    }

    #[test]
    fn rejects_non_script_main_target() {
        let program = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([(0, Some(1))])),
                Node::Table(Table::from_entries([(0, Some(0))])),
            ],
        };
        assert!(
            program
                .validate_lossless()
                .unwrap_err()
                .to_string()
                .contains("entry 0")
        );
    }

    /// A declaration is free to be partial: it only carries the indices it
    /// writes, and the state table itself may wait for the native block.
    #[test]
    fn a_native_base_declaration_may_omit_the_state_table() {
        let program = Program {
            species: 6,
            base: Base::Native,
            root: 0,
            nodes: vec![Node::Table(Table::from_entries([(14, None)]))],
        };
        program.validate_lossless().unwrap();
    }

    /// An empty base has nothing to fall back on, so the two slots the client
    /// always reads have to be there.
    #[test]
    fn an_empty_base_needs_a_main_table_with_an_entry_zero() {
        let missing_table = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![Node::Table(Table::from_entries([(14, Some(0))]))],
        };
        assert!(
            missing_table
                .validate_lossless()
                .unwrap_err()
                .to_string()
                .contains("no main-script table")
        );

        let empty_table = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([(0, Some(1))])),
                Node::Table(Table::new()),
            ],
        };
        assert!(
            empty_table
                .validate_lossless()
                .unwrap_err()
                .to_string()
                .contains("no entry 0")
        );
    }

    /// `None` is the one thing an undeclared index cannot already mean.
    #[test]
    fn rejects_explicit_clears_in_a_graph_that_is_already_empty() {
        let program = Program {
            species: 6,
            base: Base::Empty,
            root: 0,
            nodes: vec![
                Node::Table(Table::from_entries([(0, Some(1)), (1, None)])),
                Node::Table(Table::from_entries([(0, Some(2))])),
                Node::Script(vec![0x92]),
            ],
        };
        assert!(
            program
                .validate_lossless()
                .unwrap_err()
                .to_string()
                .contains("treats undeclared indices as empty")
        );
    }
}
