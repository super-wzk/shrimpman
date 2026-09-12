//! HD stage render tables consumed in order by native 11394DA0.
//!
//! The thirteen counts are stored in header order, but the +0x1c count's
//! 16-byte table follows the +0x04 table physically. Record semantics remain
//! numeric until a consuming native path establishes a field's meaning.

use crate::{Error, Result};

const HEADER_SIZE: usize = 32;
const TABLE_LAYOUT: [(usize, usize); 13] = [
    (4, 28),
    (28, 16),
    (6, 40),
    (8, 60),
    (10, 36),
    (12, 32),
    (14, 44),
    (16, 28),
    (18, 52),
    (20, 20),
    (22, 28),
    (24, 12),
    (26, 16),
];

#[derive(Clone, Debug)]
pub struct RenderTable<'a> {
    /// Byte offset of this table's count within its resource header.
    pub count_offset: usize,
    pub record_size: usize,
    pub count: u16,
    /// Byte offset of the table's raw records within this resource.
    pub offset: usize,
    pub records: &'a [u8],
}

impl<'a> RenderTable<'a> {
    pub fn records(&self) -> std::slice::ChunksExact<'a, u8> {
        self.records.chunks_exact(self.record_size)
    }
}

#[derive(Clone, Debug)]
pub struct RenderTables<'a> {
    pub version: u16,
    /// 11394DA0 copies this word to a runtime scalar; its purpose is unknown.
    /// It does not participate in table counts, offsets or record traversal.
    pub unknown_02: u16,
    /// Not consumed by the table setup in 11394DA0. Preserve the original word.
    pub unknown_1e: u16,
    /// Physical table order, retaining tables whose count is zero.
    pub tables: Vec<RenderTable<'a>>,
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> RenderTables<'a> {
    /// Explicit interpretation of the native version-2-or-later layout.
    /// Unknown trailing bytes remain available and are never discarded.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated stage render table header"))?;
        let word = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]);
        let version = word(0);
        if version < 2 {
            return Err(Error::new(
                0,
                "stage render table versions before 2 have an unsupported layout",
            ));
        }
        let mut offset = HEADER_SIZE;
        let mut tables = Vec::with_capacity(TABLE_LAYOUT.len());
        for (count_offset, record_size) in TABLE_LAYOUT {
            let count = word(count_offset);
            let size = usize::from(count)
                .checked_mul(record_size)
                .ok_or_else(|| Error::new(count_offset, "stage render table length overflow"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(count_offset, "stage render table range overflow"))?;
            let records = source.get(offset..end).ok_or_else(|| {
                Error::new(count_offset, "stage render table records exceed resource")
            })?;
            tables.push(RenderTable {
                count_offset,
                record_size,
                count,
                offset,
                records,
            });
            offset = end;
        }
        Ok(Self {
            version,
            unknown_02: word(2),
            unknown_1e: word(30),
            tables,
            trailing: &source[offset..],
            source,
        })
    }

    /// Structural recognition without a signature: require an exact payload
    /// length and at least one record, avoiding arbitrary all-zero headers.
    pub fn probe(source: &'a [u8]) -> Result<Self> {
        let parsed = Self::parse(source)?;
        if !parsed.trailing.is_empty() {
            return Err(Error::new(
                source.len() - parsed.trailing.len(),
                "trailing bytes prevent stage render table recognition",
            ));
        }
        if parsed.tables.iter().all(|table| table.count == 0) {
            return Err(Error::new(
                4,
                "empty counts do not identify stage render tables",
            ));
        }
        Ok(parsed)
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

/// Twelve-byte record in the table whose count is at header +0x18.
/// Native 11395F50 matches +0, +4 and +6; +6 may retain the 0xffff wildcard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MatchRecord {
    pub kind: u8,
    pub unknown_01: u8,
    pub unknown_02: u16,
    pub match_04: u16,
    pub match_06: u16,
    pub value: u32,
}

impl MatchRecord {
    pub const SIZE: usize = 12;

    pub fn parse(record: &[u8]) -> Result<Self> {
        if record.len() != Self::SIZE {
            return Err(Error::new(
                0,
                "stage match record must contain exactly 12 bytes",
            ));
        }
        let word = |offset| u16::from_le_bytes([record[offset], record[offset + 1]]);
        Ok(Self {
            kind: record[0],
            unknown_01: record[1],
            unknown_02: word(2),
            match_04: word(4),
            match_06: word(6),
            value: u32::from_le_bytes(record[8..12].try_into().unwrap()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(counts: &[u16; 13]) -> Vec<u8> {
        let mut source = vec![0; HEADER_SIZE];
        source[..2].copy_from_slice(&2u16.to_le_bytes());
        source[2..4].copy_from_slice(&0x3412u16.to_le_bytes());
        source[30..32].copy_from_slice(&0xcdefu16.to_le_bytes());
        for (index, ((count_offset, record_size), &count)) in
            TABLE_LAYOUT.into_iter().zip(counts).enumerate()
        {
            source[count_offset..count_offset + 2].copy_from_slice(&count.to_le_bytes());
            for record in 0..count {
                source.extend(std::iter::repeat_n(
                    (index as u8).wrapping_add(record as u8),
                    record_size,
                ));
            }
        }
        source
    }

    #[test]
    fn physical_order_and_record_views_preserve_header_and_payload_bytes() {
        let counts = std::array::from_fn(|index| index as u16 + 1);
        let source = fixture(&counts);
        let parsed = RenderTables::probe(&source).unwrap();
        assert_eq!(parsed.version, 2);
        assert_eq!(parsed.unknown_02, 0x3412);
        assert_eq!(parsed.unknown_1e, 0xcdef);
        assert_eq!(parsed.as_bytes(), source);
        assert_eq!(parsed.tables.len(), 13);
        assert_eq!(
            parsed
                .tables
                .iter()
                .map(|table| table.count_offset)
                .collect::<Vec<_>>(),
            [4, 28, 6, 8, 10, 12, 14, 16, 18, 20, 22, 24, 26]
        );
        let mut offset = HEADER_SIZE;
        for (index, table) in parsed.tables.iter().enumerate() {
            assert_eq!(table.offset, offset);
            assert_eq!(table.count, counts[index]);
            assert_eq!(
                table.records.len(),
                usize::from(counts[index]) * table.record_size
            );
            assert_eq!(table.records().len(), usize::from(table.count));
            for (record_index, record) in table.records().enumerate() {
                assert!(
                    record
                        .iter()
                        .all(|&byte| byte == (index + record_index) as u8)
                );
            }
            offset += table.records.len();
        }
        assert_eq!(offset, source.len());
    }

    #[test]
    fn explicit_parse_retains_trailing_and_empty_tables_but_probe_requires_evidence() {
        let empty = fixture(&[0; 13]);
        let parsed = RenderTables::parse(&empty).unwrap();
        assert_eq!(parsed.tables.len(), 13);
        assert!(
            parsed
                .tables
                .iter()
                .all(|table| table.offset == 32 && table.records.is_empty())
        );
        assert!(RenderTables::probe(&empty).is_err());
        let mut counts = [0; 13];
        counts[11] = 1;
        let mut source = fixture(&counts);
        assert_eq!(source.len(), 44);
        assert!(RenderTables::probe(&source).is_ok());
        source.extend_from_slice(&[0xde, 0xad]);
        let parsed = RenderTables::parse(&source).unwrap();
        assert_eq!(parsed.trailing, [0xde, 0xad]);
        assert_eq!(parsed.as_bytes(), source);
        assert_eq!(RenderTables::probe(&source).unwrap_err().offset, 44);
    }

    #[test]
    fn every_truncation_and_unsupported_version_is_rejected() {
        let mut source = fixture(&[1; 13]);
        for length in 0..source.len() {
            assert!(
                RenderTables::parse(&source[..length]).is_err(),
                "length {length}"
            );
        }
        for version in [0u16, 1] {
            source[..2].copy_from_slice(&version.to_le_bytes());
            assert_eq!(RenderTables::parse(&source).unwrap_err().offset, 0);
        }
        source[..2].copy_from_slice(&3u16.to_le_bytes());
        assert!(RenderTables::probe(&source).is_ok());
        source[28..30].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(RenderTables::parse(&source).unwrap_err().offset, 28);
    }

    #[test]
    fn match_record_preserves_unknown_fields_wildcard_and_unsigned_value() {
        let record = [
            3, 0xa5, 0x34, 0x12, 0x78, 0x56, 0xff, 0xff, 0xef, 0xcd, 0xab, 0x89,
        ];
        assert_eq!(
            MatchRecord::parse(&record).unwrap(),
            MatchRecord {
                kind: 3,
                unknown_01: 0xa5,
                unknown_02: 0x1234,
                match_04: 0x5678,
                match_06: 0xffff,
                value: 0x89ab_cdef,
            }
        );
        assert!(MatchRecord::parse(&record[..11]).is_err());
        assert!(MatchRecord::parse(&[0; 13]).is_err());
    }
}
