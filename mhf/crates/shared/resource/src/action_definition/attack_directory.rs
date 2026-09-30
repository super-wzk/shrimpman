//! Owned attack lookup metadata from the original SDT file directory.
use super::AttackReference;
use crate::{Error, PathSegment, ResourcePath, Result, sdt};

#[derive(Clone, Debug)]
struct Entry {
    index: u32,
    kind: u16,
    subtype: u16,
    offset: usize,
    records: Result<Option<usize>>,
}

/// Retains original file indices before the native loader reorders its copy.
/// Applications can share this small immutable directory without retaining
/// native pointers or parsing the complete SDT for every displayed reference.
#[derive(Clone, Debug)]
pub struct AttackDirectory {
    source: String,
    entries: Vec<Entry>,
}

impl AttackDirectory {
    pub fn from_sdt(source: &str, file: &sdt::Sdt<'_>) -> Result<Self> {
        let source = ResourcePath::new(source)
            .map_err(|error| Error::new(0, error.to_string()))?
            .source()
            .to_owned();
        let entries = file
            .entries()
            .iter()
            .map(|entry| {
                Ok(Entry {
                    index: u32::try_from(entry.index).map_err(|_| {
                        Error::new(
                            entry.offset,
                            "SDT directory index exceeds resource path range",
                        )
                    })?,
                    kind: entry.kind,
                    subtype: entry.subtype,
                    offset: entry.offset,
                    records: file
                        .table(entry, sdt::TableKind::Attack)
                        .map(|table| table.map(|table| table.count)),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self { source, entries })
    }

    pub fn resolve(&self, reference: AttackReference) -> Result<Option<ResourcePath>> {
        // 10AFCD10 (10AFCEFA/10AFCF40) stably sorts (kind, subtype), then
        // 10AFCFD0 takes the kind lower bound. 10AFD040 adds an exact subtype.
        // Keep the original file index and first physical entry for equal keys.
        let Some(entry) = self
            .entries
            .iter()
            .filter(|entry| {
                entry.kind == reference.category
                    && reference
                        .subtype
                        .is_none_or(|subtype| entry.subtype == subtype)
            })
            .min_by_key(|entry| entry.subtype)
        else {
            return Ok(None);
        };
        let Some(count) = entry.records.as_ref().map_err(Clone::clone)? else {
            return Ok(None);
        };
        if usize::from(reference.record) >= *count {
            return Err(Error::new(
                entry.offset,
                "SDT attack record index out of range",
            ));
        }
        ResourcePath::from_parts(
            &self.source,
            [
                PathSegment::Index(entry.index),
                PathSegment::Field("attacks".into()),
                PathSegment::Index(u32::from(reference.record)),
            ],
        )
        .map(Some)
        .map_err(|error| Error::new(entry.offset, error.to_string()))
    }
}
