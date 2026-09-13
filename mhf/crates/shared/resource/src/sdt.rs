//! SDT attack, collision and category-specific parameters before relocation.
//!
//! 10AFCD10 loads `mhfsdt.bin`. The decoded image has no magic/header: it
//! starts with 28-byte directory records, terminated by a kind of 0xffff.
//! References are offsets from this image, not offsets in its ECD/JKR envelope.

use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Range,
};

use crate::{Error, Result, binary::ScalarType};

mod attack;
mod auxiliary;
mod effect_parameters;
mod hitbox;
mod parameters;
mod weapon_parameters;

pub const DIRECTORY_STRIDE: usize = 28;
pub const ATTACK_STRIDE: usize = 40;
pub const AUXILIARY_STRIDE: usize = 16;
pub const HITBOX_GROUP_STRIDE: usize = 32;
pub const HITBOX_SLOTS: usize = 8;
pub const HITBOX_STRIDE: usize = 40;
pub const EXTRA_STRIDE: usize = 32;

#[derive(Clone, Copy, Debug)]
pub struct FieldLayout {
    pub name: &'static str,
    pub offset: u16,
    pub scalar: ScalarType,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TableKind {
    Attack,
    Auxiliary,
    Extra,
}

impl TableKind {
    pub const fn stride(self) -> usize {
        match self {
            Self::Attack => ATTACK_STRIDE,
            Self::Auxiliary => AUXILIARY_STRIDE,
            Self::Extra => EXTRA_STRIDE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Physical directory index; the native loader sorts a separate runtime
    /// copy. Neither lookup nor parsing renumbers the original file.
    pub index: usize,
    pub offset: usize,
    pub subtype: u16,
    pub kind: u16,
    /// Shared upper bound used by native attack and optional auxiliary lookups.
    /// Auxiliary windows may alias/overlap other tables; they have no separate
    /// physical record count in this directory.
    pub record_count: u16,
    pub hitbox_group_count: u16,
    pub attacks_offset: u32,
    pub auxiliary_offset: u32,
    pub hitboxes_offset: u32,
    pub extra_offset: u32,
    pub extra_count: u32,
}

#[derive(Clone, Debug)]
pub struct Sdt<'a> {
    source: &'a [u8],
    entries: Vec<Entry>,
    terminator: Range<usize>,
}

#[derive(Clone, Debug)]
pub struct Table<'a> {
    pub kind: TableKind,
    pub range: Range<usize>,
    pub count: usize,
    pub stride: usize,
    category: u16,
    source: &'a [u8],
}

impl<'a> Table<'a> {
    pub fn record(&self, index: usize) -> Result<Record<'a>> {
        let (offset, source) = record(self.source, self.range.start, self.stride, index)?;
        let fields = match self.kind {
            TableKind::Attack => attack::FIELDS,
            TableKind::Auxiliary => auxiliary::fields(self.category, index),
            TableKind::Extra => parameters::fields(self.category, index, source, self.source),
        };
        Ok(Record {
            offset,
            source,
            fields,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Record<'a> {
    pub offset: usize,
    source: &'a [u8],
    fields: &'static [FieldLayout],
}

impl<'a> Record<'a> {
    pub fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn fields(&self) -> &'static [FieldLayout] {
        self.fields
    }
}

#[derive(Clone, Debug)]
pub struct HitboxGroup {
    pub index: usize,
    pub offset: usize,
    /// Every original slot is retained, including aliases to the same list.
    pub slots: [u32; HITBOX_SLOTS],
}

#[derive(Clone, Debug)]
pub struct HitboxList<'a> {
    /// Records only; the sentinel's consumed WORD is exposed separately.
    pub range: Range<usize>,
    pub count: usize,
    pub terminator: Range<usize>,
    source: &'a [u8],
}

#[derive(Clone, Copy)]
struct HitboxEnd {
    /// First sentinel or first unreadable record on this 40-byte stride.
    offset: usize,
    error: Option<&'static str>,
}

impl<'a> HitboxList<'a> {
    pub fn record(&self, index: usize) -> Result<Record<'a>> {
        let (offset, source) = record(self.source, self.range.start, HITBOX_STRIDE, index)?;
        Ok(Record {
            offset,
            source,
            fields: hitbox::fields(source),
        })
    }
}

impl<'a> Sdt<'a> {
    /// Read the physical directory. Referenced data is checked by accessors,
    /// so one damaged pointer does not hide unrelated categories or tables.
    /// Without a trusted resource context, use the stronger `probe` instead.
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        Self::directory(source, source.len(), false)
    }

    fn directory(source: &'a [u8], limit: usize, check_unique_keys: bool) -> Result<Self> {
        let directory = &source[..limit];
        let mut entries = Vec::new();
        let mut keys = check_unique_keys.then(BTreeSet::new);
        let mut offset = 0;
        loop {
            let prefix = directory
                .get(offset..offset + 4)
                .ok_or_else(|| Error::new(offset, "missing SDT directory terminator"))?;
            let kind = u16::from_le_bytes(prefix[2..4].try_into().unwrap());
            if kind == u16::MAX {
                return Ok(Self {
                    source,
                    entries,
                    terminator: offset..offset + 4,
                });
            }
            let bytes = directory
                .get(offset..offset + DIRECTORY_STRIDE)
                .ok_or_else(|| Error::new(offset, "truncated SDT directory record"))?;
            let word = |at| u16::from_le_bytes(bytes[at..at + 2].try_into().unwrap());
            let dword = |at| u32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
            if let Some(keys) = &mut keys
                && !keys.insert((kind, word(0)))
            {
                return Err(Error::new(offset, "duplicate SDT category key"));
            }
            entries.push(Entry {
                index: entries.len(),
                offset,
                subtype: word(0),
                kind,
                record_count: word(4),
                hitbox_group_count: word(6),
                attacks_offset: dword(8),
                auxiliary_offset: dword(12),
                hitboxes_offset: dword(16),
                extra_offset: dword(20),
                extra_count: dword(24),
            });
            offset += DIRECTORY_STRIDE;
        }
    }

    /// Recognize a headerless image by validating its complete directory and
    /// referenced tables/lists. Empty or pointer-free data is not evidence of
    /// SDT. Shared references are legitimate and are validated only once.
    pub fn probe(source: &'a [u8]) -> Result<Self> {
        // Most inspected assets are not SDT. Reject impossible first records
        // before scanning/allocating a directory from an arbitrary large image.
        let first = source
            .get(..DIRECTORY_STRIDE)
            .ok_or_else(|| Error::new(0, "truncated SDT directory record"))?;
        let word = |offset| u16::from_le_bytes(first[offset..offset + 2].try_into().unwrap());
        let dword =
            |offset| u32::from_le_bytes(first[offset..offset + 4].try_into().unwrap()) as usize;
        let mut limit = source.len();
        let mut has_reference = false;
        for (field, count, stride, optional) in [
            (8, usize::from(word(4)), ATTACK_STRIDE, false),
            (12, usize::from(word(4)), AUXILIARY_STRIDE, true),
            (16, usize::from(word(6)), HITBOX_GROUP_STRIDE, false),
            (20, dword(24), EXTRA_STRIDE, false),
        ] {
            let pointer = dword(field);
            if pointer == 0 && (optional || count == 0) {
                continue;
            }
            if pointer < DIRECTORY_STRIDE + 4
                || pointer % 4 != 0
                || count
                    .checked_mul(stride)
                    .and_then(|size| pointer.checked_add(size))
                    .is_none_or(|end| end > source.len())
            {
                return Err(Error::new(field, "invalid initial SDT table reference"));
            }
            has_reference |= count != 0;
            limit = limit.min(pointer);
        }
        if !has_reference {
            return Err(Error::new(
                0,
                "initial directory record does not identify SDT",
            ));
        }
        let file = Self::directory(source, limit, true)?;
        if file.entries.is_empty() {
            return Err(Error::new(0, "empty directory does not identify SDT"));
        }
        let mut hitboxes = BTreeMap::new();
        let mut has_records = false;
        for entry in &file.entries {
            for (field, pointer) in [
                (8, entry.attacks_offset),
                (12, entry.auxiliary_offset),
                (16, entry.hitboxes_offset),
                (20, entry.extra_offset),
            ] {
                if pointer != 0 && pointer % 4 != 0 {
                    return Err(Error::new(
                        entry.offset + field,
                        "unaligned SDT table reference",
                    ));
                }
            }
            for kind in [TableKind::Attack, TableKind::Auxiliary, TableKind::Extra] {
                if let Some(table) = file.table(entry, kind)? {
                    has_records |= table.count != 0;
                }
            }
            if file.hitbox_groups(entry)?.is_some() {
                for index in 0..usize::from(entry.hitbox_group_count) {
                    let group = file.hitbox_group(entry, index)?;
                    for slot in 0..HITBOX_SLOTS {
                        hitboxes
                            .entry(group.slots[slot])
                            .or_insert(group.offset + slot * 4);
                    }
                }
            }
        }
        if !has_records {
            return Err(Error::new(0, "SDT directory has no parameter records"));
        }
        for list in file.hitbox_lists(hitboxes) {
            list?;
        }
        Ok(file)
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    pub fn entry(&self, index: usize) -> Result<&Entry> {
        self.entries
            .get(index)
            .ok_or_else(|| Error::new(0, "SDT directory index out of range"))
    }

    pub fn terminator(&self) -> Range<usize> {
        self.terminator.clone()
    }

    pub fn table(&self, entry: &Entry, kind: TableKind) -> Result<Option<Table<'a>>> {
        let (field, pointer, count, optional) = match kind {
            TableKind::Attack => (
                8,
                entry.attacks_offset,
                usize::from(entry.record_count),
                false,
            ),
            TableKind::Auxiliary => (
                12,
                entry.auxiliary_offset,
                usize::from(entry.record_count),
                true,
            ),
            TableKind::Extra => (20, entry.extra_offset, entry.extra_count as usize, false),
        };
        if pointer == 0 && (optional || count == 0) {
            return Ok(None);
        }
        let stride = kind.stride();
        let range = self.table_range(entry.offset + field, pointer, count, stride)?;
        Ok(Some(Table {
            kind,
            source: &self.source[range.clone()],
            range,
            count,
            stride,
            category: entry.kind,
        }))
    }

    pub fn hitbox_groups(&self, entry: &Entry) -> Result<Option<Range<usize>>> {
        if entry.hitboxes_offset == 0 && entry.hitbox_group_count == 0 {
            return Ok(None);
        }
        self.table_range(
            entry.offset + 16,
            entry.hitboxes_offset,
            usize::from(entry.hitbox_group_count),
            HITBOX_GROUP_STRIDE,
        )
        .map(Some)
    }

    pub fn hitbox_group(&self, entry: &Entry, index: usize) -> Result<HitboxGroup> {
        if index >= usize::from(entry.hitbox_group_count) {
            return Err(Error::new(
                entry.offset + 6,
                "SDT hitbox group index out of range",
            ));
        }
        let range = self
            .hitbox_groups(entry)?
            .ok_or_else(|| Error::new(entry.offset + 16, "missing SDT hitbox group table"))?;
        let offset = range.start + index * HITBOX_GROUP_STRIDE;
        let mut slots = [0; HITBOX_SLOTS];
        for (index, slot) in slots.iter_mut().enumerate() {
            let at = offset + index * 4;
            *slot = u32::from_le_bytes(self.source[at..at + 4].try_into().unwrap());
        }
        Ok(HitboxGroup {
            index,
            offset,
            slots,
        })
    }

    pub fn hitboxes(&self, group: &HitboxGroup, slot: usize) -> Result<HitboxList<'a>> {
        let pointer = *group
            .slots
            .get(slot)
            .ok_or_else(|| Error::new(group.offset, "SDT hitbox slot out of range"))?;
        let field = group.offset + slot * 4;
        // The native loader relocates all eight pointers, even zero. Zero is
        // not a documented absent slot: an empty list has a real sentinel.
        let range = self.table_range(field, pointer, 1, 2)?;
        let start = range.start;
        self.hitbox_list(field, start, self.hitbox_end(start))
    }

    /// Sorted offsets allow one cached boundary per residue modulo the record
    /// stride. A later start in that interval shares its terminal result,
    /// including an invalid tail; starts beyond it begin a new scan. Neither
    /// source slots nor their independently addressed list views are merged.
    fn hitbox_lists(
        &self,
        references: BTreeMap<u32, usize>,
    ) -> impl Iterator<Item = Result<HitboxList<'a>>> + '_ {
        let mut ends: [Option<HitboxEnd>; HITBOX_STRIDE] = [None; HITBOX_STRIDE];
        references.into_iter().map(move |(pointer, field)| {
            let start = self.table_range(field, pointer, 1, 2)?.start;
            let cached = &mut ends[start % HITBOX_STRIDE];
            let end = match *cached {
                Some(end) if start <= end.offset => end,
                _ => {
                    let end = self.hitbox_end(start);
                    *cached = Some(end);
                    end
                }
            };
            self.hitbox_list(field, start, end)
        })
    }

    fn hitbox_list(&self, field: usize, start: usize, end: HitboxEnd) -> Result<HitboxList<'a>> {
        if let Some(error) = end.error {
            return Err(Error::new(field, error));
        }
        Ok(HitboxList {
            range: start..end.offset,
            count: (end.offset - start) / HITBOX_STRIDE,
            terminator: end.offset..end.offset + 2,
            source: &self.source[start..end.offset],
        })
    }

    fn hitbox_end(&self, start: usize) -> HitboxEnd {
        let mut offset = start;
        loop {
            let Some(bytes) = self.source.get(offset..offset + 2) else {
                return HitboxEnd {
                    offset,
                    error: Some("unterminated SDT hitbox list"),
                };
            };
            if bytes == [0xff, 0xff] {
                return HitboxEnd {
                    offset,
                    error: None,
                };
            }
            if self.source.get(offset..offset + HITBOX_STRIDE).is_none() {
                return HitboxEnd {
                    offset,
                    error: Some("truncated SDT hitbox record"),
                };
            }
            offset += HITBOX_STRIDE;
        }
    }

    /// Regions outside successfully resolved records/references. Padding and
    /// damaged or unknown blocks remain available without inventing a layout.
    /// Call on demand: collecting this view visits every unique hitbox list.
    pub fn unclaimed_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = std::iter::once(0..self.terminator.end).collect::<Vec<_>>();
        let mut hitboxes = BTreeMap::new();
        for entry in &self.entries {
            for kind in [TableKind::Attack, TableKind::Auxiliary, TableKind::Extra] {
                if let Ok(Some(table)) = self.table(entry, kind) {
                    ranges.push(table.range);
                }
            }
            if let Ok(Some(range)) = self.hitbox_groups(entry) {
                ranges.push(range);
                for index in 0..usize::from(entry.hitbox_group_count) {
                    let Ok(group) = self.hitbox_group(entry, index) else {
                        continue;
                    };
                    for slot in 0..HITBOX_SLOTS {
                        hitboxes
                            .entry(group.slots[slot])
                            .or_insert(group.offset + slot * 4);
                    }
                }
            }
        }
        for list in self.hitbox_lists(hitboxes).flatten() {
            ranges.push(list.range);
            ranges.push(list.terminator);
        }
        ranges.sort_unstable_by_key(|range| range.start);
        let mut cursor = 0;
        let mut gaps = Vec::new();
        for range in ranges {
            if cursor < range.start {
                gaps.push(cursor..range.start);
            }
            cursor = cursor.max(range.end);
        }
        if cursor < self.source.len() {
            gaps.push(cursor..self.source.len());
        }
        gaps
    }

    fn table_range(
        &self,
        field: usize,
        pointer: u32,
        count: usize,
        stride: usize,
    ) -> Result<Range<usize>> {
        let offset = pointer as usize;
        if offset < self.terminator.end || offset > self.source.len() {
            return Err(Error::new(field, "SDT reference outside data area"));
        }
        let end = count
            .checked_mul(stride)
            .and_then(|size| offset.checked_add(size))
            .filter(|&end| end <= self.source.len())
            .ok_or_else(|| Error::new(field, "SDT table exceeds resource"))?;
        Ok(offset..end)
    }
}

fn record(source: &[u8], base: usize, stride: usize, index: usize) -> Result<(usize, &[u8])> {
    if index >= source.len() / stride {
        return Err(Error::new(base, "SDT record index out of range"));
    }
    let offset = index * stride;
    Ok((base + offset, &source[offset..offset + stride]))
}
