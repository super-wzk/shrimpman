//! HITS collision grids before native pointer relocation.
//!
//! 108C78A0 and 108C7960 resolve the cell directory and each terminated cell
//! list relative to file +8; list members are relative to the record table.
//! 108C7B90 copies 56-byte records, and 108CBE30 uses their three vertices and
//! four plane coefficients. Metadata bits and the two unused header words are
//! retained without assigning surface or gameplay meanings.

use std::collections::BTreeMap;

use crate::{Error, Result};

pub const MAGIC: [u8; 4] = *b"HITS";
pub const HEADER_SIZE: usize = 40;
pub const RECORD_SIZE: usize = 56;
const RELATIVE_BASE: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HitsHeader {
    pub size: u32,
    /// Integer cell dimensions used to divide query x/z coordinates.
    pub cell_size_x: u32,
    pub cell_size_z: u32,
    pub cells_x: u32,
    pub cells_z: u32,
    pub unknown_18: u32,
    pub unknown_1c: u32,
    /// Encoded offsets are relative to file +8, not the file start.
    pub cell_table_offset: u32,
    pub record_table_offset: u32,
}

#[derive(Clone, Debug)]
pub struct HitCell<'a> {
    /// Native directory ordinal: z + x * header.cells_z.
    pub index: usize,
    pub relative_offset: u32,
    /// Absolute byte offset of this cell's first list word in the resource.
    pub offset: usize,
    /// Original record-relative offset words, excluding the 0xffffffff sentinel.
    pub references: &'a [u8],
}

impl HitCell<'_> {
    pub fn record_offsets(&self) -> impl ExactSizeIterator<Item = u32> + '_ {
        self.references
            .as_chunks::<4>()
            .0
            .iter()
            .map(|bytes| u32::from_le_bytes(*bytes))
    }

    pub fn record_indices(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.record_offsets()
            .map(|offset| offset as usize / RECORD_SIZE)
    }
}

#[derive(Clone, Debug)]
pub struct HitRecord<'a> {
    pub index: usize,
    pub offset: usize,
    pub unknown_00: u32,
    pub vertices: [[f32; 3]; 3],
    /// Stored x/y/z/constant coefficients, with no normalization or sign change.
    pub plane: [f32; 4],
    source: &'a [u8],
}

impl<'a> HitRecord<'a> {
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[derive(Clone, Debug)]
pub struct Hits<'a> {
    pub header: HitsHeader,
    pub cell_table_offset: usize,
    pub records_offset: usize,
    pub cells: Vec<HitCell<'a>>,
    pub records: Vec<HitRecord<'a>>,
    /// Bytes after the size declared by the HITS header.
    pub trailing: &'a [u8],
    pub source: &'a [u8],
}

impl<'a> Hits<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        if !source.starts_with(&MAGIC) {
            return Err(Error::new(0, "expected HITS collision signature"));
        }
        let header_bytes = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated HITS collision header"))?;
        let header = HitsHeader {
            size: word(header_bytes, 4),
            cell_size_x: word(header_bytes, 8),
            cell_size_z: word(header_bytes, 12),
            cells_x: word(header_bytes, 16),
            cells_z: word(header_bytes, 20),
            unknown_18: word(header_bytes, 24),
            unknown_1c: word(header_bytes, 28),
            cell_table_offset: word(header_bytes, 32),
            record_table_offset: word(header_bytes, 36),
        };
        let size = header.size as usize;
        if size < HEADER_SIZE {
            return Err(Error::new(4, "HITS size is smaller than its header"));
        }
        let bytes = source
            .get(..size)
            .ok_or_else(|| Error::new(4, "HITS size exceeds the resource"))?;
        let cell_table_offset = relative_offset(header.cell_table_offset, 32)?;
        let records_offset = relative_offset(header.record_table_offset, 36)?;
        if cell_table_offset < HEADER_SIZE || !cell_table_offset.is_multiple_of(4) {
            return Err(Error::new(
                32,
                "HITS cell directory overlaps its header or is unaligned",
            ));
        }
        if records_offset > size || !records_offset.is_multiple_of(4) {
            return Err(Error::new(
                36,
                "HITS record table is outside the resource or unaligned",
            ));
        }
        let cell_count = (header.cells_x as usize)
            .checked_mul(header.cells_z as usize)
            .ok_or_else(|| Error::new(16, "HITS cell count overflow"))?;
        let cell_bytes = cell_count
            .checked_mul(4)
            .ok_or_else(|| Error::new(16, "HITS cell directory length overflow"))?;
        let lists_offset = cell_table_offset
            .checked_add(cell_bytes)
            .ok_or_else(|| Error::new(32, "HITS cell directory range overflow"))?;
        if lists_offset > records_offset {
            return Err(Error::new(
                32,
                "HITS cell directory overlaps its record table",
            ));
        }
        if cell_count != 0 && (header.cell_size_x == 0 || header.cell_size_z == 0) {
            return Err(Error::new(
                8,
                "HITS cell dimensions cannot be zero for a nonempty grid",
            ));
        }
        let record_bytes = &bytes[records_offset..];
        if !record_bytes.len().is_multiple_of(RECORD_SIZE) {
            return Err(Error::new(
                records_offset,
                "HITS record table ends with a partial 56-byte record",
            ));
        }

        let mut cells = Vec::new();
        cells
            .try_reserve_exact(cell_count)
            .map_err(|_| Error::new(16, "cannot allocate HITS cell directory"))?;
        let mut list_ends = BTreeMap::new();
        for index in 0..cell_count {
            let field = cell_table_offset + index * 4;
            let relative = word(bytes, field);
            let offset = relative_offset(relative, field)?;
            if offset < lists_offset || offset >= records_offset || !offset.is_multiple_of(4) {
                return Err(Error::new(
                    field,
                    "HITS cell list is outside the list region or unaligned",
                ));
            }
            cells.push(HitCell {
                index,
                relative_offset: relative,
                offset,
                references: &[],
            });
            list_ends.insert(offset, 0usize);
        }
        // Validate each physical list span once. Aliases and lists sharing a
        // suffix retain their own directory entries without repeated scanning
        // or allocating a copy of every reference for every cell.
        let mut next_list: Option<(usize, usize)> = None;
        for (&start, end) in list_ends.iter_mut().rev() {
            let mut cursor = start;
            loop {
                if let Some((next_start, next_end)) = next_list
                    && cursor == next_start
                {
                    *end = next_end;
                    break;
                }
                if cursor >= records_offset {
                    return Err(Error::new(
                        start,
                        "HITS cell list has no terminating 0xffffffff",
                    ));
                }
                let reference = word(bytes, cursor);
                if reference == u32::MAX {
                    *end = cursor;
                    break;
                }
                let reference = reference as usize;
                if !reference.is_multiple_of(RECORD_SIZE) || reference >= record_bytes.len() {
                    return Err(Error::new(
                        cursor,
                        "HITS cell reference does not address a complete record",
                    ));
                }
                cursor += 4;
            }
            next_list = Some((start, *end));
        }
        for cell in &mut cells {
            cell.references = &bytes[cell.offset..list_ends[&cell.offset]];
        }
        let mut records = Vec::new();
        records
            .try_reserve_exact(record_bytes.len() / RECORD_SIZE)
            .map_err(|_| Error::new(records_offset, "cannot allocate HITS record views"))?;
        for (index, raw) in record_bytes.as_chunks::<RECORD_SIZE>().0.iter().enumerate() {
            let float = |offset| f32::from_bits(word(raw, offset));
            records.push(HitRecord {
                index,
                offset: records_offset + index * RECORD_SIZE,
                unknown_00: word(raw, 0),
                vertices: std::array::from_fn(|vertex| {
                    std::array::from_fn(|axis| float(4 + vertex * 12 + axis * 4))
                }),
                plane: std::array::from_fn(|axis| float(40 + axis * 4)),
                source: raw,
            });
        }
        Ok(Self {
            header,
            cell_table_offset,
            records_offset,
            cells,
            records,
            trailing: &source[size..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

fn word(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}

fn relative_offset(encoded: u32, field: usize) -> Result<usize> {
    RELATIVE_BASE
        .checked_add(encoded as usize)
        .ok_or_else(|| Error::new(field, "HITS relative offset overflow"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn put(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture(cells: &[&[u32]], record_count: usize) -> Vec<u8> {
        let mut bytes = vec![0; HEADER_SIZE + cells.len() * 4];
        bytes[..4].copy_from_slice(&MAGIC);
        for (offset, value) in [
            (8, 501),
            (12, 500),
            (16, 1),
            (20, cells.len() as u32),
            (24, 0x1234_5678),
            (28, 0x8765_4321),
            (32, 32),
        ] {
            put(&mut bytes, offset, value);
        }
        for (index, references) in cells.iter().enumerate() {
            let offset = (bytes.len() - RELATIVE_BASE) as u32;
            put(&mut bytes, HEADER_SIZE + index * 4, offset);
            for &record in *references {
                bytes.extend_from_slice(&(record * RECORD_SIZE as u32).to_le_bytes());
            }
            bytes.extend_from_slice(&u32::MAX.to_le_bytes());
        }
        let records_offset = (bytes.len() - RELATIVE_BASE) as u32;
        put(&mut bytes, 36, records_offset);
        for index in 0..record_count {
            bytes.extend_from_slice(&(0x0102_0000 | index as u32).to_le_bytes());
            for value in (0..9).map(|axis| (index * 10 + axis) as f32) {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
            for value in [0.0, 1.0, 0.0, -(index as f32)] {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        let size = bytes.len() as u32;
        put(&mut bytes, 4, size);
        bytes
    }

    #[test]
    fn resolves_both_relative_bases_and_keeps_raw_geometry_and_metadata() {
        let source = fixture(&[&[0, 1], &[], &[1]], 3);
        let parsed = Hits::parse(&source).unwrap();
        assert_eq!(parsed.as_bytes(), source);
        assert_eq!(parsed.header.cell_size_x, 501);
        assert_eq!(parsed.header.cell_size_z, 500);
        assert_eq!((parsed.header.cells_x, parsed.header.cells_z), (1, 3));
        assert_eq!(parsed.header.unknown_18, 0x1234_5678);
        assert_eq!(parsed.header.unknown_1c, 0x8765_4321);
        assert_eq!(parsed.cell_table_offset, 40);
        assert_eq!(parsed.cells[0].offset, 52);
        assert_eq!(parsed.cells[0].relative_offset, 44);
        assert_eq!(
            parsed.cells[0].record_offsets().collect::<Vec<_>>(),
            [0, 56]
        );
        assert_eq!(parsed.cells[0].record_indices().collect::<Vec<_>>(), [0, 1]);
        assert_eq!(parsed.cells[1].record_indices().len(), 0);
        assert_eq!(parsed.cells[2].index, 2);
        assert_eq!(parsed.records.len(), 3); // Unreferenced records remain visible.
        assert_eq!(parsed.records[1].unknown_00, 0x0102_0001);
        assert_eq!(
            parsed.records[1].vertices,
            [[10.0, 11.0, 12.0], [13.0, 14.0, 15.0], [16.0, 17.0, 18.0]]
        );
        assert_eq!(parsed.records[1].plane, [0.0, 1.0, 0.0, -1.0]);
        assert_eq!(
            parsed.records[1].offset,
            parsed.records_offset + RECORD_SIZE
        );
        assert_eq!(
            parsed.records[1].as_bytes(),
            &source[parsed.records_offset + RECORD_SIZE..parsed.records_offset + 2 * RECORD_SIZE]
        );
    }

    #[test]
    fn shared_list_suffixes_empty_cells_and_source_tails_are_retained() {
        let mut source = fixture(&[&[0, 1], &[], &[1], &[0]], 2);
        let first = word(&source, 40);
        put(&mut source, 44, first + 8); // Shares the first list's sentinel.
        put(&mut source, 48, first + 4); // Shares its second reference and sentinel.
        put(&mut source, 52, first); // Exact alias.
        source.extend_from_slice(&[0xde, 0xad, 0xbe]);
        let parsed = Hits::parse(&source).unwrap();
        assert_eq!(parsed.cells[0].references, parsed.cells[3].references);
        assert_eq!(parsed.cells[1].references, []);
        assert_eq!(parsed.cells[2].record_indices().collect::<Vec<_>>(), [1]);
        assert_eq!(parsed.trailing, [0xde, 0xad, 0xbe]);
        assert_eq!(parsed.as_bytes(), source);
        let empty = fixture(&[], 0);
        let parsed = Hits::parse(&empty).unwrap();
        assert!(parsed.cells.is_empty());
        assert!(parsed.records.is_empty());
    }

    #[test]
    fn truncation_bad_ranges_and_missing_list_sentinels_are_rejected() {
        let source = fixture(&[&[0], &[]], 1);
        for length in 0..source.len() {
            assert!(Hits::parse(&source[..length]).is_err(), "length {length}");
        }
        for (offset, value) in [
            (0, 0),
            (4, 39),
            (8, 0),
            (16, u32::MAX),
            (20, u32::MAX),
            (32, 0),
            (32, u32::MAX),
            (36, 0),
            (36, u32::MAX),
            (40, 32),
            (40, 1),
            (40, word(&source, 36)),
        ] {
            let mut bad = source.clone();
            put(&mut bad, offset, value);
            assert!(Hits::parse(&bad).is_err(), "field {offset:#x} = {value:#x}");
        }
        let lists = word(&source, 40) as usize + RELATIVE_BASE;
        for reference in [1, RECORD_SIZE as u32] {
            let mut bad = source.clone();
            put(&mut bad, lists, reference);
            assert_eq!(Hits::parse(&bad).unwrap_err().offset, lists);
        }
        let mut bad = source.clone();
        let record_offset = word(&bad, 36) as usize + RELATIVE_BASE;
        for offset in (lists..record_offset).step_by(4) {
            put(&mut bad, offset, 0);
        }
        assert!(
            Hits::parse(&bad)
                .unwrap_err()
                .message
                .contains("terminating")
        );
        let mut bad = source;
        bad.pop();
        let size = bad.len() as u32;
        put(&mut bad, 4, size);
        assert!(
            Hits::parse(&bad)
                .unwrap_err()
                .message
                .contains("partial 56-byte")
        );
    }

    #[test]
    fn encoded_float_bits_are_not_normalized_or_rejected() {
        let mut source = fixture(&[&[0]], 1);
        let record = word(&source, 36) as usize + RELATIVE_BASE;
        put(&mut source, record + 4, 0x7fc0_3141);
        put(&mut source, record + 40, 0x8000_0000);
        let parsed = Hits::parse(&source).unwrap();
        assert_eq!(parsed.records[0].vertices[0][0].to_bits(), 0x7fc0_3141);
        assert_eq!(parsed.records[0].plane[0].to_bits(), 0x8000_0000);
        assert_eq!(parsed.as_bytes(), source);
    }
}
