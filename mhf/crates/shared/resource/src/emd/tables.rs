//! Table extents derived from native indexing, never from adjacent offsets.
use std::ops::Range;

use super::{Emd, HEADER_SIZE, ROOT_SIZE, SPECIES_STRIDE, checked_range};
use crate::{Error, Result, binary::Reader};

pub const ROOT_LABELS: [&str; 24] = [
    "头部",
    "部位参数配置目录",
    "物种固定参数",
    "物种记录",
    "80 字节参数配置目录",
    "物种显示分类",
    "三项指针目录",
    "物种检索记录",
    "table_08",
    "参数指针目录",
    "90 字节参数配置目录",
    "部位索引映射",
    "table_12",
    "物种与配置修正",
    "三键倍率记录",
    "32 字节记录组计数",
    "32 字节记录组目录",
    "带指针记录",
    "物种八项数值记录",
    "物种关联记录",
    "table_20",
    "物种分类键",
    "物种修正记录",
    "table_23",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RecordKind {
    Header,
    Species,
    Pointers,
    PartParameters,
    FixedParameters,
    Parameters80,
    Classification,
    SpeciesLookup,
    Parameters90,
    PartMap,
    Modifiers,
    SpeciesModifiers,
    KeyedMultiplier,
    Count,
    PointerRecord,
    ParameterLink,
    SpeciesValues,
    SpeciesAssociation,
    ActionRule,
    Category,
    GroupRecord,
}

#[derive(Clone, Debug)]
pub struct Table<'a> {
    pub range: Range<usize>,
    pub count: usize,
    pub stride: usize,
    pub kind: RecordKind,
    /// Present only for the zero-terminated root-10 directory.
    pub terminator: Option<Range<usize>>,
    source: &'a [u8],
}

impl<'a> Table<'a> {
    pub fn record(&self, index: usize) -> Result<(usize, &'a [u8])> {
        if index >= self.count {
            return Err(Error::new(
                self.range.start,
                "EMD record index out of range",
            ));
        }
        let offset = index
            .checked_mul(self.stride)
            .and_then(|offset| self.range.start.checked_add(offset))
            .ok_or_else(|| Error::new(self.range.start, "EMD record offset overflow"))?;
        checked_range(self.source, offset, self.stride)?;
        Ok((offset, &self.source[offset..offset + self.stride]))
    }
}

impl<'a> Emd<'a> {
    pub fn root_offset(&self, slot: usize) -> Result<usize> {
        if slot >= ROOT_LABELS.len() {
            return Err(Error::new(0, "EMD root slot out of range"));
        }
        Ok(Reader::new(self.bytes).read_at::<u32>(slot * 4)?.value as usize)
    }

    fn header_count(&self, offset: usize) -> Result<usize> {
        Ok(Reader::new(self.bytes)
            .read_at::<u16>(self.header_offset + offset)?
            .value as usize)
    }

    fn table(
        &self,
        offset: usize,
        count: usize,
        stride: usize,
        kind: RecordKind,
    ) -> Result<Table<'a>> {
        let size = count
            .checked_mul(stride)
            .ok_or_else(|| Error::new(offset, "EMD table size overflow"))?;
        checked_range(self.bytes, offset, size)?;
        if size != 0
            && (offset < ROOT_SIZE
                || (kind != RecordKind::Header
                    && offset < self.header_offset + HEADER_SIZE
                    && self.header_offset < offset + size))
        {
            return Err(Error::new(offset, "EMD table overlaps root or header"));
        }
        Ok(Table {
            range: offset..offset + size,
            count,
            stride,
            kind,
            terminator: None,
            source: self.bytes,
        })
    }

    /// Validate one known table on demand. An unknown layout returns None;
    /// callers can still display the original root offset. Other
    /// tables can be inspected even if this one is damaged.
    pub fn root_table(&self, slot: usize) -> Result<Option<Table<'a>>> {
        use RecordKind::*;
        let offset = self.root_offset(slot)?;
        let species = usize::from(self.count);
        let (count, stride, kind) = match slot {
            0 => (1, HEADER_SIZE, Header),
            1 | 4 => (12, 4, Pointers),
            2 => (species, 52, FixedParameters),
            3 => (species, SPECIES_STRIDE, Species),
            5 => (species, 6, Classification),
            6 => (3, 4, Pointers),
            7 => (self.header_count(12)?, 12, SpeciesLookup),
            9 => (self.header_count(16)?, 4, Pointers),
            10 => {
                if offset < ROOT_SIZE {
                    return Err(Error::new(offset, "EMD directory overlaps root"));
                }
                let reader = Reader::new(self.bytes);
                let mut end = offset;
                while reader.read_at::<u32>(end)?.value != 0 {
                    end = end
                        .checked_add(4)
                        .ok_or_else(|| Error::new(end, "EMD directory overflow"))?;
                }
                // Validate the terminator too, including for an empty directory.
                let mut table = self.table(offset, (end - offset) / 4 + 1, 4, Pointers)?;
                table.count -= 1;
                table.range.end = end;
                table.terminator = Some(end..end + 4);
                return Ok(Some(table));
            }
            11 => (species, 18, PartMap),
            13 => (self.header_count(18)?, 28, Modifiers),
            14 => (self.header_count(20)?, 12, KeyedMultiplier),
            15 => (self.header_count(22)?, 2, Count),
            16 => (self.header_count(22)?, 4, Pointers),
            17 => (self.header_count(24)?, 8, PointerRecord),
            18 => (
                usize::from(self.bytes[self.header_offset + 26]),
                18,
                SpeciesValues,
            ),
            19 => (self.header_count(28)?, 32, SpeciesAssociation),
            21 => (species, 2, Category),
            22 => (self.header_count(34)?, 28, SpeciesModifiers),
            // Relocated by the loader, but no complete extent has been proved.
            8 | 12 | 20 | 23 => return Ok(None),
            _ => unreachable!("root_offset checked the slot"),
        };
        self.table(offset, count, stride, kind).map(Some)
    }

    /// Follow only directories whose target record layout and cardinality have
    /// both been established. Aliases are allowed; offsets are blob-relative.
    pub fn directory_table(&self, slot: usize, index: usize) -> Result<Option<Table<'a>>> {
        use RecordKind::*;
        let (count, stride, kind) = match slot {
            1 => (usize::from(self.count), 34, PartParameters),
            3 => {
                let records = self
                    .root_table(3)?
                    .expect("species table has a known layout");
                let (at, _) = records.record(index)?;
                let offset = Reader::new(self.bytes).read_at::<u32>(at + 184)?.value as usize;
                if offset == 0 {
                    return Ok(None);
                }
                // The loader visits exactly 200 pairs. The second DWORD gates
                // relocation but has not been established as a target count.
                return self.table(offset, 200, 8, ParameterLink).map(Some);
            }
            4 => (usize::from(self.count), 80, Parameters80),
            10 => (usize::from(self.count), 90, Parameters90),
            19 => {
                let records = self
                    .root_table(19)?
                    .ok_or_else(|| Error::new(76, "EMD species association table is absent"))?;
                let (at, _) = records.record(index)?;
                let reader = Reader::new(self.bytes);
                let count = usize::from(reader.read_at::<u16>(at + 26)?.value);
                let offset = reader.read_at::<u32>(at + 28)?.value as usize;
                // The loader relocates this link only when BOTH fields are nonzero.
                if count == 0 || offset == 0 {
                    return Ok(None);
                }
                return self.table(offset, count, 4, ActionRule).map(Some);
            }
            16 => {
                let counts = self
                    .root_table(15)?
                    .ok_or_else(|| Error::new(60, "EMD record counts are absent"))?;
                let (at, _) = counts.record(index)?;
                (
                    Reader::new(self.bytes).read_at::<u16>(at)?.value as usize,
                    32,
                    GroupRecord,
                )
            }
            _ => {
                return Err(Error::new(
                    slot.saturating_mul(4),
                    "EMD target layout is not established",
                ));
            }
        };
        let directory = self
            .root_table(slot)?
            .ok_or_else(|| Error::new(slot * 4, "EMD directory is absent"))?;
        let (at, _) = directory.record(index)?;
        let offset = Reader::new(self.bytes).read_at::<u32>(at)?.value as usize;
        // These directories are relocated unconditionally, unlike nullable
        // species links. A zero offset with nonzero count is malformed, not null.
        self.table(offset, count, stride, kind).map(Some)
    }
}
