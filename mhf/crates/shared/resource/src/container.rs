//! Resource envelopes and offset directories. BIN/TXB/PAC are filename
//! conventions, not signatures: use the complete directory to validate them.

use std::{collections::BTreeMap, ops::Deref};

use crate::{
    Decoded, Error, Result,
    crypto::{Ecd, EcdHeader, Exf, ExfHeader},
    jkr::{Header as JkrHeader, Jkr},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerHeader {
    Ecd(EcdHeader),
    Exf(ExfHeader),
    Jkr(JkrHeader),
}

#[derive(Clone, Debug)]
pub struct OpenedResource<'a> {
    /// Original file, including all encoded bytes. Suitable for verbatim export.
    pub source: &'a [u8],
    /// Each layer owns its decoded bytes. Its original input is `source` or
    /// the preceding layer, without storing references into this vector.
    pub layers: Vec<Decoded<LayerHeader, Box<[u8]>>>,
}

impl OpenedResource<'_> {
    pub fn payload(&self) -> &[u8] {
        self.layers.last().map_or(self.source, |layer| layer)
    }

    /// Exact input for a recorded envelope, not a reconstructed encoding.
    pub fn layer_source(&self, index: usize) -> Option<&[u8]> {
        if index >= self.layers.len() {
            None
        } else if index == 0 {
            Some(self.source)
        } else {
            Some(&self.layers[index - 1])
        }
    }
}

impl Deref for OpenedResource<'_> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        self.payload()
    }
}

/// Open only signatures at the beginning of each payload. This does not scan
/// for embedded signatures. The byte budget is cumulative across retained
/// decoded layers; max_layers bounds nested wrappers, including no-op JKR.
pub fn open_layers(
    source: &[u8],
    max_output_bytes: usize,
    max_layers: usize,
) -> Result<OpenedResource<'_>> {
    let mut resource = OpenedResource {
        source,
        layers: Vec::new(),
    };
    let mut remaining = max_output_bytes;
    loop {
        let bytes = resource.payload();
        let magic = bytes.get(..4);
        if !matches!(magic, Some(b"ecd\x1a" | b"exf\x1a" | b"JKR\x1a")) {
            return Ok(resource);
        }
        if resource.layers.len() >= max_layers {
            return Err(Error::new(
                0,
                "resource envelope depth exceeds caller budget",
            ));
        }
        let decoded = match magic {
            Some(b"ecd\x1a") => {
                let file = Ecd::parse(bytes)?;
                file.decode(remaining)?
                    .map_encoding(|file| LayerHeader::Ecd(file.header))
            }
            Some(b"exf\x1a") => {
                let file = Exf::parse(bytes)?;
                file.decode(remaining)?
                    .map_encoding(|file| LayerHeader::Exf(file.header))
            }
            Some(b"JKR\x1a") => {
                let file = Jkr::parse(bytes)?;
                file.decode(remaining)?
                    .map_encoding(|file| LayerHeader::Jkr(file.header))
            }
            _ => unreachable!("signature checked above"),
        };
        remaining -= decoded.len();
        resource.layers.push(decoded);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry {
    pub index: usize,
    pub offset: u32,
    pub size: u32,
}

impl Entry {
    pub fn payload<'a>(&self, source: &'a [u8]) -> Result<&'a [u8]> {
        // Empty slots can retain stale offsets. They do not reference bytes.
        if self.size == 0 {
            Ok(&source[..0])
        } else {
            let offset = self.offset as usize;
            let end = offset
                .checked_add(self.size as usize)
                .ok_or_else(|| Error::new(offset, "archive entry range overflow"))?;
            source
                .get(offset..end)
                .ok_or_else(|| Error::new(offset, "archive entry outside resource"))
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DirectoryKind {
    OffsetSize,
    Momo,
}

#[derive(Clone, Debug)]
pub struct SimpleArchive<'a> {
    pub source: &'a [u8],
    pub kind: DirectoryKind,
    pub count: u32,
    pub table_offset: usize,
    pub entries: Vec<Entry>,
}

impl<'a> SimpleArchive<'a> {
    /// The whole count + offset/size table is checked before exposing entries.
    /// Empty slots are preserved. Aliases and non-contiguous layouts are legal.
    pub fn parse(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let (kind, count_offset) = if source.starts_with(b"MOMO") {
            (DirectoryKind::Momo, 4)
        } else {
            (DirectoryKind::OffsetSize, 0)
        };
        let bytes = source
            .get(count_offset..count_offset + 4)
            .ok_or_else(|| Error::new(count_offset, "truncated archive count"))?;
        let count = u32::from_le_bytes(bytes.try_into().unwrap());
        let table_offset = count_offset + 4;
        let end = table_end(source, table_offset, count as usize, 8, max_entries)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count as usize)
            .map_err(|_| Error::new(count_offset, "cannot allocate archive directory"))?;
        for (index, bytes) in source[table_offset..end]
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
        {
            let field = table_offset + index * 8;
            let entry = Entry {
                index,
                offset: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                size: u32::from_le_bytes(bytes[4..].try_into().unwrap()),
            };
            check_entry(source, &entry, end, field)?;
            entries.push(entry);
        }
        Ok(Self {
            source,
            kind,
            count,
            table_offset,
            entries,
        })
    }

    pub fn payload(&self, index: usize) -> Result<&'a [u8]> {
        self.entries
            .get(index)
            .ok_or_else(|| Error::new(self.table_offset, "archive entry index out of bounds"))?
            .payload(self.source)
    }
}

#[derive(Clone, Debug)]
pub struct StageEntry {
    pub entry: Entry,
    /// Additional entries begin with the ID used by native 113E8DA0 to match
    /// their resource payload to the placement table's resource_id field.
    pub resource_id: Option<u32>,
}

#[derive(Clone, Debug)]
pub struct StageArchive<'a> {
    pub source: &'a [u8],
    pub additional_count: u32,
    pub entries: Vec<StageEntry>,
}

impl<'a> StageArchive<'a> {
    /// Explicit stage-container interpretation. There is no dependable stage
    /// magic; callers must not infer it just from the `.pac` extension.
    pub fn parse(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let header = source
            .get(..28)
            .ok_or_else(|| Error::new(0, "truncated stage header"))?;
        let additional_count = u32::from_le_bytes(header[24..28].try_into().unwrap());
        let count = (additional_count as usize)
            .checked_add(3)
            .ok_or_else(|| Error::new(0x18, "stage entry count overflow"))?;
        if count > max_entries {
            return Err(Error::new(
                0x18,
                "stage directory exceeds caller entry budget",
            ));
        }
        let end = table_end(source, 0x1c, additional_count as usize, 12, max_entries)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(count)
            .map_err(|_| Error::new(0x18, "cannot allocate stage directory"))?;
        for index in 0..count {
            let field = if index < 3 {
                index * 8
            } else {
                0x1c + (index - 3) * 12
            };
            let length = if index < 3 { 8 } else { 12 };
            let bytes = &source[field..field + length];
            let offset_field = if index < 3 { 0 } else { 4 };
            let entry = Entry {
                index,
                offset: u32::from_le_bytes(
                    bytes[offset_field..offset_field + 4].try_into().unwrap(),
                ),
                size: u32::from_le_bytes(
                    bytes[offset_field + 4..offset_field + 8]
                        .try_into()
                        .unwrap(),
                ),
            };
            check_entry(source, &entry, end, field + offset_field)?;
            entries.push(StageEntry {
                entry,
                resource_id: if index < 3 {
                    None
                } else {
                    Some(u32::from_le_bytes(bytes[..4].try_into().unwrap()))
                },
            });
        }
        Ok(Self {
            source,
            additional_count,
            entries,
        })
    }

    /// Content-based recognition requires the native placement table in slot 0,
    /// in addition to validating every directory record and payload boundary.
    pub fn probe(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let archive = Self::parse(source, max_entries)?;
        let entry = &archive.entries[0].entry;
        crate::stage::PlacementTable::probe(entry.payload(source)?)
            .map_err(|error| Error::new(entry.offset as usize + error.offset, error.message))?;
        Ok(archive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MhaHeader {
    pub magic: [u8; 4],
    pub entries_offset: u32,
    pub count: u32,
    pub names_offset: u32,
    pub names_size: u32,
    pub unknown_14: u16,
    pub unknown_16: u16,
}

#[derive(Clone, Debug)]
pub struct MhaEntry<'a> {
    pub entry: Entry,
    /// Relative to MhaHeader::names_offset, not the whole file.
    pub name_offset: u32,
    pub padded_size: u32,
    pub file_id: u32,
    /// Original name bytes; decoding and display escaping are UI concerns.
    pub name: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct MhaArchive<'a> {
    pub source: &'a [u8],
    pub header: MhaHeader,
    pub entries: Vec<MhaEntry<'a>>,
}

impl<'a> MhaArchive<'a> {
    pub fn parse(source: &'a [u8], max_entries: usize) -> Result<Self> {
        let bytes = source
            .get(..24)
            .ok_or_else(|| Error::new(0, "truncated MHA header"))?;
        let header = MhaHeader {
            magic: bytes[..4].try_into().unwrap(),
            entries_offset: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            count: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            names_offset: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
            names_size: u32::from_le_bytes(bytes[16..20].try_into().unwrap()),
            unknown_14: u16::from_le_bytes(bytes[20..22].try_into().unwrap()),
            unknown_16: u16::from_le_bytes(bytes[22..24].try_into().unwrap()),
        };
        if header.magic != *b"mha\x01" {
            return Err(Error::new(0, "expected MHA signature"));
        }
        if header.entries_offset < 24 || header.names_offset < 24 {
            return Err(Error::new(4, "MHA directory overlaps header"));
        }
        let entries_end = table_end(
            source,
            header.entries_offset as usize,
            header.count as usize,
            20,
            max_entries,
        )?;
        let names_end = (header.names_offset as usize)
            .checked_add(header.names_size as usize)
            .ok_or_else(|| Error::new(12, "MHA names range overflow"))?;
        let names = source
            .get(header.names_offset as usize..names_end)
            .ok_or_else(|| Error::new(12, "MHA name block outside resource"))?;
        // Resolve sorted name offsets in one forward pass. Duplicate and
        // suffix-sharing names are legal; scanning each alias independently
        // could otherwise do count * names_size work on an untrusted archive.
        let mut name_ends = BTreeMap::new();
        let records = source[header.entries_offset as usize..entries_end]
            .as_chunks::<20>()
            .0;
        for (index, bytes) in records.iter().enumerate() {
            let field = header.entries_offset as usize + index * 20;
            let offset = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
            if offset >= names.len() {
                return Err(Error::new(field, "MHA name offset outside name block"));
            }
            name_ends.insert(offset, 0usize);
        }
        let mut previous_end = None;
        for (&offset, end) in &mut name_ends {
            *end = if let Some(previous) = previous_end.filter(|&value| offset <= value) {
                previous
            } else {
                offset
                    + names[offset..]
                        .iter()
                        .position(|&byte| byte == 0)
                        .ok_or_else(|| {
                            Error::new(
                                header.names_offset as usize + offset,
                                "unterminated MHA entry name",
                            )
                        })?
            };
            previous_end = Some(*end);
        }
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(header.count as usize)
            .map_err(|_| Error::new(8, "cannot allocate MHA directory"))?;
        for (index, bytes) in records.iter().enumerate() {
            let field = header.entries_offset as usize + index * 20;
            let name_offset = u32::from_le_bytes(bytes[..4].try_into().unwrap());
            let entry = Entry {
                index,
                offset: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                size: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            };
            let padded_size = u32::from_le_bytes(bytes[12..16].try_into().unwrap());
            let file_id = u32::from_le_bytes(bytes[16..20].try_into().unwrap());
            check_entry(source, &entry, 24, field + 4)?;
            if padded_size < entry.size {
                return Err(Error::new(
                    field + 12,
                    "MHA padded size smaller than payload",
                ));
            }
            if padded_size > entry.size {
                let padded = Entry {
                    size: padded_size,
                    ..entry
                };
                padded.payload(source)?;
            }
            let name_end = name_ends[&(name_offset as usize)];
            entries.push(MhaEntry {
                entry,
                name_offset,
                padded_size,
                file_id,
                name: &names[name_offset as usize..name_end],
            });
        }
        Ok(Self {
            source,
            header,
            entries,
        })
    }
}

fn table_end(
    source: &[u8],
    offset: usize,
    count: usize,
    stride: usize,
    max_entries: usize,
) -> Result<usize> {
    if count > max_entries {
        return Err(Error::new(
            offset,
            "archive directory exceeds caller entry budget",
        ));
    }
    let size = count
        .checked_mul(stride)
        .ok_or_else(|| Error::new(offset, "archive directory length overflow"))?;
    let end = offset
        .checked_add(size)
        .ok_or_else(|| Error::new(offset, "archive directory range overflow"))?;
    source
        .get(offset..end)
        .ok_or_else(|| Error::new(offset, "truncated archive directory"))?;
    Ok(end)
}

fn check_entry(source: &[u8], entry: &Entry, directory_end: usize, field: usize) -> Result<()> {
    if entry.size != 0 && (entry.offset as usize) < directory_end {
        return Err(Error::new(field, "archive payload overlaps its directory"));
    }
    entry.payload(source)?;
    Ok(())
}
