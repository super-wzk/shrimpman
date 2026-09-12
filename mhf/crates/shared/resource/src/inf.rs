//! INF v6 quest directories and text references before native relocation.
//!
//! 10AFAE80 validates the header and relocates categories, quest pointers and
//! eight text pointers. Quest records have independent pointers, not a proven
//! fixed stride; only the prefix containing the known fields is exposed here.

use std::ops::Range;

use crate::{Error, Result, binary::Reader};

pub const MAGIC: &[u8; 4] = b"inf\x1a";
pub const VERSION: u32 = 6;
pub const HEADER_SIZE: usize = 136;

/// The quest layout shared with the runtime text-resource catalog.
#[derive(Clone, Copy, Debug)]
pub struct QuestLayout {
    pub root_field: u32,
    pub count_root_field: u32,
    pub category_stride: u16,
    pub category_count_field: u16,
    pub category_records_field: u16,
    pub record_text_field: u16,
    pub record_id_field: u16,
    pub parts: u16,
}

impl QuestLayout {
    /// Minimum known prefix; this does not establish the complete record size.
    pub fn quest_prefix_size(self) -> usize {
        (usize::from(self.record_text_field) + 4).max(usize::from(self.record_id_field) + 2)
    }

    fn validate(self) -> Result<()> {
        let stride = usize::from(self.category_stride);
        let count = usize::from(self.category_count_field);
        let records = usize::from(self.category_records_field);
        let text = usize::from(self.record_text_field);
        let id = usize::from(self.record_id_field);
        if self.root_field < 16
            || self.count_root_field < 16
            || self.root_field as usize > HEADER_SIZE - 4
            || self.count_root_field as usize > HEADER_SIZE - 4
            || self.root_field == self.count_root_field
            || count < 2
            || records < count + 2
            || records + 4 > stride
            || text < id + 2 && id < text + 4
            || self.parts == 0
        {
            return Err(Error::new(0, "invalid INF quest layout"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Inf<'a> {
    source: &'a [u8],
    pub unknown_08: u32,
    pub layout: QuestLayout,
}

#[derive(Clone, Debug)]
pub struct Category<'a> {
    pub index: usize,
    pub offset: usize,
    /// 1047A820 selects the first category whose limit is >= the requested ID;
    /// a zero limit stops the scan and uses that category's slot table as well.
    /// Categories retain physical file order.
    pub quest_id_limit: u16,
    pub slot_count: u16,
    pub slots_offset: u32,
    source: &'a [u8],
}

impl Category<'_> {
    pub fn as_bytes(&self) -> &[u8] {
        self.source
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QuestSlot {
    /// Offset of the stored pointer within the decoded INF image.
    pub field: usize,
    /// Zero marks an absent slot. Nonzero offsets are not normalized or deduped.
    pub offset: u32,
}

#[derive(Clone, Debug)]
pub struct Quest<'a> {
    pub offset: usize,
    /// Read as a u16 by 1042D440 and other quest-ID consumers.
    pub quest_id: u16,
    pub text_table_offset: u32,
    prefix: &'a [u8],
}

impl Quest<'_> {
    /// Exact known prefix, not a complete standalone quest resource.
    pub fn prefix(&self) -> &[u8] {
        self.prefix
    }
}

#[derive(Clone, Debug)]
pub struct Text<'a> {
    pub pointer_field: usize,
    pub offset: usize,
    /// Original bytes excluding the NUL terminator. Decoding is a UI concern.
    pub bytes: &'a [u8],
}

impl<'a> Inf<'a> {
    /// Identify the fixed INF v6 header. Referenced tables are checked on demand
    /// so a malformed category or string does not hide unrelated records.
    pub fn parse(source: &'a [u8], layout: QuestLayout) -> Result<Self> {
        let header = source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "truncated INF header"))?;
        let reader = Reader::new(header);
        if reader.read_at::<[u8; 4]>(0)?.value != *MAGIC {
            return Err(Error::new(0, "invalid INF magic"));
        }
        if reader.read_at::<u32>(4)?.value != VERSION {
            return Err(Error::new(4, "unsupported INF version (expected 6)"));
        }
        if reader.read_at::<u32>(12)?.value as usize != HEADER_SIZE {
            return Err(Error::new(12, "unsupported INF root header size"));
        }
        layout.validate()?;
        Ok(Self {
            source,
            unknown_08: reader.read_at::<u32>(8)?.value,
            layout,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn u32(&self, offset: usize) -> Result<u32> {
        Reader::new(self.source)
            .read_at::<u32>(offset)
            .map(|field| field.value)
    }

    /// All references use the decoded INF image as their base.
    pub fn pointer(&self, field: usize) -> Result<Option<usize>> {
        let offset = self.u32(field)? as usize;
        if offset == 0 {
            return Ok(None);
        }
        if !(HEADER_SIZE..self.source.len()).contains(&offset) {
            return Err(Error::new(field, "INF pointer outside data"));
        }
        Ok(Some(offset))
    }

    pub fn category_count(&self) -> Result<usize> {
        match self.pointer(self.layout.count_root_field as usize)? {
            Some(offset) => Reader::new(self.source)
                .read_at::<u16>(offset)
                .map(|field| usize::from(field.value)),
            None => Ok(0),
        }
    }

    pub fn category_range(&self) -> Result<Range<usize>> {
        self.table_range(
            self.layout.root_field as usize,
            self.category_count()?,
            usize::from(self.layout.category_stride),
        )
    }

    pub fn category(&self, index: usize) -> Result<Category<'a>> {
        let range = self.category_range()?;
        let stride = usize::from(self.layout.category_stride);
        if index >= range.len() / stride {
            return Err(Error::new(
                self.layout.root_field as usize,
                "INF category index out of range",
            ));
        }
        let offset = range.start + index * stride;
        let source = &self.source[offset..offset + stride];
        let reader = Reader::with_base(source, offset);
        Ok(Category {
            index,
            offset,
            quest_id_limit: reader.read_at::<u16>(0)?.value,
            slot_count: reader
                .read_at::<u16>(usize::from(self.layout.category_count_field))?
                .value,
            slots_offset: reader
                .read_at::<u32>(usize::from(self.layout.category_records_field))?
                .value,
            source,
        })
    }

    pub fn slots_range(&self, category: &Category<'_>) -> Result<Range<usize>> {
        self.table_range(
            category.offset + usize::from(self.layout.category_records_field),
            usize::from(category.slot_count),
            4,
        )
    }

    pub fn slot(&self, category: &Category<'_>, index: usize) -> Result<QuestSlot> {
        if index >= usize::from(category.slot_count) {
            return Err(Error::new(
                category.offset + usize::from(self.layout.category_count_field),
                "INF quest slot index out of range",
            ));
        }
        let range = self.slots_range(category)?;
        let field = range.start + index * 4;
        Ok(QuestSlot {
            field,
            offset: self.u32(field)?,
        })
    }

    /// Read only the prefix required by the confirmed ID and text fields.
    pub fn quest(&self, offset: u32) -> Result<Quest<'a>> {
        let offset = offset as usize;
        let size = self.layout.quest_prefix_size();
        let prefix = self.data_range(offset, size, offset)?;
        let reader = Reader::with_base(prefix, offset);
        Ok(Quest {
            offset,
            quest_id: reader
                .read_at::<u16>(usize::from(self.layout.record_id_field))?
                .value,
            text_table_offset: reader
                .read_at::<u32>(usize::from(self.layout.record_text_field))?
                .value,
            prefix,
        })
    }

    pub fn text_table_range(&self, quest: &Quest<'_>) -> Result<Option<Range<usize>>> {
        let field = quest.offset + usize::from(self.layout.record_text_field);
        if self.u32(field)? == 0 {
            return Ok(None);
        }
        self.table_range(field, usize::from(self.layout.parts), 4)
            .map(Some)
    }

    pub fn text(&self, quest: &Quest<'_>, part: usize) -> Result<Option<Text<'a>>> {
        let table_field = quest.offset + usize::from(self.layout.record_text_field);
        if part >= usize::from(self.layout.parts) {
            return Err(Error::new(table_field, "INF text part index out of range"));
        }
        let Some(table) = self.text_table_range(quest)? else {
            return Ok(None);
        };
        let field = table.start + part * 4;
        let Some(offset) = self.pointer(field)? else {
            return Ok(None);
        };
        let length = self.source[offset..]
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| Error::new(offset, "unterminated INF string"))?;
        Ok(Some(Text {
            pointer_field: field,
            offset,
            bytes: &self.source[offset..offset + length],
        }))
    }

    /// Bounded form of 1047A820's static lookup. IDs >= 40000 use a different
    /// native resource and are not resolved by INF. No category sorting or
    /// record renumbering is performed. A zero limit ends the category scan,
    /// then still indexes that category's slots. Unlike native unchecked reads,
    /// exhausting the declared categories or slots reports a bounds error.
    pub fn lookup(&self, quest_id: u16) -> Result<Option<Quest<'a>>> {
        if quest_id >= 40000 {
            return Ok(None);
        }
        for index in 0..self.category_count()? {
            let category = self.category(index)?;
            if category.quest_id_limit == 0 || quest_id <= category.quest_id_limit {
                let slot = self.slot(&category, usize::from(quest_id % 100))?;
                return if slot.offset == 0 {
                    Ok(None)
                } else {
                    self.quest(slot.offset).map(Some)
                };
            }
        }
        Err(Error::new(
            self.layout.root_field as usize,
            "INF quest lookup exceeds declared category table",
        ))
    }

    fn table_range(&self, field: usize, count: usize, stride: usize) -> Result<Range<usize>> {
        // An empty table has no referenced bytes; retain stale offsets without
        // dereferencing them, just as an empty directory slot is retained.
        if count == 0 {
            return Ok(0..0);
        }
        let size = count
            .checked_mul(stride)
            .ok_or_else(|| Error::new(field, "INF table size overflow"))?;
        let offset = self
            .pointer(field)?
            .ok_or_else(|| Error::new(field, "missing INF table pointer"))?;
        self.data_range(offset, size, field)?;
        Ok(offset..offset + size)
    }

    fn data_range(&self, offset: usize, size: usize, field: usize) -> Result<&'a [u8]> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| Error::new(field, "INF data range overflow"))?;
        if offset < HEADER_SIZE {
            return Err(Error::new(field, "INF data overlaps its header"));
        }
        self.source
            .get(offset..end)
            .ok_or_else(|| Error::new(field, "INF data range outside image"))
    }
}
