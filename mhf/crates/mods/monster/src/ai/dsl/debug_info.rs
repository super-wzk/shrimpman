//! Owned compiler source maps. Offsets remain relative to pointer-free script
//! nodes, so callers can resolve them independently of a native allocation.

use super::SourceFile;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceLocation {
    pub path: String,
    pub function: String,
    /// One-based line and UTF-8 byte column, matching compiler diagnostics.
    pub line: usize,
    pub column: usize,
    /// Exclusive end position.
    pub end_line: usize,
    pub end_column: usize,
    pub byte_start: usize,
    pub byte_end: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SourceMapping {
    pub script: usize,
    /// Half-open byte range within the script node.
    pub start: usize,
    pub end: usize,
    pub source: SourceLocation,
    /// A compiler-inserted branch marker or implicit function ending.
    pub generated: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugInfo {
    /// The exact source revision used for this compilation, including modules.
    pub files: Vec<SourceFile>,
    /// Sorted by `(script, start)`, with disjoint ranges within each script.
    pub mappings: Vec<SourceMapping>,
}

impl DebugInfo {
    pub fn lookup(&self, script: usize, offset: usize) -> Option<&SourceMapping> {
        let end = self
            .mappings
            .partition_point(|mapping| (mapping.script, mapping.start) <= (script, offset));
        let mapping = self.mappings.get(end.checked_sub(1)?)?;
        (mapping.script == script && offset < mapping.end).then_some(mapping)
    }

    /// A source statement may be copied into several branches by entry-return
    /// lowering. Bind every returned location, not just the first one.
    pub fn positions<'a>(
        &'a self,
        path: &'a str,
        line: usize,
    ) -> impl Iterator<Item = &'a SourceMapping> {
        self.mappings.iter().filter(move |mapping| {
            !mapping.generated && mapping.source.path == path && mapping.source.line == line
        })
    }
}
