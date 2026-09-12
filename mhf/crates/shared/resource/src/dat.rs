//! Version 89 core game data. Offsets are relative to the decoded DAT image.
//! This is a pointer-based database, not an offset/size archive. See
//! `docs/dat-format.md` for the native consumers and the supported layouts.

use std::ops::Range;

use crate::{Error, Result, binary::Reader};

mod effects;
mod schema;
pub use effects::{EFFECT_TABLES, EffectRecordKind};
pub use schema::{DATA_TABLES, FieldLayout};

pub const MAGIC: &[u8; 4] = b"mhf\x1a";
pub const VERSION: u32 = 89;
pub const HEADER_SIZE: usize = 3016;

#[derive(Clone, Copy, Debug)]
pub enum RecordCount {
    Fixed(u32),
    U16(&'static [u32]),
    U32(&'static [u32]),
    Sentinel {
        root: &'static [u32],
        stride: u16,
        offset: u16,
        width: u8,
        value: u32,
    },
}

#[derive(Clone, Copy, Debug)]
pub enum RecordFormat {
    Fields(&'static [FieldLayout]),
    Text { offset: u16, parts: u16 },
    Effect(EffectRecordKind),
}

#[derive(Clone, Copy, Debug)]
pub struct TableLayout {
    pub id: &'static str,
    pub label: &'static str,
    pub root: &'static [u32],
    pub first_record: u32,
    pub records: RecordCount,
    pub stride: u16,
    pub format: RecordFormat,
    pub directory: Option<(u32, RecordCount)>,
    /// A parallel string pointer table, when its indexing has been established.
    pub names: Option<u32>,
}

#[derive(Clone, Copy, Debug)]
pub struct Dat<'a> {
    source: &'a [u8],
    /// Header +0x08 is neither read nor relocated by the DAT initialization
    /// path in 10AF5140. No pointer, count or checksum semantics are established;
    /// the outer ECD header's +0x08 belongs to a different resource layer.
    pub unknown_08: u32,
}

#[derive(Clone, Debug)]
pub struct Table<'a> {
    pub layout: &'static TableLayout,
    pub range: Range<usize>,
    pub count: usize,
    pub root_field: Option<usize>,
    pub terminator: Option<Range<usize>>,
    source: &'a [u8],
}

impl<'a> Table<'a> {
    pub fn record(&self, index: usize) -> Result<(usize, &'a [u8])> {
        if index >= self.count {
            return Err(Error::new(
                self.range.start,
                "DAT record index out of range",
            ));
        }
        let offset = self.range.start + index * usize::from(self.layout.stride);
        Ok((
            offset,
            &self.source[offset..offset + usize::from(self.layout.stride)],
        ))
    }
}

impl<'a> Dat<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        if source.get(..4) != Some(MAGIC) {
            return Err(Error::new(0, "invalid DAT magic"));
        }
        let file = Self {
            source,
            unknown_08: 0,
        };
        if file.u32(4)? != VERSION {
            return Err(Error::new(4, "unsupported DAT version (expected 89)"));
        }
        if file.u32(12)? as usize != HEADER_SIZE {
            return Err(Error::new(12, "unsupported DAT root header size"));
        }
        file.bytes(0, HEADER_SIZE)?;
        Ok(Self {
            unknown_08: file.u32(8)?,
            ..file
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn bytes(&self, offset: usize, size: usize) -> Result<&'a [u8]> {
        let end = offset
            .checked_add(size)
            .ok_or_else(|| Error::new(offset, "DAT range overflow"))?;
        self.source
            .get(offset..end)
            .ok_or_else(|| Error::new(offset, "DAT range outside image"))
    }

    pub fn u16(&self, offset: usize) -> Result<u16> {
        Reader::new(self.source)
            .read_at::<u16>(offset)
            .map(|field| field.value)
    }

    pub fn u32(&self, offset: usize) -> Result<u32> {
        Reader::new(self.source)
            .read_at::<u32>(offset)
            .map(|field| field.value)
    }

    /// Resolve each intermediate pointer, preserving null as an absent path.
    pub fn field(&self, path: &[u32]) -> Result<Option<usize>> {
        let Some((&last, parents)) = path.split_last() else {
            return Err(Error::new(0, "empty DAT field path"));
        };
        let mut base = 0usize;
        for &offset in parents {
            let at = add(base, offset as usize)?;
            let Some(target) = self.pointer(at)? else {
                return Ok(None);
            };
            base = target;
        }
        Ok(Some(add(base, last as usize)?))
    }

    pub fn pointer(&self, field: usize) -> Result<Option<usize>> {
        let offset = self.u32(field)? as usize;
        if offset == 0 {
            return Ok(None);
        }
        if offset < HEADER_SIZE || offset >= self.source.len() {
            return Err(Error::new(
                field,
                format!("DAT pointer {offset:#X} outside data"),
            ));
        }
        Ok(Some(offset))
    }

    pub fn count(&self, count: RecordCount) -> Result<usize> {
        Ok(self.count_with_terminator(count)?.0)
    }

    fn count_with_terminator(&self, count: RecordCount) -> Result<(usize, Option<Range<usize>>)> {
        match count {
            RecordCount::Fixed(count) => Ok((count as usize, None)),
            RecordCount::U16(path) | RecordCount::U32(path) => {
                let Some(at) = self.field(path)? else {
                    return Ok((0, None));
                };
                let count = match count {
                    RecordCount::U16(_) => usize::from(self.u16(at)?),
                    _ => self.u32(at)? as usize,
                };
                Ok((count, None))
            }
            RecordCount::Sentinel {
                root,
                stride,
                offset,
                width,
                value,
            } => {
                if stride == 0
                    || !matches!(width, 1 | 2 | 4)
                    || usize::from(offset) + usize::from(width) > usize::from(stride)
                {
                    return Err(Error::new(0, "invalid DAT sentinel layout"));
                }
                let Some(field) = self.field(root)? else {
                    return Ok((0, None));
                };
                let Some(start) = self.pointer(field)? else {
                    return Ok((0, None));
                };
                let mut at = start;
                let mut count = 0;
                loop {
                    let cell = add(at, usize::from(offset))?;
                    let actual = match width {
                        1 => u32::from(self.bytes(cell, 1)?[0]),
                        2 => u32::from(self.u16(cell)?),
                        _ => self.u32(cell)?,
                    };
                    if actual == value {
                        return Ok((count, Some(cell..cell + usize::from(width))));
                    }
                    self.bytes(at, usize::from(stride))?;
                    at = add(at, usize::from(stride))?;
                    count += 1;
                }
            }
        }
    }

    pub fn table(&self, layout: &'static TableLayout) -> Result<Table<'a>> {
        let empty = || Table {
            layout,
            range: 0..0,
            count: 0,
            root_field: None,
            terminator: None,
            source: self.source,
        };
        if let Some((index, count)) = layout.directory
            && index as usize >= self.count(count)?
        {
            return Ok(empty());
        }
        let Some(field) = self.field(layout.root)? else {
            return Ok(empty());
        };
        let Some(start) = self.pointer(field)? else {
            return Ok(Table {
                root_field: Some(field),
                ..empty()
            });
        };
        let stride = usize::from(layout.stride);
        if stride == 0 {
            return Err(Error::new(field, "zero DAT record stride"));
        }
        let (count, terminator) = self.count_with_terminator(layout.records)?;
        let first = (layout.first_record as usize)
            .checked_mul(stride)
            .ok_or_else(|| Error::new(field, "DAT first record overflow"))?;
        let start = add(start, first)?;
        let size = count
            .checked_mul(stride)
            .ok_or_else(|| Error::new(field, "DAT table size overflow"))?;
        self.bytes(start, size)?;
        // A count may be obtained from a *different* table's sentinel. Expose
        // it as metadata only; it does not extend this table's byte range.
        Ok(Table {
            layout,
            range: start..start + size,
            count,
            root_field: Some(field),
            terminator,
            source: self.source,
        })
    }

    /// NUL-terminated source bytes; display decoding is the caller's choice.
    pub fn text(&self, cell: usize) -> Result<Option<(usize, &'a [u8])>> {
        let Some(start) = self.pointer(cell)? else {
            return Ok(None);
        };
        let tail = &self.source[start..];
        let size = tail
            .iter()
            .position(|&byte| byte == 0)
            .ok_or_else(|| Error::new(start, "unterminated DAT string"))?;
        Ok(Some((start, &tail[..size])))
    }
}

fn add(base: usize, offset: usize) -> Result<usize> {
    base.checked_add(offset)
        .ok_or_else(|| Error::new(base, "DAT offset overflow"))
}
