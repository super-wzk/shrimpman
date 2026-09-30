//! DAT[390]/[391] motion event directories used by `1099B130`/`1091EB00`.
//! Their shared eight-byte directories lead to different event record layouts.
//! All offsets refer to the supplied image, even for relocated native pointers.

use std::ops::Range;

use super::HEADER_SIZE;
use crate::{Error, Result, binary::Reader};

const GROUP_SIZE: usize = 8;
const ENTRY_SIZE: usize = 8;
const ADDITIONAL_GROUP_COUNT: usize = 665 * 4;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum EventKind {
    Command,
    Choice,
}

impl EventKind {
    pub const fn root(self) -> u32 {
        match self {
            Self::Command => 390,
            Self::Choice => 391,
        }
    }

    pub const fn record_size(self) -> usize {
        match self {
            Self::Command => CommandEvent::SIZE,
            Self::Choice => ChoiceEvent::SIZE,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Directory<'a> {
    pub kind: EventKind,
    pub range: Range<usize>,
    pub count: usize,
    /// DAT[665] is a u32 scalar for the selectors. The loader uses its low u16
    /// when sizing this directory; preserve both facts without altering the value.
    pub additional_groups: u32,
    source: &'a [u8],
    base: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Group {
    pub offset: usize,
    pub count: u16,
    pub unknown_02: u16,
    pub records: Range<usize>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub offset: usize,
    /// Sorted key compared by the native binary search, rather than a row index.
    pub key: u16,
    pub count: u16,
    pub events: Range<usize>,
}

impl<'a> Directory<'a> {
    /// Validate only the group directory. Nested tables are checked when read so
    /// a damaged group or event table does not hide unrelated records.
    pub fn parse(source: &'a [u8], base: u32, kind: EventKind) -> Result<Self> {
        source
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(0, "DAT motion directory requires the root header"))?;
        let reader = Reader::new(source);
        let additional = reader.read_at::<u32>(ADDITIONAL_GROUP_COUNT)?.value;
        // 10AF78AB/10AF7913 add 18 in a 16-bit register. The selectors read
        // DAT[665] as u32; keep its complete value in additional_groups.
        let count = usize::from((additional as u16).wrapping_add(18));
        let field = kind.root() as usize * 4;
        let pointer = reader.read_at::<u32>(field)?.value;
        let range = table(source, base, pointer, count, GROUP_SIZE, field)?;
        Ok(Self {
            kind,
            range,
            count,
            additional_groups: additional,
            source,
            base,
        })
    }

    pub fn group(&self, index: usize) -> Result<Group> {
        if index >= self.count {
            return Err(Error::new(
                self.range.start,
                "DAT motion group index out of range",
            ));
        }
        let offset = self.range.start + index * GROUP_SIZE;
        let reader = Reader::new(self.source);
        let count = reader.read_at::<u16>(offset)?.value;
        let unknown_02 = reader.read_at::<u16>(offset + 2)?.value;
        let pointer = reader.read_at::<u32>(offset + 4)?.value;
        let records = table(
            self.source,
            self.base,
            pointer,
            usize::from(count),
            ENTRY_SIZE,
            offset + 4,
        )?;
        Ok(Group {
            offset,
            count,
            unknown_02,
            records,
        })
    }

    pub fn entry(&self, group: usize, index: usize) -> Result<Entry> {
        let group = self.group(group)?;
        if index >= usize::from(group.count) {
            return Err(Error::new(
                group.offset,
                "DAT motion entry index out of range",
            ));
        }
        let offset = group.records.start + index * ENTRY_SIZE;
        let reader = Reader::new(self.source);
        let key = reader.read_at::<u16>(offset)?.value;
        let count = reader.read_at::<u16>(offset + 2)?.value;
        let pointer = reader.read_at::<u32>(offset + 4)?.value;
        let events = table(
            self.source,
            self.base,
            pointer,
            usize::from(count),
            self.kind.record_size(),
            offset + 4,
        )?;
        Ok(Entry {
            offset,
            key,
            count,
            events,
        })
    }
}

fn table(
    source: &[u8],
    base: u32,
    pointer: u32,
    count: usize,
    stride: usize,
    field: usize,
) -> Result<Range<usize>> {
    if count == 0 {
        return Ok(0..0);
    }
    if pointer == 0 {
        return Err(Error::new(
            field,
            "nonempty DAT motion table has a null pointer",
        ));
    }
    let start = pointer
        .checked_sub(base)
        .ok_or_else(|| Error::new(field, "DAT motion pointer is below image base"))?
        as usize;
    if start < HEADER_SIZE {
        return Err(Error::new(
            field,
            "DAT motion pointer is inside root header",
        ));
    }
    let size = count
        .checked_mul(stride)
        .ok_or_else(|| Error::new(field, "DAT motion table size overflow"))?;
    let end = start
        .checked_add(size)
        .ok_or_else(|| Error::new(field, "DAT motion table range overflow"))?;
    if end > source.len() {
        return Err(Error::new(field, "DAT motion table outside image"));
    }
    Ok(start..end)
}

/// DAT[390] passes these seven arguments to the operation-specific callback.
/// Signed interpretation and parameter semantics depend on that callback.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CommandEvent {
    pub frame: u16,
    pub operation: u16,
    pub arg_a: u16,
    pub arg_b: u16,
    pub arg_c: u8,
    pub arg_d: u8,
    pub arg_e: u16,
    pub arg_f: u16,
    pub arg_g: u16,
}

impl CommandEvent {
    pub const SIZE: usize = 16;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; Self::SIZE] = bytes
            .try_into()
            .map_err(|_| Error::new(0, "expected 16 DAT command event bytes"))?;
        Ok(Self {
            frame: u16::from_le_bytes([b[0], b[1]]),
            operation: u16::from_le_bytes([b[2], b[3]]),
            arg_a: u16::from_le_bytes([b[4], b[5]]),
            arg_b: u16::from_le_bytes([b[6], b[7]]),
            arg_c: b[8],
            arg_d: b[9],
            arg_e: u16::from_le_bytes([b[10], b[11]]),
            arg_f: u16::from_le_bytes([b[12], b[13]]),
            arg_g: u16::from_le_bytes([b[14], b[15]]),
        })
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (index, word) in [self.frame, self.operation, self.arg_a, self.arg_b]
            .into_iter()
            .enumerate()
        {
            bytes[index * 2..index * 2 + 2].copy_from_slice(&word.to_le_bytes());
        }
        bytes[8] = self.arg_c;
        bytes[9] = self.arg_d;
        for (index, word) in [self.arg_e, self.arg_f, self.arg_g].into_iter().enumerate() {
            bytes[10 + index * 2..12 + index * 2].copy_from_slice(&word.to_le_bytes());
        }
        bytes
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeightedChoice {
    pub id: u16,
    pub weight: u16,
}

/// DAT[391] uses a weighted four-way selection before dispatching the chosen ID.
/// Zero total weight uses choice 0, and ID 0xFFFF suppresses dispatch. Keep the
/// condition and dispatch values raw; their domains differ from DAT[390].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ChoiceEvent {
    pub frame: u16,
    pub dispatch_kind: u16,
    pub condition: u8,
    pub unknown_05: u8,
    pub choices: [WeightedChoice; 4],
}

impl ChoiceEvent {
    pub const SIZE: usize = 22;

    pub fn parse(bytes: &[u8]) -> Result<Self> {
        let b: &[u8; Self::SIZE] = bytes
            .try_into()
            .map_err(|_| Error::new(0, "expected 22 DAT choice event bytes"))?;
        Ok(Self {
            frame: u16::from_le_bytes([b[0], b[1]]),
            dispatch_kind: u16::from_le_bytes([b[2], b[3]]),
            condition: b[4],
            unknown_05: b[5],
            choices: std::array::from_fn(|index| {
                let at = 6 + index * 4;
                WeightedChoice {
                    id: u16::from_le_bytes([b[at], b[at + 1]]),
                    weight: u16::from_le_bytes([b[at + 2], b[at + 3]]),
                }
            }),
        })
    }

    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        bytes[0..2].copy_from_slice(&self.frame.to_le_bytes());
        bytes[2..4].copy_from_slice(&self.dispatch_kind.to_le_bytes());
        bytes[4] = self.condition;
        bytes[5] = self.unknown_05;
        for (index, choice) in self.choices.into_iter().enumerate() {
            let at = 6 + index * 4;
            bytes[at..at + 2].copy_from_slice(&choice.id.to_le_bytes());
            bytes[at + 2..at + 4].copy_from_slice(&choice.weight.to_le_bytes());
        }
        bytes
    }
}
