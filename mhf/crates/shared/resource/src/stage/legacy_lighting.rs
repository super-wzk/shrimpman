//! Legacy scene environment parameters consumed by `108E0460`.
//!
//! `1089EF20` passes its outer directory's entry 0; `108E0460` reads that
//! package's entry 2. The small version byte is not a standalone signature.
//! This type therefore has an explicit parser only, with no global probe.

use crate::{Error, Result};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyLightingExtension {
    /// First four words copied by `108E01C0`, relative to resource +122.
    pub words_00: [u32; 4],
    /// Eight packed colors whose bytes the native loader rearranges. Preserve
    /// their original disk words rather than performing that conversion.
    pub color_bits_10: [u32; 8],
}

impl LegacyLightingExtension {
    pub const OFFSET: usize = 122;
    pub const SIZE: usize = 48;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyLightingRecord24 {
    pub offset: usize,
    pub words: [u32; 5],
    pub shorts: [u16; 2],
}

impl LegacyLightingRecord24 {
    pub const SIZE: usize = 24;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyLightingRecord16 {
    pub offset: usize,
    pub words: [u32; 4],
}

impl LegacyLightingRecord16 {
    pub const SIZE: usize = 16;
}

/// Additional version-3 tables consumed by `108E0370`. Their scalar roles are
/// not established; counts, order, unknown words and source offsets stay intact.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LegacyLightingTables {
    pub count_aa: u32,
    pub records_24: Vec<LegacyLightingRecord24>,
    pub count_16_offset: usize,
    pub count_16: u32,
    pub records_16: Vec<LegacyLightingRecord16>,
}

impl LegacyLightingTables {
    pub const OFFSET: usize = 170;

    fn parse(source: &[u8]) -> Result<(Self, usize)> {
        let count_aa = word(
            source
                .get(170..174)
                .ok_or_else(|| Error::new(170, "truncated legacy lighting first table count"))?,
        );
        let count_16_offset = (count_aa as usize)
            .checked_mul(LegacyLightingRecord24::SIZE)
            .and_then(|size| 174usize.checked_add(size))
            .ok_or_else(|| Error::new(170, "legacy lighting first table length overflow"))?;
        let first_bytes = source
            .get(174..count_16_offset)
            .ok_or_else(|| Error::new(170, "legacy lighting first table exceeds resource"))?;
        let second_offset = count_16_offset
            .checked_add(4)
            .ok_or_else(|| Error::new(count_16_offset, "legacy lighting table offset overflow"))?;
        let count_16 = word(source.get(count_16_offset..second_offset).ok_or_else(|| {
            Error::new(
                count_16_offset,
                "truncated legacy lighting second table count",
            )
        })?);
        let end = (count_16 as usize)
            .checked_mul(LegacyLightingRecord16::SIZE)
            .and_then(|size| second_offset.checked_add(size))
            .ok_or_else(|| {
                Error::new(
                    count_16_offset,
                    "legacy lighting second table length overflow",
                )
            })?;
        let second_bytes = source.get(second_offset..end).ok_or_else(|| {
            Error::new(
                count_16_offset,
                "legacy lighting second table exceeds resource",
            )
        })?;
        // Both complete extents are validated before allocating either table.
        let records_24 = first_bytes
            .as_chunks::<{ LegacyLightingRecord24::SIZE }>()
            .0
            .iter()
            .enumerate()
            .map(|(index, record)| LegacyLightingRecord24 {
                offset: 174 + index * LegacyLightingRecord24::SIZE,
                words: std::array::from_fn(|index| word(&record[index * 4..index * 4 + 4])),
                shorts: std::array::from_fn(|index| {
                    u16::from_le_bytes(record[20 + index * 2..22 + index * 2].try_into().unwrap())
                }),
            })
            .collect();
        let records_16 = second_bytes
            .as_chunks::<{ LegacyLightingRecord16::SIZE }>()
            .0
            .iter()
            .enumerate()
            .map(|(index, record)| LegacyLightingRecord16 {
                offset: second_offset + index * LegacyLightingRecord16::SIZE,
                words: std::array::from_fn(|index| word(&record[index * 4..index * 4 + 4])),
            })
            .collect();
        Ok((
            Self {
                count_aa,
                records_24,
                count_16_offset,
                count_16,
                records_16,
            },
            end,
        ))
    }
}

#[derive(Clone, Debug)]
pub struct LegacyLighting<'a> {
    pub version: u8,
    pub unknown_01: u8,
    pub color_02: u32,
    pub value_06_bits: u32,
    pub value_0a_bits: u32,
    /// `108E00C0` copies 108 bytes; `108DE7B0` consumes three groups, each
    /// containing three XYZ vectors. Scalar meanings remain unassigned.
    pub vector_groups_0e_bits: [[[u32; 3]; 3]; 3],
    pub extension: Option<LegacyLightingExtension>,
    pub tables: Option<LegacyLightingTables>,
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> LegacyLighting<'a> {
    pub const BASE_SIZE: usize = 122;

    /// Parse an environment member identified by its native scene context.
    /// Versions 1/2 consume 122/170 bytes; version 3 then reads two counted
    /// tables. Bytes beyond the native consumed extent remain in `trailing`.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let version = *source
            .first()
            .ok_or_else(|| Error::new(0, "truncated legacy lighting version"))?;
        if !matches!(version, 1..=3) {
            return Err(Error::new(0, "unsupported legacy lighting version"));
        }
        let base = source
            .get(..Self::BASE_SIZE)
            .ok_or_else(|| Error::new(0, "truncated legacy lighting base parameters"))?;
        let vector_groups_0e_bits = std::array::from_fn(|group| {
            std::array::from_fn(|vector| {
                std::array::from_fn(|component| {
                    let offset = 14 + (group * 9 + vector * 3 + component) * 4;
                    word(&base[offset..offset + 4])
                })
            })
        });
        let extension = if version >= 2 {
            let bytes = source
                .get(LegacyLightingExtension::OFFSET..LegacyLightingTables::OFFSET)
                .ok_or_else(|| Error::new(122, "truncated legacy lighting extension"))?;
            Some(LegacyLightingExtension {
                words_00: std::array::from_fn(|index| word(&bytes[index * 4..index * 4 + 4])),
                color_bits_10: std::array::from_fn(|index| {
                    word(&bytes[16 + index * 4..20 + index * 4])
                }),
            })
        } else {
            None
        };
        let (tables, end) = if version == 3 {
            let (tables, end) = LegacyLightingTables::parse(source)?;
            (Some(tables), end)
        } else {
            (
                None,
                if version == 2 {
                    LegacyLightingTables::OFFSET
                } else {
                    Self::BASE_SIZE
                },
            )
        };
        Ok(Self {
            version,
            unknown_01: base[1],
            color_02: word(&base[2..6]),
            value_06_bits: word(&base[6..10]),
            value_0a_bits: word(&base[10..14]),
            vector_groups_0e_bits,
            extension,
            tables,
            trailing: &source[end..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(version: u8) -> Vec<u8> {
        let mut bytes = vec![version, 0xa5];
        for value in [0x1234_5678u32, 0x8000_0000, 0x7fc0_1234] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for value in 0..27u32 {
            bytes.extend_from_slice(&(0x7fc0_1000 + value).to_le_bytes());
        }
        if version >= 2 {
            for value in 0..12u32 {
                bytes.extend_from_slice(&(0x1020_30f0 + value).to_le_bytes());
            }
        }
        if version == 3 {
            bytes.extend_from_slice(&2u32.to_le_bytes());
            for index in 0..2u32 {
                for value in 0..5u32 {
                    bytes.extend_from_slice(&(index * 5 + value).to_le_bytes());
                }
                bytes.extend_from_slice(&0xffffu16.to_le_bytes());
                bytes.extend_from_slice(&0x1234u16.to_le_bytes());
            }
            bytes.extend_from_slice(&1u32.to_le_bytes());
            for value in [0x8000_0000u32, 0x7fc0_4321, 0xfedc_ba98, 0] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        bytes
    }

    #[test]
    fn versions_preserve_unaligned_words_color_bytes_and_table_boundaries() {
        for (version, size) in [(1, 122), (2, 170), (3, 242)] {
            let bytes = fixture(version);
            assert_eq!(bytes.len(), size);
            let file = LegacyLighting::parse(&bytes).unwrap();
            assert_eq!(file.version, version);
            assert_eq!(file.unknown_01, 0xa5);
            assert_eq!(file.color_02, 0x1234_5678);
            assert_eq!(file.value_06_bits, 0x8000_0000);
            assert_eq!(file.value_0a_bits, 0x7fc0_1234);
            assert_eq!(file.as_bytes().as_ptr(), bytes.as_ptr());
            assert!(file.trailing.is_empty());
            for (index, &bits) in file
                .vector_groups_0e_bits
                .iter()
                .flatten()
                .flatten()
                .enumerate()
            {
                assert_eq!(bits, 0x7fc0_1000 + index as u32);
            }
            assert_eq!(file.extension.is_some(), version >= 2);
            if let Some(extension) = file.extension {
                assert_eq!(
                    extension.words_00,
                    [0x1020_30f0, 0x1020_30f1, 0x1020_30f2, 0x1020_30f3]
                );
                assert_eq!(extension.color_bits_10[0], 0x1020_30f4);
                assert_eq!(extension.color_bits_10[7], 0x1020_30fb);
            }
            assert_eq!(file.tables.is_some(), version == 3);
            if let Some(tables) = file.tables {
                assert_eq!(tables.count_aa, 2);
                assert_eq!(tables.count_16_offset, 222);
                assert_eq!(tables.count_16, 1);
                assert_eq!(tables.records_24[0].offset, 174);
                assert_eq!(tables.records_24[1].offset, 198);
                assert_eq!(tables.records_24[1].words, [5, 6, 7, 8, 9]);
                assert_eq!(tables.records_24[1].shorts, [0xffff, 0x1234]);
                assert_eq!(tables.records_16[0].offset, 226);
                assert_eq!(
                    tables.records_16[0].words,
                    [0x8000_0000, 0x7fc0_4321, 0xfedc_ba98, 0]
                );
            }
        }
    }

    #[test]
    fn checks_every_truncation_and_count_extent_before_allocation() {
        for version in 1..=3 {
            let bytes = fixture(version);
            for length in 0..bytes.len() {
                assert!(
                    LegacyLighting::parse(&bytes[..length]).is_err(),
                    "version {version}, length {length}"
                );
            }
        }
        for count_offset in [170, 222] {
            let mut bytes = fixture(3);
            bytes[count_offset..count_offset + 4].copy_from_slice(&u32::MAX.to_le_bytes());
            assert!(LegacyLighting::parse(&bytes).is_err());
        }
        let mut bytes = fixture(1);
        for version in [0, 4, 255] {
            bytes[0] = version;
            assert!(LegacyLighting::parse(&bytes).is_err());
        }
    }

    #[test]
    fn keeps_empty_version_three_tables_and_unconsumed_source_bytes() {
        let mut bytes = fixture(2);
        bytes[0] = 3;
        bytes.extend_from_slice(&[0; 8]);
        let file = LegacyLighting::parse(&bytes).unwrap();
        let tables = file.tables.unwrap();
        assert_eq!(tables.count_16_offset, 174);
        assert!(tables.records_24.is_empty());
        assert!(tables.records_16.is_empty());
        assert!(file.trailing.is_empty());
        for version in 1..=3 {
            let mut bytes = fixture(version);
            let end = bytes.len();
            bytes.extend_from_slice(&[0xde, 0xad, 0xbe, 0xef]);
            let file = LegacyLighting::parse(&bytes).unwrap();
            assert_eq!(file.as_bytes(), bytes);
            assert_eq!(file.trailing, [0xde, 0xad, 0xbe, 0xef]);
            assert_eq!(file.trailing.as_ptr(), bytes[end..].as_ptr());
        }
    }
}
