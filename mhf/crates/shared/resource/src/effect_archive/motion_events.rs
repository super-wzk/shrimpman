//! Motion-indexed effect events loaded by native functions `113D5380` and
//! `113D4BB0`. The file stores indices and fixed records, not relocated pointers.

use std::io::{Cursor, Read};

use crate::{Error, Result};

#[derive(Clone, Debug)]
pub struct MotionEvents<'a> {
    bytes: &'a [u8],
    pub unknown_00: u16,
    pub lookup_count: u16,
    pub event_count: u16,
    pub unknown_06: u16,
    pub lookup: Option<MotionLookup>,
    pub events: Vec<MotionEvent>,
    pub trailing_bytes: &'a [u8],
}

impl<'a> MotionEvents<'a> {
    pub const HEADER_SIZE: usize = 8;

    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let mut header = [0; Self::HEADER_SIZE];
        cursor
            .read_exact(&mut header)
            .map_err(|_| Error::new(0, "truncated effect motion-event header"))?;
        let lookup_count = u16::from_le_bytes(header[2..4].try_into().unwrap());
        let event_count = u16::from_le_bytes(header[4..6].try_into().unwrap());
        let lookup = if lookup_count == 0 {
            None
        } else {
            Some(MotionLookup::read(&mut cursor, lookup_count)?)
        };
        let events = MotionEvent::read_table(&mut cursor, event_count)?;
        Ok(Self {
            bytes,
            unknown_00: u16::from_le_bytes(header[0..2].try_into().unwrap()),
            lookup_count,
            event_count,
            unknown_06: u16::from_le_bytes(header[6..8].try_into().unwrap()),
            lookup,
            events,
            trailing_bytes: &bytes[cursor.position() as usize..],
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Return consecutive events starting at this motion's stored index, as
    /// `113D4C50` does. Missing motions and empty slots return an empty slice.
    /// Invalid indices remain inspectable after parsing and fail on access.
    pub fn events_for_motion(&self, id: i16) -> Result<&[MotionEvent]> {
        let Some(lookup) = &self.lookup else {
            return Ok(&self.events[..0]);
        };
        if id < lookup.start || id >= lookup.end {
            return Ok(&self.events[..0]);
        }
        let slot = (i32::from(id) - i32::from(lookup.start)) as usize;
        let offset = lookup.offset + 4 + slot * 4;
        let index = lookup
            .event_indices
            .get(slot)
            .ok_or_else(|| Error::new(offset, "effect motion lookup slot is missing"))?;
        let Some(index) = *index else {
            return Ok(&self.events[..0]);
        };
        let events = self.events.get(index as usize..).ok_or_else(|| {
            Error::new(offset, "effect motion event index exceeds the event table")
        })?;
        let count = events
            .iter()
            .position(|event| event.motion_id != id)
            .unwrap_or(events.len());
        Ok(&events[..count])
    }
}

/// Every slot in the signed native motion range, including absent motions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionLookup {
    /// Offset of the range header in the source supplied to the cursor.
    pub offset: usize,
    pub start: i16,
    pub end: i16,
    /// `0xffffffff` is an absent slot; all other stored values are preserved.
    pub event_indices: Vec<Option<u32>>,
}

impl MotionLookup {
    pub(super) fn read(cursor: &mut Cursor<&[u8]>, count: u16) -> Result<Self> {
        let offset = usize::try_from(cursor.position())
            .map_err(|_| Error::new(usize::MAX, "effect motion lookup offset overflow"))?;
        let size = 4 + usize::from(count) * 4;
        if offset > cursor.get_ref().len() || size > cursor.get_ref().len() - offset {
            return Err(Error::new(offset, "truncated effect motion lookup"));
        }
        let mut range = [0; 4];
        cursor
            .read_exact(&mut range)
            .map_err(|_| Error::new(offset, "truncated effect motion range"))?;
        let start = i16::from_le_bytes(range[0..2].try_into().unwrap());
        let end = i16::from_le_bytes(range[2..4].try_into().unwrap());
        if i32::from(end) - i32::from(start) != i32::from(count) {
            return Err(Error::new(
                offset,
                "effect motion range does not match the lookup count",
            ));
        }
        let mut event_indices = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let at = cursor.position() as usize;
            let mut bytes = [0; 4];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| Error::new(at, "truncated effect motion event index"))?;
            let index = u32::from_le_bytes(bytes);
            event_indices.push((index != u32::MAX).then_some(index));
        }
        Ok(Self {
            offset,
            start,
            end,
            event_indices,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MotionEvent {
    /// Record offset in the source supplied to the cursor.
    pub offset: usize,
    pub position_bits: [u32; 3],
    pub motion_id: i16,
    pub frame: i16,
    pub node_index: i16,
    pub emitter_id: i16,
    pub resource_id: i16,
    pub flags: u16,
    pub unknown_18: [u8; 8],
}

impl MotionEvent {
    pub const SIZE: usize = 32;

    pub(super) fn read_table(cursor: &mut Cursor<&[u8]>, count: u16) -> Result<Vec<Self>> {
        let offset = usize::try_from(cursor.position())
            .map_err(|_| Error::new(usize::MAX, "effect motion-event table offset overflow"))?;
        let size = usize::from(count) * Self::SIZE;
        if offset > cursor.get_ref().len() || size > cursor.get_ref().len() - offset {
            return Err(Error::new(offset, "truncated effect motion-event table"));
        }
        let mut events = Vec::with_capacity(usize::from(count));
        for _ in 0..count {
            let offset = cursor.position() as usize;
            let mut bytes = [0; Self::SIZE];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| Error::new(offset, "truncated effect motion event"))?;
            events.push(Self {
                offset,
                position_bits: std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
                }),
                motion_id: i16::from_le_bytes(bytes[12..14].try_into().unwrap()),
                frame: i16::from_le_bytes(bytes[14..16].try_into().unwrap()),
                node_index: i16::from_le_bytes(bytes[16..18].try_into().unwrap()),
                emitter_id: i16::from_le_bytes(bytes[18..20].try_into().unwrap()),
                resource_id: i16::from_le_bytes(bytes[20..22].try_into().unwrap()),
                flags: u16::from_le_bytes(bytes[22..24].try_into().unwrap()),
                unknown_18: bytes[24..32].try_into().unwrap(),
            });
        }
        Ok(events)
    }

    /// Serialize the complete native record without normalizing any field.
    pub fn to_bytes(self) -> [u8; Self::SIZE] {
        let mut bytes = [0; Self::SIZE];
        for (i, value) in self.position_bits.into_iter().enumerate() {
            bytes[i * 4..i * 4 + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in [
            self.motion_id,
            self.frame,
            self.node_index,
            self.emitter_id,
            self.resource_id,
        ]
        .into_iter()
        .enumerate()
        {
            bytes[12 + i * 2..14 + i * 2].copy_from_slice(&value.to_le_bytes());
        }
        bytes[22..24].copy_from_slice(&self.flags.to_le_bytes());
        bytes[24..32].copy_from_slice(&self.unknown_18);
        bytes
    }
}
