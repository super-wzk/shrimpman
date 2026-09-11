//! Grouped material parameters consumed by native 108FCD70.
//!
//! This resource has no magic signature. Its monster-package context is needed
//! for identification; a leading 0x20 alone does not identify this format.
//! The original 96/100-byte record layout is retained without the client's
//! legacy-record expansion or assignment into runtime material objects.

use std::io::{Cursor, Read};

use crate::{Error, Result};

pub const HEADER_SIZE: usize = 16;
pub const LEGACY_RECORD_SIZE: usize = 96;
pub const EXTENDED_RECORD_SIZE: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MaterialHeader {
    pub offset: usize,
    /// Native code reads a signed byte; negative counts are rejected by parse.
    pub count: u8,
    pub unknown: [u8; 15],
}

impl MaterialHeader {
    fn parse(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        let offset = cursor.position() as usize;
        let mut bytes = [0; HEADER_SIZE];
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| Error::new(offset, "truncated material count header"))?;
        if (bytes[0] as i8) < 0 {
            return Err(Error::new(offset, "negative material count"));
        }
        let mut unknown = [0; 15];
        unknown.copy_from_slice(&bytes[1..]);
        Ok(Self {
            offset,
            count: bytes[0],
            unknown,
        })
    }
}

#[derive(Clone, Debug)]
pub struct MaterialRecord<'a> {
    pub offset: usize,
    /// Original f32 bits, with no color-space conversion or normalization.
    pub color_00: [u32; 4],
    pub color_10: [u32; 4],
    pub color_20: [u32; 4],
    /// Six (legacy) or seven (extended) f32 words starting at file offset 0x30.
    /// Their shader meanings remain unresolved. No missing word is synthesized.
    pub parameter_words: Vec<u32>,
    pub unknown_tail: &'a [u8],
    source: &'a [u8],
}

impl<'a> MaterialRecord<'a> {
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[derive(Clone, Debug)]
pub struct MaterialGroup<'a> {
    pub header: MaterialHeader,
    pub records: Vec<MaterialRecord<'a>>,
}

#[derive(Clone, Debug)]
pub struct GroupedMaterials<'a> {
    /// Native code skips the first byte when it is at least 0x20 and then reads
    /// 100-byte records. Smaller first bytes belong to the legacy file header.
    pub version_marker: Option<u8>,
    pub header: MaterialHeader,
    pub groups: Vec<MaterialGroup<'a>>,
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> GroupedMaterials<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        let mut first = [0; 1];
        cursor
            .read_exact(&mut first)
            .map_err(|_| Error::new(0, "truncated material file header"))?;
        let version_marker = if first[0] >= 0x20 {
            Some(first[0])
        } else {
            cursor.set_position(0);
            None
        };
        let stride = if version_marker.is_some() {
            EXTENDED_RECORD_SIZE
        } else {
            LEGACY_RECORD_SIZE
        };
        let header = MaterialHeader::parse(&mut cursor)?;
        if usize::from(header.count) > (source.len() - cursor.position() as usize) / HEADER_SIZE {
            return Err(Error::new(
                header.offset,
                "material group count exceeds remaining file",
            ));
        }
        let mut groups = Vec::with_capacity(usize::from(header.count));
        for _ in 0..header.count {
            let header = MaterialHeader::parse(&mut cursor)?;
            if usize::from(header.count) > (source.len() - cursor.position() as usize) / stride {
                return Err(Error::new(
                    header.offset,
                    "material record count exceeds remaining file",
                ));
            }
            let mut records = Vec::with_capacity(usize::from(header.count));
            for _ in 0..header.count {
                let offset = cursor.position() as usize;
                let mut bytes = [0; EXTENDED_RECORD_SIZE];
                cursor
                    .read_exact(&mut bytes[..stride])
                    .map_err(|_| Error::new(offset, "truncated material parameter record"))?;
                let color = |start| {
                    std::array::from_fn(|i| {
                        let at = start + i * 4;
                        u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap())
                    })
                };
                let unknown_at = stride - 24;
                records.push(MaterialRecord {
                    offset,
                    color_00: color(0),
                    color_10: color(16),
                    color_20: color(32),
                    parameter_words: bytes[48..unknown_at]
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .map(|word| u32::from_le_bytes(*word))
                        .collect(),
                    unknown_tail: &source[offset + unknown_at..offset + stride],
                    source: &source[offset..offset + stride],
                });
            }
            groups.push(MaterialGroup { header, records });
        }
        Ok(Self {
            version_marker,
            header,
            groups,
            trailing: &source[cursor.position() as usize..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
#[path = "tests/material.rs"]
mod tests;
