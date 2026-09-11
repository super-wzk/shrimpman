//! Fixed 60-byte scene-object placements consumed by native 113E8DA0.
//!
//! The resource ID at +54 matches the ID preceding each additional entry in
//! StageArchive. Source words are retained even when their runtime use is unknown.

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Placement {
    pub offset: usize,
    pub vector_00_bits: [u32; 3],
    pub vector_0c_bits: [u32; 3],
    pub unknown_18: u32,
    pub unknown_1c: u32,
    pub vector_20_bits: [u32; 4],
    pub unknown_30: u16,
    pub unknown_32: u16,
    pub unknown_34: u16,
    pub resource_id: u16,
    pub unknown_38: u32,
}

impl Placement {
    pub const SIZE: usize = 60;
}

#[derive(Clone, Debug)]
pub struct PlacementTable<'a> {
    pub version: u32,
    pub count: u32,
    pub unknown_08: u32,
    pub unknown_0c: u32,
    pub placements: Vec<Placement>,
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> PlacementTable<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..16)
            .ok_or_else(|| Error::new(0, "truncated stage placement header"))?;
        let word = |bytes: &[u8]| u32::from_le_bytes(bytes.try_into().unwrap());
        let short = |bytes: &[u8]| u16::from_le_bytes(bytes.try_into().unwrap());
        let version = word(&header[..4]);
        if version != 2 {
            return Err(Error::new(0, "unsupported stage placement table version"));
        }
        let count = word(&header[4..8]);
        let end = (count as usize)
            .checked_mul(Placement::SIZE)
            .and_then(|size| size.checked_add(16))
            .ok_or_else(|| Error::new(4, "stage placement count overflow"))?;
        let records = source
            .get(16..end)
            .ok_or_else(|| Error::new(4, "stage placements exceed resource"))?;
        let placements = records
            .as_chunks::<{ Placement::SIZE }>()
            .0
            .iter()
            .enumerate()
            .map(|(index, record)| Placement {
                offset: 16 + index * Placement::SIZE,
                vector_00_bits: std::array::from_fn(|index| {
                    word(&record[index * 4..index * 4 + 4])
                }),
                vector_0c_bits: std::array::from_fn(|index| {
                    word(&record[12 + index * 4..16 + index * 4])
                }),
                unknown_18: word(&record[24..28]),
                unknown_1c: word(&record[28..32]),
                vector_20_bits: std::array::from_fn(|index| {
                    word(&record[32 + index * 4..36 + index * 4])
                }),
                unknown_30: short(&record[48..50]),
                unknown_32: short(&record[50..52]),
                unknown_34: short(&record[52..54]),
                resource_id: short(&record[54..56]),
                unknown_38: word(&record[56..60]),
            })
            .collect();
        Ok(Self {
            version,
            count,
            unknown_08: word(&header[8..12]),
            unknown_0c: word(&header[12..16]),
            placements,
            trailing: &source[end..],
            source,
        })
    }

    /// A signatureless table is recognized only with its complete record extent
    /// and the +8 sentinel observed in all 225 audited stage placement tables.
    pub fn probe(source: &'a [u8]) -> Result<Self> {
        let table = Self::parse(source)?;
        if table.unknown_08 != u32::MAX {
            return Err(Error::new(8, "missing observed stage placement sentinel"));
        }
        if !table.trailing.is_empty() {
            return Err(Error::new(
                source.len() - table.trailing.len(),
                "bytes remain after stage placements",
            ));
        }
        Ok(table)
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> Vec<u8> {
        let mut bytes = [2u32, 2, u32::MAX, 8]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        for id in [77u16, 87] {
            let mut record = [0; Placement::SIZE];
            record[..4].copy_from_slice(&0x8000_0000u32.to_le_bytes());
            record[12..16].copy_from_slice(&0x7fc0_1234u32.to_le_bytes());
            record[54..56].copy_from_slice(&id.to_le_bytes());
            record[56..60].copy_from_slice(&0xfedc_ba98u32.to_le_bytes());
            bytes.extend_from_slice(&record);
        }
        bytes
    }

    #[test]
    fn preserves_resource_ids_float_bits_and_original_record_offsets() {
        let bytes = fixture();
        let table = PlacementTable::probe(&bytes).unwrap();
        assert_eq!(
            table
                .placements
                .iter()
                .map(|record| record.resource_id)
                .collect::<Vec<_>>(),
            [77, 87]
        );
        assert_eq!(table.placements[1].offset, 76);
        assert_eq!(table.placements[0].vector_00_bits[0], 0x8000_0000);
        assert_eq!(table.placements[0].vector_0c_bits[0], 0x7fc0_1234);
        assert_eq!(table.placements[1].unknown_38, 0xfedc_ba98);
        assert_eq!(table.as_bytes(), bytes);
    }

    #[test]
    fn rejects_incomplete_tables_without_overidentifying_arbitrary_headers() {
        let bytes = fixture();
        for length in 0..bytes.len() {
            assert!(PlacementTable::parse(&bytes[..length]).is_err());
        }
        let mut changed = bytes.clone();
        changed[4..8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(PlacementTable::parse(&changed).is_err());
        let mut changed = bytes;
        changed[8..12].fill(0);
        assert!(PlacementTable::parse(&changed).is_ok());
        assert!(PlacementTable::probe(&changed).is_err());
    }
}
