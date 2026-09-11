//! Context-selected stage tables consumed by native 113FD190.
//!
//! 113E8DA0 passes its stage directory's fixed slot 2 to this loader. The
//! original version/control WORDs do not gate the six-table layout in that
//! path, so this parser retains them without inventing a version restriction.

use super::render_tables::RenderTable;
use crate::{Error, Result};

const HEADER_SIZE: usize = 16;
const TABLE_LAYOUT: [(usize, usize); 6] = [(2, 24), (4, 32), (6, 4), (8, 32), (10, 68), (12, 16)];

#[derive(Clone, Debug)]
pub struct LegacyRenderTables<'a> {
    pub version: u16,
    pub control: u16,
    /// Physical order, including all zero-count tables.
    pub tables: Vec<RenderTable<'a>>,
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> LegacyRenderTables<'a> {
    /// Explicit interpretation supplied by the stage-directory slot. There is
    /// no signature or global probe; unknown trailing bytes remain available.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated legacy stage render table header"))?;
        let word = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]);
        let mut tables = Vec::with_capacity(TABLE_LAYOUT.len());
        let mut offset = HEADER_SIZE;
        for (count_offset, record_size) in TABLE_LAYOUT {
            let count = word(count_offset);
            let size = usize::from(count)
                .checked_mul(record_size)
                .ok_or_else(|| Error::new(count_offset, "legacy stage table length overflow"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(count_offset, "legacy stage table range overflow"))?;
            let records = source.get(offset..end).ok_or_else(|| {
                Error::new(count_offset, "legacy stage table records exceed resource")
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
            version: word(0),
            control: word(14),
            tables,
            trailing: &source[offset..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(counts: [u16; 6], version: u16, control: u16) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE];
        bytes[..2].copy_from_slice(&version.to_le_bytes());
        bytes[14..16].copy_from_slice(&control.to_le_bytes());
        for (index, ((count_offset, stride), count)) in
            TABLE_LAYOUT.into_iter().zip(counts).enumerate()
        {
            bytes[count_offset..count_offset + 2].copy_from_slice(&count.to_le_bytes());
            for record in 0..count {
                bytes.extend(std::iter::repeat_n(index as u8 * 16 + record as u8, stride));
            }
        }
        bytes
    }

    #[test]
    fn st001_layout_retains_exact_table_slices_and_trailing_bytes() {
        let counts = [3, 3, 8, 1, 3, 0];
        let mut source = fixture(counts, 1, 0x4321);
        assert_eq!(source.len(), 452);
        source.extend_from_slice(&[0xde, 0xad, 0xbe]);
        let parsed = LegacyRenderTables::parse(&source).unwrap();
        assert_eq!(parsed.version, 1);
        assert_eq!(parsed.control, 0x4321);
        assert_eq!(parsed.tables.len(), 6);
        assert_eq!(parsed.trailing, [0xde, 0xad, 0xbe]);
        assert_eq!(parsed.as_bytes(), source);
        for (index, table) in parsed.tables.iter().enumerate() {
            assert_eq!(table.count_offset, 2 + index * 2);
            assert_eq!(table.record_size, [24, 32, 4, 32, 68, 16][index]);
            assert_eq!(table.count, counts[index]);
            assert_eq!(table.offset, [16, 88, 184, 216, 248, 452][index]);
            assert_eq!(table.records().len(), usize::from(counts[index]));
            assert_eq!(table.records.as_ptr(), source[table.offset..].as_ptr());
            for (record, bytes) in table.records().enumerate() {
                assert!(
                    bytes
                        .iter()
                        .all(|&byte| byte == (index * 16 + record) as u8)
                );
            }
        }
    }

    #[test]
    fn unknown_version_control_and_zero_counts_are_not_reinterpreted() {
        for (version, control) in [(0, 0), (0xffff, 0xabcd), (8, 0xffff)] {
            let source = fixture([0; 6], version, control);
            let parsed = LegacyRenderTables::parse(&source).unwrap();
            assert_eq!((parsed.version, parsed.control), (version, control));
            assert_eq!(parsed.tables.len(), 6);
            assert!(
                parsed
                    .tables
                    .iter()
                    .all(|table| table.offset == 16 && table.records.is_empty())
            );
            assert!(parsed.trailing.is_empty());
            assert_eq!(parsed.as_bytes(), source);
        }
        let source = fixture([0, 0, 0, 0, 0, 2], 0xbeef, 0x1234);
        let parsed = LegacyRenderTables::parse(&source).unwrap();
        assert_eq!(parsed.tables[5].offset, 16);
        assert_eq!(parsed.tables[5].records.len(), 32);
    }

    #[test]
    fn every_truncation_and_oversized_count_is_rejected() {
        let source = fixture([1; 6], 1, 0);
        for length in 0..source.len() {
            assert!(
                LegacyRenderTables::parse(&source[..length]).is_err(),
                "length {length}"
            );
        }
        for count_offset in [2, 4, 6, 8, 10, 12] {
            let mut bad = source.clone();
            bad[count_offset..count_offset + 2].copy_from_slice(&u16::MAX.to_le_bytes());
            assert_eq!(
                LegacyRenderTables::parse(&bad).unwrap_err().offset,
                count_offset
            );
        }
    }
}
