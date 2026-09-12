//! The 56-byte definition and its references into an effect bank's key tables.
//! Native lookup returns the first match plus the total number of matches;
//! this can differ from filtering all records with the requested curve ID.

use std::ops::Range;

use crate::binary::{BinaryValue, Endian, Reader};

use super::{ColorKey, EffectBank, IntegerKey, VectorKey};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CurveKind {
    Vector,
    Color,
    Integer,
}

impl CurveKind {
    pub const fn table_index(self) -> usize {
        match self {
            Self::Vector => 1,
            Self::Color => 2,
            Self::Integer => 3,
        }
    }

    pub const fn record_size(self) -> usize {
        match self {
            Self::Vector => VectorKey::SIZE,
            Self::Color => ColorKey::SIZE,
            Self::Integer => IntegerKey::SIZE,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CurveReference {
    /// Byte offset of the stored ID within its definition record.
    pub offset: usize,
    pub kind: CurveKind,
    /// Full value passed to native lookup. Most references zero-extend a u16;
    /// the integer reference at +0x24 instead sign-extends an i16.
    pub id: i32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CurveLookup {
    /// Original table indices, in file order. No sorting or regrouping occurs.
    pub matching_indices: Vec<usize>,
}

impl CurveLookup {
    /// Native interpolation consumes this contiguous first-plus-count span,
    /// even when matching records are separated by another curve's keys.
    pub fn native_range(&self) -> Option<Range<usize>> {
        let first = *self.matching_indices.first()?;
        Some(first..first.checked_add(self.matching_indices.len())?)
    }

    pub fn is_contiguous(&self) -> bool {
        self.matching_indices
            .windows(2)
            .all(|pair| pair[0].checked_add(1) == Some(pair[1]))
    }
}

impl EffectBank<'_> {
    /// 113CCD10/113CCAB0 compare a byte key ID with the complete reference;
    /// 113CC8A0 compares an unsigned word. A wide vector/color reference does
    /// not wrap to its low byte. Neither zero nor 0xffff is an implicit sentinel.
    pub fn curve_lookup(&self, reference: CurveReference) -> CurveLookup {
        match reference.kind {
            CurveKind::Vector => lookup(
                self.vector_keys.iter().map(|key| i32::from(key.curve_id)),
                reference.id,
            ),
            CurveKind::Color => lookup(
                self.color_keys.iter().map(|key| i32::from(key.curve_id)),
                reference.id,
            ),
            CurveKind::Integer => lookup(
                self.integer_keys.iter().map(|key| i32::from(key.curve_id)),
                reference.id,
            ),
        }
    }
}

fn lookup(ids: impl Iterator<Item = i32>, requested: i32) -> CurveLookup {
    CurveLookup {
        matching_indices: ids
            .enumerate()
            .filter_map(|(index, id)| (id == requested).then_some(index))
            .collect(),
    }
}

/// Selected by emission flags bit 0 clear and constructed by 113CD4B0.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Definition56 {
    pub flags: u32,
    pub definition_id: u16,
    pub unknown_06: u16,
    pub unknown_08: u16,
    pub integer_curve_0a: u16,
    /// Threshold for the native effect age counter, before instance time
    /// scaling and repeat/owner handling. This is not a duration in seconds.
    pub duration_steps: u16,
    pub position_curve_id: u16,
    pub rotation_curve_id: u16,
    pub scale_curve_id: u16,
    pub unknown_14: [u8; 12],
    pub color_curve_id: u16,
    pub vector_curve_22: u16,
    pub integer_curve_24: i16,
    pub vector_curve_26: u16,
    pub unknown_28: [u8; 16],
}

impl Definition56 {
    pub const SIZE: usize = 56;

    pub fn from_record(record: &[u8; Self::SIZE]) -> Self {
        let mut reader = Reader::new(record);
        // The fixed array supplies all five segments without fallible bounds.
        let flags = reader.read::<u32>().unwrap().value;
        let prefix = reader.read::<[u16; 8]>().unwrap().value;
        let unknown_14 = reader.read::<[u8; 12]>().unwrap().value;
        let curves = reader.read::<[u16; 4]>().unwrap().value;
        let unknown_28 = reader.read::<[u8; 16]>().unwrap().value;
        Self {
            flags,
            definition_id: prefix[0],
            unknown_06: prefix[1],
            unknown_08: prefix[2],
            integer_curve_0a: prefix[3],
            duration_steps: prefix[4],
            position_curve_id: prefix[5],
            rotation_curve_id: prefix[6],
            scale_curve_id: prefix[7],
            unknown_14,
            color_curve_id: curves[0],
            vector_curve_22: curves[1],
            integer_curve_24: curves[2] as i16,
            vector_curve_26: curves[3],
            unknown_28,
        }
    }

    pub fn curve_references(&self) -> [CurveReference; 8] {
        [
            (0x0a, CurveKind::Integer, i32::from(self.integer_curve_0a)),
            (0x0e, CurveKind::Vector, i32::from(self.position_curve_id)),
            (0x10, CurveKind::Vector, i32::from(self.rotation_curve_id)),
            (0x12, CurveKind::Vector, i32::from(self.scale_curve_id)),
            (0x20, CurveKind::Color, i32::from(self.color_curve_id)),
            (0x22, CurveKind::Vector, i32::from(self.vector_curve_22)),
            (0x24, CurveKind::Integer, i32::from(self.integer_curve_24)),
            (0x26, CurveKind::Vector, i32::from(self.vector_curve_26)),
        ]
        .map(|(offset, kind, id)| CurveReference { offset, kind, id })
    }

    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[..4].copy_from_slice(&self.flags.to_le_bytes());
        [
            self.definition_id,
            self.unknown_06,
            self.unknown_08,
            self.integer_curve_0a,
            self.duration_steps,
            self.position_curve_id,
            self.rotation_curve_id,
            self.scale_curve_id,
        ]
        .encode(&mut bytes[4..20], Endian::Little)
        .expect("fixed definition prefix width");
        bytes[20..32].copy_from_slice(&self.unknown_14);
        [
            self.color_curve_id,
            self.vector_curve_22,
            self.integer_curve_24 as u16,
            self.vector_curve_26,
        ]
        .encode(&mut bytes[32..40], Endian::Little)
        .expect("fixed definition curve width");
        bytes[40..].copy_from_slice(&self.unknown_28);
        bytes
    }
}
