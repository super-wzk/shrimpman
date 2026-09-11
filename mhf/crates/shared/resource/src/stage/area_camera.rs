//! Stage area-camera resources relocated by native 1081FDF0.
//!
//! The stage loader supplies archive member 29 after optional JKR decoding.
//! 1082F860 selects a region through the grid and 1082F910 tests its 64-byte
//! subrecords. Unknown camera parameters remain verbatim; this parser neither
//! relocates file offsets nor identifies arbitrary byte strings by their size.

use crate::{Error, Result};

const HEADER_SIZE: usize = 48;
const REGION_HEADER_SIZE: usize = 768;
const SUBRECORD_SIZE: usize = 64;
const EXTENSION_SIZE: usize = 32;

#[derive(Clone, Debug)]
pub struct AreaCameraRegion<'a> {
    pub index: usize,
    pub offset: usize,
    pub source: &'a [u8],
}

impl<'a> AreaCameraRegion<'a> {
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

#[derive(Clone, Debug)]
pub struct AreaCameraCell<'a> {
    pub index: usize,
    /// Offset of the eight-byte directory record within the resource.
    pub offset: usize,
    pub count: u32,
    /// Original list pointer, relative to the resource's beginning.
    pub relative_offset: u32,
    /// Validated resource offset of `references`, without native relocation.
    pub list_offset: usize,
    /// Verbatim DWORD offsets of regions, relative to the resource beginning.
    pub references: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct AreaCamera<'a> {
    pub source: &'a [u8],
    pub regions: Vec<AreaCameraRegion<'a>>,
    pub cells: Vec<AreaCameraCell<'a>>,
    pub trailing: &'a [u8],
}

impl<'a> AreaCamera<'a> {
    /// Explicit, context-driven parsing of the verified 0x0102 layout. The
    /// native default initializer 1082F740 writes this exact header WORD.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let header = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated stage area-camera header"))?;
        if word(header, 0) != 0x0102 {
            return Err(Error::new(0, "unsupported stage area-camera version"));
        }
        let region_count = usize::from(word(header, 2));
        let minimum_regions = region_count
            .checked_mul(REGION_HEADER_SIZE)
            .and_then(|size| HEADER_SIZE.checked_add(size))
            .ok_or_else(|| Error::new(2, "area-camera region length overflow"))?;
        if minimum_regions > source.len() {
            return Err(Error::new(2, "area-camera region headers exceed resource"));
        }
        let mut regions = Vec::with_capacity(region_count);
        let mut offset = HEADER_SIZE;
        for index in 0..region_count {
            let head_end = end(offset, REGION_HEADER_SIZE, source.len(), offset)?;
            let head = &source[offset..head_end];
            let subrecord_count = usize::from(head[5]);
            let subrecord_offset = dword(head, 24) as usize;
            let extension_offset = dword(head, 28) as usize;
            let subrecord_size = subrecord_count * SUBRECORD_SIZE;
            let size = REGION_HEADER_SIZE
                + subrecord_size
                + if extension_offset == 0 {
                    0
                } else {
                    EXTENSION_SIZE
                };
            let region_end = end(offset, size, source.len(), offset)?;
            if subrecord_count != 0 {
                if subrecord_offset != REGION_HEADER_SIZE {
                    return Err(Error::new(
                        offset + 24,
                        "area-camera subrecords do not follow their header",
                    ));
                }
            } else if subrecord_offset != 0 && subrecord_offset != REGION_HEADER_SIZE {
                return Err(Error::new(
                    offset + 24,
                    "empty area-camera subrecord pointer is invalid",
                ));
            }
            if extension_offset != 0 && extension_offset != REGION_HEADER_SIZE + subrecord_size {
                return Err(Error::new(
                    offset + 28,
                    "area-camera extension does not follow its subrecords",
                ));
            }
            regions.push(AreaCameraRegion {
                index,
                offset,
                source: &source[offset..region_end],
            });
            offset = region_end;
        }

        let cell_count = usize::from(word(header, 4))
            .checked_mul(usize::from(word(header, 6)))
            .ok_or_else(|| Error::new(4, "area-camera grid size overflow"))?;
        let cell_table = dword(header, 28) as usize;
        let reference_table = dword(header, 32) as usize;
        if cell_count == 0 && cell_table == 0 && reference_table == 0 {
            return Ok(Self {
                source,
                regions,
                cells: Vec::new(),
                trailing: &source[offset..],
            });
        }
        let cell_bytes = cell_count
            .checked_mul(8)
            .ok_or_else(|| Error::new(4, "area-camera grid length overflow"))?;
        if cell_table < offset {
            return Err(Error::new(28, "area-camera grid overlaps region records"));
        }
        let grid_end = end(cell_table, cell_bytes, source.len(), 28)?;
        if reference_table < grid_end || reference_table > source.len() {
            return Err(Error::new(
                32,
                "area-camera reference table is outside its payload",
            ));
        }
        let total_references = source[cell_table..grid_end]
            .as_chunks::<8>()
            .0
            .iter()
            .try_fold(0usize, |total, cell| {
                total.checked_add(dword(cell, 0) as usize)
            })
            .ok_or_else(|| Error::new(cell_table, "area-camera reference count overflow"))?;
        let reference_bytes = total_references
            .checked_mul(4)
            .ok_or_else(|| Error::new(32, "area-camera reference length overflow"))?;
        let reference_end = end(reference_table, reference_bytes, source.len(), 32)?;
        // Native relocation touches this whole flat pool, including entries
        // that multiple cells may share. Every value must name a real region.
        for (index, bytes) in source[reference_table..reference_end]
            .as_chunks::<4>()
            .0
            .iter()
            .enumerate()
        {
            let target = dword(bytes, 0) as usize;
            if regions
                .binary_search_by_key(&target, |region| region.offset)
                .is_err()
            {
                return Err(Error::new(
                    reference_table + 4 * index,
                    "area-camera reference does not name a region record",
                ));
            }
        }
        let mut cells = Vec::with_capacity(cell_count);
        for index in 0..cell_count {
            let at = cell_table + index * 8;
            let count = dword(source, at);
            let relative_offset = dword(source, at + 4);
            let list_offset = if count == 0 {
                reference_table
            } else {
                relative_offset as usize
            };
            let references = if count == 0 {
                // Native 1081FDF0 leaves the pointer untouched for an empty cell.
                &source[reference_table..reference_table]
            } else {
                let size = (count as usize)
                    .checked_mul(4)
                    .ok_or_else(|| Error::new(at, "area-camera cell reference length overflow"))?;
                if list_offset < reference_table
                    || !(list_offset - reference_table).is_multiple_of(4)
                {
                    return Err(Error::new(
                        at + 4,
                        "area-camera cell pointer is outside the reference table",
                    ));
                }
                let list_end = end(list_offset, size, reference_end, at + 4)?;
                &source[list_offset..list_end]
            };
            cells.push(AreaCameraCell {
                index,
                offset: at,
                count,
                relative_offset,
                list_offset,
                references,
            });
        }
        Ok(Self {
            source,
            regions,
            cells,
            trailing: &source[reference_end..],
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }
}

fn word(source: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(source[offset..offset + 2].try_into().unwrap())
}

fn dword(source: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(source[offset..offset + 4].try_into().unwrap())
}

fn end(offset: usize, size: usize, limit: usize, field: usize) -> Result<usize> {
    offset
        .checked_add(size)
        .filter(|&end| end <= limit)
        .ok_or_else(|| Error::new(field, "area-camera range exceeds its containing payload"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_word(source: &mut [u8], offset: usize, value: u16) {
        source[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
    }

    fn set_dword(source: &mut [u8], offset: usize, value: u32) {
        source[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn fixture() -> Vec<u8> {
        let mut source = vec![0; 1692];
        set_word(&mut source, 0, 0x0102);
        set_word(&mut source, 2, 2);
        set_word(&mut source, 4, 1);
        set_word(&mut source, 6, 1);
        set_word(&mut source, 8, 20000);
        set_word(&mut source, 10, 20000);
        set_dword(&mut source, 28, 1680);
        set_dword(&mut source, 32, 1688);
        set_dword(&mut source, 36, 0xdead_beef); // Still unverified header data.
        source[816 + 5] = 1;
        set_dword(&mut source, 816 + 24, 768);
        set_dword(&mut source, 816 + 28, 832);
        set_dword(&mut source, 1680, 1);
        set_dword(&mut source, 1684, 1688);
        set_dword(&mut source, 1688, 816);
        source
    }

    #[test]
    fn regions_and_grid_lists_are_bounded_original_slices() {
        let mut source = fixture();
        source.extend_from_slice(&[0xab, 0xcd, 0xef]);
        let file = AreaCamera::parse(&source).unwrap();
        assert_eq!(file.as_bytes(), source);
        assert_eq!(
            file.regions
                .iter()
                .map(|region| (region.offset, region.source.len()))
                .collect::<Vec<_>>(),
            [(48, 768), (816, 864)]
        );
        assert_eq!(file.regions[1].as_bytes().as_ptr(), source[816..].as_ptr());
        let cell = &file.cells[0];
        assert_eq!(
            (
                cell.offset,
                cell.count,
                cell.relative_offset,
                cell.list_offset
            ),
            (1680, 1, 1688, 1688)
        );
        assert_eq!(cell.references.as_ptr(), source[1688..].as_ptr());
        assert_eq!(file.trailing, [0xab, 0xcd, 0xef]);
        assert_eq!(dword(file.source, 36), 0xdead_beef);
    }

    #[test]
    fn truncations_and_invalid_nested_offsets_fail_before_exposing_views() {
        let source = fixture();
        for length in 0..source.len() {
            assert!(
                AreaCamera::parse(&source[..length]).is_err(),
                "length {length}"
            );
        }
        for (offset, value) in [
            (28, 1679),
            (32, 1687),
            (816 + 24, 764),
            (816 + 28, 864),
            (1680, u32::MAX),
            (1684, 1689),
            (1684, u32::MAX),
            (1688, 817),
        ] {
            let mut bad = source.clone();
            set_dword(&mut bad, offset, value);
            assert!(AreaCamera::parse(&bad).is_err(), "field {offset}");
        }
        let mut bad = source;
        set_word(&mut bad, 0, 0x0101);
        assert_eq!(AreaCamera::parse(&bad).unwrap_err().offset, 0);
    }

    #[test]
    fn empty_cells_preserve_stale_pointer_bits_but_expose_an_in_bounds_empty_view() {
        let mut source = fixture();
        set_dword(&mut source, 1680, 0);
        set_dword(&mut source, 1684, u32::MAX);
        let file = AreaCamera::parse(&source).unwrap();
        let cell = &file.cells[0];
        assert_eq!(cell.relative_offset, u32::MAX);
        assert_eq!(cell.list_offset, 1688);
        assert!(cell.references.is_empty());
        assert_eq!(
            cell.references.as_ptr(),
            source[cell.list_offset..].as_ptr()
        );
        for region in &file.regions {
            assert_eq!(
                source.get(region.offset..region.offset + region.source.len()),
                Some(region.source)
            );
        }
        for cell in &file.cells {
            assert!(source.get(cell.offset..cell.offset + 8).is_some());
            assert_eq!(
                source.get(cell.list_offset..cell.list_offset + cell.references.len()),
                Some(cell.references)
            );
        }
    }

    #[test]
    #[ignore = "requires MHF_AREA_CAMERA_SAMPLE pointing to a decoded original stage member"]
    fn original_stage_area_camera_has_six_regions_and_one_grid_cell() {
        let source = std::fs::read(std::env::var_os("MHF_AREA_CAMERA_SAMPLE").unwrap()).unwrap();
        let file = AreaCamera::parse(&source).unwrap();
        assert_eq!(
            file.regions
                .iter()
                .map(|region| region.offset)
                .collect::<Vec<_>>(),
            [48, 816, 1808, 2672, 3536, 4400]
        );
        assert_eq!(file.cells.len(), 1);
        assert_eq!(
            (
                file.cells[0].offset,
                file.cells[0].count,
                file.cells[0].list_offset
            ),
            (5264, 5, 5272)
        );
        assert_eq!(
            file.cells[0]
                .references
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| dword(bytes, 0))
                .collect::<Vec<_>>(),
            [816, 1808, 2672, 3536, 4400]
        );
        assert!(file.trailing.is_empty());
        assert_eq!(file.as_bytes().len(), 5292);
    }
}
