//! Decoded `mhfemd.bin` species slots. Offsets are relative to the decoded blob.
//!
//! The native loader (10AFA180) relocates `header[4]` records of 192 bytes;
//! 10050530 indexes those records by the actor's species byte. A slot's existence
//! does not establish that it is spawnable or has an exportable AI.

use crate::{Error, Result, binary::Reader};

pub const ROOT_SIZE: usize = 96;
/// Verified prefix, including the table-22 count read at +34 by 10851990.
pub const HEADER_SIZE: usize = 36;
pub const SPECIES_STRIDE: usize = 192;

mod fields;
#[cfg(test)]
mod table_tests;
mod tables;
pub use fields::FieldLayout;
pub use tables::{ROOT_LABELS, RecordKind, Table};

#[derive(Clone, Debug)]
pub struct Emd<'a> {
    bytes: &'a [u8],
    pub header_offset: usize,
    pub species_offset: usize,
    count: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Species<'a> {
    pub id: u8,
    pub offset: usize,
    bytes: &'a [u8],
}

impl<'a> Emd<'a> {
    /// Parse the verified root/header and species-record array. Other tables and
    /// record-internal pointers are retained, not interpreted or relocated.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        checked_range(bytes, 0, ROOT_SIZE)?;
        let reader = Reader::new(bytes);
        let header_offset = reader.read_at::<u32>(0)?.value as usize;
        if header_offset < ROOT_SIZE {
            return Err(Error::new(0, "EMD header overlaps root directory"));
        }
        checked_range(bytes, header_offset, HEADER_SIZE)?;
        let count = reader.read_at::<u8>(header_offset + 4)?.value;
        let species_offset = reader.read_at::<u32>(12)?.value as usize;
        let size = usize::from(count) * SPECIES_STRIDE;
        checked_range(bytes, species_offset, size)?;
        if size != 0
            && (species_offset < ROOT_SIZE
                || (species_offset < header_offset + HEADER_SIZE
                    && header_offset < species_offset + size))
        {
            return Err(Error::new(12, "EMD species records overlap root or header"));
        }
        Ok(Self {
            bytes,
            header_offset,
            species_offset,
            count,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn species_count(&self) -> u8 {
        self.count
    }

    pub fn species(&self) -> impl ExactSizeIterator<Item = Species<'a>> + '_ {
        (0..self.count).map(|id| {
            let offset = self.species_offset + usize::from(id) * SPECIES_STRIDE;
            Species {
                id,
                offset,
                bytes: &self.bytes[offset..offset + SPECIES_STRIDE],
            }
        })
    }
}

impl<'a> Species<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

fn checked_range(bytes: &[u8], offset: usize, size: usize) -> Result<()> {
    if offset.checked_add(size).is_none_or(|end| end > bytes.len()) {
        return Err(Error::new(offset, "EMD range exceeds decoded resource"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(count: u8) -> Vec<u8> {
        let mut bytes = vec![0xa5; 144 + usize::from(count) * SPECIES_STRIDE];
        bytes[..ROOT_SIZE].fill(0);
        bytes[..4].copy_from_slice(&96u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&144u32.to_le_bytes());
        bytes[100] = count;
        bytes
    }

    #[test]
    fn count_not_names_controls_enumeration_and_unknown_bytes_survive() {
        for count in [0, 1, 178, 255] {
            let bytes = sample(count);
            let file = Emd::parse(&bytes).unwrap();
            assert_eq!(file.species().len(), usize::from(count));
            assert_eq!(file.as_bytes(), bytes);
            for (index, species) in file.species().enumerate() {
                assert_eq!(usize::from(species.id), index);
                assert_eq!(species.as_bytes(), &[0xa5; SPECIES_STRIDE]);
            }
        }
    }

    #[test]
    fn rejects_truncation_and_overlapping_records() {
        let bytes = sample(2);
        for end in 0..bytes.len() {
            assert!(Emd::parse(&bytes[..end]).is_err());
        }
        for offset in [0u32, 96, 100, u32::MAX] {
            let mut bytes = bytes.clone();
            bytes[12..16].copy_from_slice(&offset.to_le_bytes());
            assert!(Emd::parse(&bytes).is_err());
        }
    }
}
