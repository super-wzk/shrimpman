//! Member descriptors and shared-resource links read by 113DA8C0/113DA650.
use crate::{
    Error, Result,
    container::{Entry, SimpleArchive, StageArchive},
};

pub const REFERENCE_MAGIC: [u8; 16] = [
    0xff, 0xfe, 0xfd, 0xfc, 0xfb, 0xfa, 0xf9, 0xf8, 0xf7, 0xf6, 0xf5, 0xf4, 0xf3, 0xf2, 0xf1, 0xf0,
];

#[derive(Clone, Debug)]
pub struct ObjectIndex<'a> {
    pub unknown_00: u16,
    pub count: u16,
    pub kinds: &'a [u8],
    pub trailing: &'a [u8],
    source: &'a [u8],
}
impl<'a> ObjectIndex<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..4)
            .ok_or_else(|| Error::new(0, "truncated stage object index"))?;
        let count = u16::from_le_bytes(header[2..4].try_into().unwrap());
        let end = 4 + usize::from(count);
        let kinds = source
            .get(4..end)
            .ok_or_else(|| Error::new(2, "stage object kinds exceed index"))?;
        Ok(Self {
            unknown_00: u16::from_le_bytes(header[..2].try_into().unwrap()),
            count,
            kinds,
            trailing: &source[end..],
            source,
        })
    }
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[derive(Clone, Debug)]
pub struct ObjectMember<'a> {
    /// 1=model, 2=skeleton, 3=textures, 4=control tables, 5/6=collision,
    /// 7=KEFFECT, 8..11=motion, 13=effect archive, 14=counted words of unknown
    /// meaning. Other values remain raw.
    pub kind: u8,
    pub entry: Entry,
    pub bytes: &'a [u8],
}
#[derive(Clone, Debug)]
pub struct ObjectPackage<'a> {
    pub archive: SimpleArchive<'a>,
    pub index: ObjectIndex<'a>,
    pub members: Vec<ObjectMember<'a>>,
}
impl<'a> ObjectPackage<'a> {
    pub fn parse(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let archive = SimpleArchive::parse(source, max_entries)?;
        let index_entry = archive
            .entries
            .first()
            .ok_or_else(|| Error::new(0, "stage object package has no index member"))?;
        let index = ObjectIndex::parse(index_entry.payload(source)?).map_err(|error| {
            Error::new(index_entry.offset as usize + error.offset, error.message)
        })?;
        if usize::from(index.count) + 1 > archive.entries.len() {
            return Err(Error::new(
                index_entry.offset as usize + 2,
                "stage object kind count exceeds archive members",
            ));
        }
        let members = archive.entries[1..]
            .iter()
            .zip(index.kinds)
            .map(|(&entry, &kind)| {
                Ok(ObjectMember {
                    kind,
                    entry,
                    bytes: entry.payload(source)?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            archive,
            index,
            members,
        })
    }
    pub fn probe(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let package = Self::parse(source, max_entries)?;
        if package.index.unknown_00 != 1
            || usize::from(package.index.count) + 1 != package.archive.entries.len()
            || !package.index.trailing.is_empty()
            || !package
                .members
                .iter()
                .any(|member| matches!(member.kind, 1..=14))
        {
            return Err(Error::new(
                package.archive.entries[0].offset as usize,
                "members do not identify an observed stage object package",
            ));
        }
        Ok(package)
    }
    /// Native reference lookup takes the first member of the requested kind.
    pub fn member(&self, kind: u8) -> Option<&ObjectMember<'a>> {
        self.members.iter().find(|member| member.kind == kind)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceReference {
    pub resource_id: u32,
}
impl ResourceReference {
    pub const SIZE: usize = 20;
    pub fn has_magic(source: &[u8]) -> bool {
        source.starts_with(&REFERENCE_MAGIC)
    }
    pub fn parse(source: &[u8]) -> Result<Self> {
        if !Self::has_magic(source) {
            return Err(Error::new(0, "expected stage resource-reference signature"));
        }
        let id = source
            .get(16..20)
            .ok_or_else(|| Error::new(16, "truncated stage resource ID"))?;
        Ok(Self {
            resource_id: u32::from_le_bytes(id.try_into().unwrap()),
        })
    }
}
#[derive(Clone, Debug)]
pub struct ResolvedMember<'a> {
    pub resource_id: u32,
    pub kind: u8,
    pub entry_index: usize,
    /// Absolute offset within the enclosing StageArchive source.
    pub offset: usize,
    pub bytes: &'a [u8],
    /// Starts at the requested resource ID and ends at the concrete member.
    pub references: Vec<u32>,
}
impl<'a> StageArchive<'a> {
    /// Iteratively resolve a member without copying its original encoded bytes.
    /// Missing IDs/kinds and cycles remain errors, never substituted resources.
    pub fn resolve_member(
        &self,
        resource_id: u32,
        kind: u8,
        max_entries: usize,
    ) -> Result<ResolvedMember<'a>> {
        let mut current = resource_id;
        let mut references = Vec::new();
        loop {
            if references.contains(&current) {
                return Err(Error::new(
                    0,
                    format!("cyclic stage resource reference at ID {current}, kind {kind}"),
                ));
            }
            references.push(current);
            let resource = self
                .entries
                .iter()
                .find(|entry| entry.resource_id == Some(current))
                .ok_or_else(|| {
                    Error::new(0, format!("stage resource ID {current} is not present"))
                })?;
            let base = resource.entry.offset as usize;
            let package = ObjectPackage::parse(resource.entry.payload(self.source)?, max_entries)
                .map_err(|error| nested_error(base, self.source.len(), error))?;
            let member = package.member(kind).ok_or_else(|| {
                Error::new(
                    base,
                    format!("stage resource ID {current} has no member of kind {kind}"),
                )
            })?;
            // Empty directory entries may retain FFFFFFFF instead of a live
            // offset. Their empty payload is package.source[..0].
            let offset = if member.entry.size == 0 {
                base
            } else {
                base + member.entry.offset as usize
            };
            if ResourceReference::has_magic(member.bytes) {
                current = ResourceReference::parse(member.bytes)
                    .map_err(|error| nested_error(offset, self.source.len(), error))?
                    .resource_id;
            } else {
                return Ok(ResolvedMember {
                    resource_id: current,
                    kind,
                    entry_index: member.entry.index,
                    offset,
                    bytes: member.bytes,
                    references,
                });
            }
        }
    }
}

fn nested_error(base: usize, source_len: usize, error: Error) -> Error {
    if let Some(offset) = base
        .checked_add(error.offset)
        .filter(|&offset| offset <= source_len)
    {
        Error::new(offset, error.message)
    } else {
        Error::new(
            base.min(source_len),
            format!(
                "{} (resource-relative offset {:#x})",
                error.message, error.offset
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn archive(entries: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = vec![0; 4 + entries.len() * 8];
        bytes[..4].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (index, payload) in entries.iter().enumerate() {
            let offset = bytes.len() as u32;
            bytes[4 + index * 8..8 + index * 8].copy_from_slice(&offset.to_le_bytes());
            bytes[8 + index * 8..12 + index * 8]
                .copy_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        bytes
    }
    fn package(kinds: &[u8], payloads: &[Vec<u8>]) -> Vec<u8> {
        let mut index = vec![1, 0];
        index.extend_from_slice(&(kinds.len() as u16).to_le_bytes());
        index.extend_from_slice(kinds);
        let mut entries = vec![index];
        entries.extend_from_slice(payloads);
        archive(&entries)
    }
    fn reference(id: u32) -> Vec<u8> {
        let mut bytes = REFERENCE_MAGIC.to_vec();
        bytes.extend_from_slice(&id.to_le_bytes());
        bytes
    }
    fn stage(resources: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0; 28 + resources.len() * 12];
        bytes[24..28].copy_from_slice(&(resources.len() as u32).to_le_bytes());
        for (index, (id, payload)) in resources.iter().enumerate() {
            let offset = bytes.len() as u32;
            bytes[28 + index * 12..32 + index * 12].copy_from_slice(&id.to_le_bytes());
            bytes[32 + index * 12..36 + index * 12].copy_from_slice(&offset.to_le_bytes());
            bytes[36 + index * 12..40 + index * 12]
                .copy_from_slice(&(payload.len() as u32).to_le_bytes());
            bytes.extend_from_slice(payload);
        }
        bytes
    }
    #[test]
    fn descriptors_keep_unknown_kinds_and_native_member_order() {
        let bytes = package(
            &[3, 1, 255],
            &[b"texture".to_vec(), b"model".to_vec(), vec![7]],
        );
        let file = ObjectPackage::probe(&bytes, 4).unwrap();
        assert_eq!(file.index.kinds, [3, 1, 255]);
        assert_eq!(file.member(1).unwrap().entry.index, 2);
        assert_eq!(file.member(1).unwrap().bytes, b"model");
        assert_eq!(file.member(255).unwrap().bytes, [7]);
        assert!(ObjectPackage::parse(&bytes, 3).is_err());
        let mut malformed = bytes;
        let offset = u32::from_le_bytes(malformed[4..8].try_into().unwrap()) as usize;
        malformed[offset + 2..offset + 4].copy_from_slice(&4u16.to_le_bytes());
        assert!(ObjectPackage::parse(&malformed, 5).is_err());
    }
    #[test]
    fn references_resolve_by_resource_id_and_kind_with_original_source_offsets() {
        let source = stage(&[
            (77, package(&[1, 2], &[reference(87), b"skeleton".to_vec()])),
            (87, package(&[3, 1], &[b"textures".to_vec(), reference(99)])),
            (
                99,
                package(&[2, 1], &[b"other skeleton".to_vec(), b"model".to_vec()]),
            ),
        ]);
        let directory = StageArchive::parse(&source, 6).unwrap();
        let resolved = directory.resolve_member(77, 1, 4).unwrap();
        assert_eq!(resolved.references, [77, 87, 99]);
        assert_eq!(resolved.resource_id, 99);
        assert_eq!(resolved.entry_index, 2);
        assert_eq!(resolved.bytes, b"model");
        assert_eq!(resolved.bytes.as_ptr(), source[resolved.offset..].as_ptr());
        assert_eq!(
            directory.resolve_member(77, 2, 4).unwrap().bytes,
            b"skeleton"
        );
        assert!(directory.resolve_member(77, 4, 4).is_err());
        assert!(directory.resolve_member(123, 1, 4).is_err());
    }
    #[test]
    fn cycles_and_truncated_links_do_not_discard_or_substitute_the_source() {
        let source = stage(&[
            (77, package(&[1], &[reference(87)])),
            (87, package(&[1], &[reference(77)])),
        ]);
        let original = source.clone();
        let directory = StageArchive::parse(&source, 5).unwrap();
        assert!(
            directory
                .resolve_member(77, 1, 4)
                .unwrap_err()
                .message
                .contains("cyclic")
        );
        assert_eq!(source, original);
        let source = reference(77);
        for length in 0..ResourceReference::SIZE {
            assert!(ResourceReference::parse(&source[..length]).is_err());
        }
    }

    #[test]
    fn empty_member_stale_offsets_and_invalid_nested_offsets_do_not_overflow() {
        let mut target = package(&[1], &[vec![7]]);
        target[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        target[16..20].fill(0);
        let source = stage(&[(77, target.clone())]);
        let directory = StageArchive::parse(&source, 4).unwrap();
        let member = directory.resolve_member(77, 1, 2).unwrap();
        assert!(member.bytes.is_empty());
        assert_eq!(member.offset, directory.entries[3].entry.offset as usize);
        assert_eq!(member.bytes.as_ptr(), source[member.offset..].as_ptr());
        target[16..20].copy_from_slice(&1u32.to_le_bytes());
        let source = stage(&[(77, target)]);
        let directory = StageArchive::parse(&source, 4).unwrap();
        let error = directory.resolve_member(77, 1, 2).unwrap_err();
        assert!(error.offset <= source.len());
    }
}
