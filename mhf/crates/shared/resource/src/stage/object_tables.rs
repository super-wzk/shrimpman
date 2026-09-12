//! Kind-4 object control tables loaded by native 113DA520.
//!
//! Five WORD counts in the 16-byte header describe consecutive tables. The
//! loader advances over the first four; 113DDB90, 113DE180 and 113DE3F0 consume
//! the final table in 16-byte steps. Record bytes remain uninterpreted here.

use crate::{Error, Result};

const HEADER_SIZE: usize = 16;
const TABLE_LAYOUT: [(usize, usize); 5] = [(2, 68), (4, 24), (6, 16), (8, 32), (10, 16)];

#[derive(Clone, Debug)]
pub struct ObjectTable<'a> {
    pub count_offset: usize,
    pub record_size: usize,
    pub count: u16,
    pub offset: usize,
    pub records: &'a [u8],
}

impl<'a> ObjectTable<'a> {
    pub fn records(&self) -> std::slice::ChunksExact<'a, u8> {
        self.records.chunks_exact(self.record_size)
    }
}

#[derive(Clone, Debug)]
pub struct ObjectTables<'a> {
    pub version: u16,
    /// 113DA520 and the five-table readers use counts only at +2 through +10.
    /// No additional table or reference is established by this header word.
    pub unknown_0c: u16,
    /// Not consumed by the known table setup; retained independently of +12.
    pub unknown_0e: u16,
    /// Physical order, including tables whose count is zero.
    pub tables: Vec<ObjectTable<'a>>,
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> ObjectTables<'a> {
    /// Parse the version-8-or-later layout after resolving any package resource
    /// reference and decoding envelopes. Preserve unknown trailing bytes.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated object control table header"))?;
        let word = |offset| u16::from_le_bytes([header[offset], header[offset + 1]]);
        let version = word(0);
        if version < 8 {
            return Err(Error::new(
                0,
                "object control table versions before 8 have an unsupported layout",
            ));
        }
        let mut offset = HEADER_SIZE;
        let mut tables = Vec::with_capacity(TABLE_LAYOUT.len());
        for (count_offset, record_size) in TABLE_LAYOUT {
            let count = word(count_offset);
            let size = usize::from(count)
                .checked_mul(record_size)
                .ok_or_else(|| Error::new(count_offset, "object control table length overflow"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(count_offset, "object control table range overflow"))?;
            let records = source.get(offset..end).ok_or_else(|| {
                Error::new(count_offset, "object control table records exceed resource")
            })?;
            tables.push(ObjectTable {
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
            unknown_0c: word(12),
            unknown_0e: word(14),
            tables,
            trailing: &source[offset..],
            source,
        })
    }

    /// Structural recognition requires an exact length and at least one record.
    /// The containing object package's kind-4 descriptor supplies the context.
    pub fn probe(source: &'a [u8]) -> Result<Self> {
        let parsed = Self::parse(source)?;
        if !parsed.trailing.is_empty() {
            return Err(Error::new(
                source.len() - parsed.trailing.len(),
                "trailing bytes prevent object control table recognition",
            ));
        }
        if parsed.tables.iter().all(|table| table.count == 0) {
            return Err(Error::new(
                2,
                "empty counts do not identify object control tables",
            ));
        }
        Ok(parsed)
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(counts: [u16; 5]) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE];
        bytes[..2].copy_from_slice(&8u16.to_le_bytes());
        bytes[12..14].copy_from_slice(&0x1234u16.to_le_bytes());
        bytes[14..16].copy_from_slice(&0x5678u16.to_le_bytes());
        for (table, ((count_offset, stride), count)) in
            TABLE_LAYOUT.into_iter().zip(counts).enumerate()
        {
            bytes[count_offset..count_offset + 2].copy_from_slice(&count.to_le_bytes());
            for record in 0..count {
                bytes.extend(std::iter::repeat_n(table as u8 * 16 + record as u8, stride));
            }
        }
        bytes
    }

    #[test]
    fn all_five_tables_keep_native_strides_offsets_and_raw_records() {
        let source = fixture([1, 2, 3, 4, 5]);
        let parsed = ObjectTables::probe(&source).unwrap();
        assert_eq!(parsed.version, 8);
        assert_eq!(parsed.unknown_0c, 0x1234);
        assert_eq!(parsed.unknown_0e, 0x5678);
        assert_eq!(parsed.as_bytes(), source);
        assert_eq!(parsed.tables.len(), 5);
        let mut offset = HEADER_SIZE;
        for (index, table) in parsed.tables.iter().enumerate() {
            assert_eq!(table.count_offset, 2 + index * 2);
            assert_eq!(table.record_size, [68, 24, 16, 32, 16][index]);
            assert_eq!(table.count, index as u16 + 1);
            assert_eq!(table.offset, offset);
            assert_eq!(table.records().len(), usize::from(table.count));
            for (record, bytes) in table.records().enumerate() {
                assert!(
                    bytes
                        .iter()
                        .all(|&byte| byte == (index * 16 + record) as u8)
                );
            }
            offset += table.records.len();
        }
        assert_eq!(offset, source.len());
        assert_eq!(parsed.tables[4].records.len(), 80);
    }

    #[test]
    fn empty_tables_and_unknown_tails_remain_inspectable_without_false_probes() {
        let empty = fixture([0; 5]);
        let parsed = ObjectTables::parse(&empty).unwrap();
        assert!(
            parsed
                .tables
                .iter()
                .all(|table| table.offset == 16 && table.records.is_empty())
        );
        assert!(ObjectTables::probe(&empty).is_err());
        let mut source = fixture([0, 0, 0, 0, 2]);
        assert_eq!(source.len(), 48);
        let parsed = ObjectTables::probe(&source).unwrap();
        assert_eq!(parsed.tables[4].offset, 16);
        assert_eq!(parsed.tables[4].records().len(), 2);
        source.extend_from_slice(&[0xde, 0xad, 0xbe]);
        let parsed = ObjectTables::parse(&source).unwrap();
        assert_eq!(parsed.trailing, [0xde, 0xad, 0xbe]);
        assert_eq!(parsed.as_bytes(), source);
        assert_eq!(ObjectTables::probe(&source).unwrap_err().offset, 48);
    }

    #[test]
    fn every_truncation_and_older_version_is_rejected_before_exposing_records() {
        let mut source = fixture([1; 5]);
        for length in 0..source.len() {
            assert!(
                ObjectTables::parse(&source[..length]).is_err(),
                "length {length}"
            );
        }
        for version in 0u16..8 {
            source[..2].copy_from_slice(&version.to_le_bytes());
            assert_eq!(ObjectTables::parse(&source).unwrap_err().offset, 0);
        }
        source[..2].copy_from_slice(&9u16.to_le_bytes());
        assert!(ObjectTables::probe(&source).is_ok());
        source[10..12].copy_from_slice(&u16::MAX.to_le_bytes());
        assert_eq!(ObjectTables::parse(&source).unwrap_err().offset, 10);
    }
}
