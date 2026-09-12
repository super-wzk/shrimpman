//! Native effect banks and motion-triggered effects stored in resource packages.
//!
//! `113D5A90` consumes an offset/size archive whose first member describes each
//! following member as `(u16 kind, u16 resource_id)`. Kind 1 is an effect bank;
//! kind 2 is a motion-event table. Compression is handled by `container` first.

mod definition;
mod motion_events;

pub use definition::{CurveKind, CurveLookup, CurveReference, Definition56};
pub use motion_events::{MotionEvent, MotionEvents, MotionLookup};

use crate::{Error, Result, container::SimpleArchive};
use std::io::Cursor;

#[derive(Clone, Debug)]
pub struct EffectArchive<'a> {
    bytes: &'a [u8],
    /// Original offset/size records, including the descriptor member at index 0.
    pub directory: SimpleArchive<'a>,
    pub index: EffectIndex<'a>,
    /// A member is parsed on demand, avoiding repeated allocations for aliases.
    pub members: Vec<EffectMember<'a>>,
}

impl<'a> EffectArchive<'a> {
    pub fn parse(bytes: &'a [u8], max_entries: usize) -> Result<Self> {
        let directory = SimpleArchive::parse(bytes, max_entries)?;
        let descriptor = directory
            .entries
            .first()
            .ok_or_else(|| Error::new(0, "effect archive has no descriptor member"))?;
        let index_bytes = descriptor.payload(bytes)?;
        let header = index_bytes.get(..4).ok_or_else(|| {
            Error::new(
                descriptor.offset as usize,
                "truncated effect descriptor header",
            )
        })?;
        let unknown_00 = u16::from_le_bytes(header[..2].try_into().unwrap());
        let count = u16::from_le_bytes(header[2..].try_into().unwrap());
        // 113D5A90 requires exactly one descriptor for every following member.
        if count as usize != directory.entries.len() - 1 {
            return Err(Error::new(
                descriptor.offset as usize + 2,
                "effect descriptor count differs from member count",
            ));
        }
        let descriptor_size = 4 + usize::from(count) * 4;
        if descriptor_size > index_bytes.len() {
            return Err(Error::new(
                descriptor.offset as usize + 4,
                "truncated effect descriptors",
            ));
        }
        let mut entries = Vec::with_capacity(count as usize);
        let mut members = Vec::with_capacity(count as usize);
        let records = index_bytes[4..descriptor_size].as_chunks::<4>().0;
        for (entry, record) in directory.entries.iter().skip(1).zip(records) {
            let reference = EffectReference {
                kind: u16::from_le_bytes(record[..2].try_into().unwrap()),
                resource_id: u16::from_le_bytes(record[2..].try_into().unwrap()),
            };
            entries.push(reference);
            members.push(EffectMember {
                index: entry.index,
                offset: entry.offset,
                size: entry.size,
                reference,
                bytes: entry.payload(bytes)?,
            });
        }
        Ok(Self {
            bytes,
            index: EffectIndex {
                bytes: index_bytes,
                offset: descriptor.offset,
                unknown_00,
                count,
                entries,
                trailing_bytes: &index_bytes[descriptor_size..],
            },
            directory,
            members,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectReference {
    pub kind: u16,
    pub resource_id: u16,
}

#[derive(Clone, Debug)]
pub struct EffectIndex<'a> {
    bytes: &'a [u8],
    pub offset: u32,
    pub unknown_00: u16,
    pub count: u16,
    pub entries: Vec<EffectReference>,
    pub trailing_bytes: &'a [u8],
}

impl<'a> EffectIndex<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

#[derive(Clone, Debug)]
pub struct EffectMember<'a> {
    bytes: &'a [u8],
    pub index: usize,
    pub offset: u32,
    pub size: u32,
    pub reference: EffectReference,
}

impl<'a> EffectMember<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn resource(&self) -> Result<EffectResource<'a>> {
        match self.reference.kind {
            1 => EffectBank::parse(self.bytes).map(|bank| EffectResource::Bank(Box::new(bank))),
            2 => MotionEvents::parse(self.bytes).map(EffectResource::MotionEvents),
            _ => Ok(EffectResource::Unknown(self.bytes)),
        }
    }
}

#[derive(Clone, Debug)]
pub enum EffectResource<'a> {
    Bank(Box<EffectBank<'a>>),
    MotionEvents(MotionEvents<'a>),
    /// Native 113D5A90 skips unrecognized descriptor kinds.
    Unknown(&'a [u8]),
}

/// Kind 1 payload. `113D4A70` lays out all arrays consecutively after 28 bytes.
#[derive(Clone, Debug)]
pub struct EffectBank<'a> {
    bytes: &'a [u8],
    pub version: u16,
    /// Counts at header +2..+18, in native table order.
    pub counts: [u16; 9],
    pub unknown_14: [u8; 8],
    /// Offsets relative to this payload, even for empty tables.
    pub table_offsets: [usize; 9],
    pub emitters: Vec<Emitter>,
    pub vector_keys: Vec<VectorKey>,
    pub color_keys: Vec<ColorKey>,
    pub integer_keys: Vec<IntegerKey>,
    pub definitions_56: Vec<Definition56>,
    pub definitions_140: Vec<Definition140>,
    pub motion_lookup: Option<MotionLookup>,
    pub motion_events: Vec<MotionEvent>,
    /// The loader records table 9's address when count[8] is nonzero, but its
    /// stride/semantics are unconfirmed. Includes all remaining source bytes.
    pub trailing_bytes: &'a [u8],
}

impl<'a> EffectBank<'a> {
    pub const HEADER_SIZE: usize = 28;

    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let header = bytes
            .get(..Self::HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated effect bank header"))?;
        let version = u16::from_le_bytes(header[..2].try_into().unwrap());
        if version < 4 {
            return Err(Error::new(
                0,
                "effect bank version predates the native 28-byte layout",
            ));
        }
        let counts = std::array::from_fn(|index| {
            u16::from_le_bytes(header[2 + 2 * index..4 + 2 * index].try_into().unwrap())
        });
        let mut table_offsets = [0; 9];
        let mut tables: [&[u8]; 6] = [&[]; 6];
        let mut offset = Self::HEADER_SIZE;
        for (index, stride) in [112, 24, 16, 16, 56, 140].into_iter().enumerate() {
            table_offsets[index] = offset;
            let end = offset
                .checked_add(usize::from(counts[index]) * stride)
                .ok_or_else(|| Error::new(offset, "effect bank table size overflow"))?;
            tables[index] = bytes
                .get(offset..end)
                .ok_or_else(|| Error::new(offset, "effect bank table exceeds its member"))?;
            offset = end;
        }
        // Each table's complete extent is checked before allocating records.
        let emitters = tables[0]
            .as_chunks::<112>()
            .0
            .iter()
            .map(Emitter::from_record)
            .collect();
        let vector_keys = tables[1]
            .as_chunks::<24>()
            .0
            .iter()
            .map(VectorKey::from_record)
            .collect();
        let color_keys = tables[2]
            .as_chunks::<16>()
            .0
            .iter()
            .map(ColorKey::from_record)
            .collect();
        let integer_keys = tables[3]
            .as_chunks::<16>()
            .0
            .iter()
            .map(IntegerKey::from_record)
            .collect();
        let definitions_56 = tables[4]
            .as_chunks::<56>()
            .0
            .iter()
            .map(Definition56::from_record)
            .collect();
        let definitions_140 = tables[5]
            .as_chunks::<140>()
            .0
            .iter()
            .map(Definition140::from_record)
            .collect();
        let mut cursor = Cursor::new(bytes);
        cursor.set_position(offset as u64);
        table_offsets[6] = offset;
        let motion_lookup = if counts[6] == 0 {
            None
        } else {
            Some(MotionLookup::read(&mut cursor, counts[6])?)
        };
        table_offsets[7] = cursor.position() as usize;
        let motion_events = MotionEvent::read_table(&mut cursor, counts[7])?;
        table_offsets[8] = cursor.position() as usize;
        Ok(Self {
            bytes,
            version,
            counts,
            unknown_14: header[20..28].try_into().unwrap(),
            table_offsets,
            emitters,
            vector_keys,
            color_keys,
            integer_keys,
            definitions_56,
            definitions_140,
            motion_lookup,
            motion_events,
            trailing_bytes: &bytes[cursor.position() as usize..],
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// 112-byte emission record, evaluated by 113D3FC0 and 113D37E0..113D3F00.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Emitter {
    pub position_bits: [u32; 3],
    pub position_random_bits: [u32; 3],
    pub rotation_bits: [u32; 3],
    pub rotation_random_bits: [u32; 3],
    pub scale_bits: [u32; 3],
    pub scale_random_bits: [u32; 3],
    pub unknown_48: u32,
    /// Matches +4 of a 56/140-byte definition; 0xffff also marks looping emitters.
    pub definition_id: u16,
    pub trigger_frame: u16,
    /// Selector used by MotionEvent::emitter_id and native effect-spawn calls.
    pub emitter_id: u16,
    pub unknown_52: u16,
    /// Bit 0 selects the 140-byte definition table; otherwise the 56-byte table.
    pub flags: u16,
    pub unknown_56: u16,
    pub spawn_count: i16,
    pub unknown_5a: u16,
    pub unknown_5c: u32,
    pub unknown_60: [u8; 16],
}

impl Emitter {
    pub const SIZE: usize = 112;

    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        let vectors: [[u32; 3]; 6] = std::array::from_fn(|row| {
            std::array::from_fn(|column| {
                let offset = 12 * row + 4 * column;
                u32::from_le_bytes(record[offset..offset + 4].try_into().unwrap())
            })
        });
        Self {
            position_bits: vectors[0],
            position_random_bits: vectors[1],
            rotation_bits: vectors[2],
            rotation_random_bits: vectors[3],
            scale_bits: vectors[4],
            scale_random_bits: vectors[5],
            unknown_48: u32::from_le_bytes(record[72..76].try_into().unwrap()),
            definition_id: u16::from_le_bytes(record[76..78].try_into().unwrap()),
            trigger_frame: u16::from_le_bytes(record[78..80].try_into().unwrap()),
            emitter_id: u16::from_le_bytes(record[80..82].try_into().unwrap()),
            unknown_52: u16::from_le_bytes(record[82..84].try_into().unwrap()),
            flags: u16::from_le_bytes(record[84..86].try_into().unwrap()),
            unknown_56: u16::from_le_bytes(record[86..88].try_into().unwrap()),
            spawn_count: i16::from_le_bytes(record[88..90].try_into().unwrap()),
            unknown_5a: u16::from_le_bytes(record[90..92].try_into().unwrap()),
            unknown_5c: u32::from_le_bytes(record[92..96].try_into().unwrap()),
            unknown_60: record[96..112].try_into().unwrap(),
        }
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (index, value) in [
            self.position_bits,
            self.position_random_bits,
            self.rotation_bits,
            self.rotation_random_bits,
            self.scale_bits,
            self.scale_random_bits,
        ]
        .into_iter()
        .flatten()
        .enumerate()
        {
            bytes[4 * index..4 * index + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[72..76].copy_from_slice(&self.unknown_48.to_le_bytes());
        for (index, value) in [
            self.definition_id,
            self.trigger_frame,
            self.emitter_id,
            self.unknown_52,
            self.flags,
            self.unknown_56,
            self.spawn_count as u16,
            self.unknown_5a,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[76 + 2 * index..78 + 2 * index].copy_from_slice(&value.to_le_bytes());
        }
        bytes[92..96].copy_from_slice(&self.unknown_5c.to_le_bytes());
        bytes[96..].copy_from_slice(&self.unknown_60);
        bytes
    }
}

/// Three-component curve key. 113CCD90 interpolates the three encoded floats.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VectorKey {
    pub value_bits: [u32; 3],
    pub frame: i32,
    pub flags: u16,
    pub curve_id: u8,
    pub unknown_13: u8,
    pub unknown_14: [u8; 4],
}

impl VectorKey {
    pub const SIZE: usize = 24;
    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        Self {
            value_bits: std::array::from_fn(|index| {
                u32::from_le_bytes(record[4 * index..4 * index + 4].try_into().unwrap())
            }),
            frame: i32::from_le_bytes(record[12..16].try_into().unwrap()),
            flags: u16::from_le_bytes(record[16..18].try_into().unwrap()),
            curve_id: record[18],
            unknown_13: record[19],
            unknown_14: record[20..].try_into().unwrap(),
        }
    }
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (index, value) in self.value_bits.into_iter().enumerate() {
            bytes[4 * index..4 * index + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes[12..16].copy_from_slice(&self.frame.to_le_bytes());
        bytes[16..18].copy_from_slice(&self.flags.to_le_bytes());
        bytes[18] = self.curve_id;
        bytes[19] = self.unknown_13;
        bytes[20..].copy_from_slice(&self.unknown_14);
        bytes
    }
}

/// Color curve key. 113CCAF0 interpolates RGBA and packs an ARGB result.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ColorKey {
    pub frame: u32,
    pub flags: u16,
    pub curve_id: u8,
    pub unknown_07: u8,
    pub rgba: [u8; 4],
    pub unknown_0c: [u8; 4],
}

impl ColorKey {
    pub const SIZE: usize = 16;
    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        Self {
            frame: u32::from_le_bytes(record[..4].try_into().unwrap()),
            flags: u16::from_le_bytes(record[4..6].try_into().unwrap()),
            curve_id: record[6],
            unknown_07: record[7],
            rgba: record[8..12].try_into().unwrap(),
            unknown_0c: record[12..].try_into().unwrap(),
        }
    }
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..4].copy_from_slice(&self.frame.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.flags.to_le_bytes());
        bytes[6] = self.curve_id;
        bytes[7] = self.unknown_07;
        bytes[8..12].copy_from_slice(&self.rgba);
        bytes[12..].copy_from_slice(&self.unknown_0c);
        bytes
    }
}

/// Discrete integer curve key, consumed by 113CC8E0 and 113CC970.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntegerKey {
    pub frame: u32,
    pub flags: u16,
    pub curve_id: u16,
    pub value: i32,
    pub unknown_0c: [u8; 4],
}

impl IntegerKey {
    pub const SIZE: usize = 16;
    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        Self {
            frame: u32::from_le_bytes(record[..4].try_into().unwrap()),
            flags: u16::from_le_bytes(record[4..6].try_into().unwrap()),
            curve_id: u16::from_le_bytes(record[6..8].try_into().unwrap()),
            value: i32::from_le_bytes(record[8..12].try_into().unwrap()),
            unknown_0c: record[12..].try_into().unwrap(),
        }
    }
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..4].copy_from_slice(&self.frame.to_le_bytes());
        bytes[4..6].copy_from_slice(&self.flags.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.curve_id.to_le_bytes());
        bytes[8..12].copy_from_slice(&self.value.to_le_bytes());
        bytes[12..].copy_from_slice(&self.unknown_0c);
        bytes
    }
}

/// Definition selected when emission flags bit 0 is set (113D0B60).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition140 {
    pub unknown_00: [u8; 4],
    pub definition_id: u16,
    pub unknown_06: [u8; 134],
}

impl Definition140 {
    pub const SIZE: usize = 140;
    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        Self {
            unknown_00: record[..4].try_into().unwrap(),
            definition_id: u16::from_le_bytes(record[4..6].try_into().unwrap()),
            unknown_06: record[6..].try_into().unwrap(),
        }
    }
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..4].copy_from_slice(&self.unknown_00);
        bytes[4..6].copy_from_slice(&self.definition_id.to_le_bytes());
        bytes[6..].copy_from_slice(&self.unknown_06);
        bytes
    }
}
