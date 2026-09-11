//! Native MOT directories, motions, ordered tracks and encoded channel keys.
//!
//! The directory's group count comes from its native caller. A motion starts with
//! a 20-byte header; tracks and channels each start with a 12-byte block header.
//! Encoded values are retained without axis changes, unit conversions, guessed
//! bone numbering, channel removal, key sorting or interpolation resampling.

use std::{
    collections::BTreeSet,
    io::{Cursor, Read},
};

use crate::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHeader {
    pub kind: u32,
    pub count: u32,
    /// Includes this header and any unrecognized bytes at the end of the block.
    pub byte_size: u32,
}

impl BlockHeader {
    pub const SIZE: usize = 12;

    fn read(cursor: &mut Cursor<&[u8]>) -> Result<Self> {
        let offset = cursor.position() as usize;
        let mut bytes = [0; Self::SIZE];
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| Error::new(offset, "truncated MOT block header"))?;
        let header = Self {
            kind: u32::from_le_bytes(bytes[0..4].try_into().unwrap()),
            count: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            byte_size: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
        };
        if header.byte_size < Self::SIZE as u32 {
            return Err(Error::new(
                offset + 8,
                "MOT block is smaller than its header",
            ));
        }
        Ok(header)
    }
}

/// One native `(count, offsets_table)` directory record. Null animation slots
/// and repeated offsets remain in their original positions.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MotionGroup {
    pub offsets_offset: u32,
    pub motion_offsets: Vec<Option<u32>>,
}

#[derive(Clone, Debug)]
pub struct MotionArchive<'a> {
    bytes: &'a [u8],
    pub groups: Vec<MotionGroup>,
}

/// A structurally validated MOT directory record region observed in a file.
///
/// The first offset table bounds the candidate record region; every record,
/// table and referenced motion must then validate. At least one slot must be
/// declared, but all slots may be empty. Empty trailing records are retained.
/// This is not the native consumed group count: that value is supplied
/// by the loader's caller and is not stored in the MOT stream. A caller may
/// consume only a prefix of these records.
///
/// This conservative probe is useful after a resource container identifies a
/// possible motion member. Layouts whose records overlap tables, whose motions
/// overlap one another, or whose motion kind is unknown require an explicit
/// count through `MotionArchive::parse_with_budget` instead.
#[derive(Clone, Debug)]
pub struct ObservedMotionDirectory<'a> {
    pub directory: MotionArchive<'a>,
}

impl<'a> ObservedMotionDirectory<'a> {
    /// `max_total_slots` independently bounds the number of observed records and
    /// the aggregate decoded slots. Aliased motions are validated only once.
    pub fn probe_with_budget(bytes: &'a [u8], max_total_slots: usize) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let mut first = [0; 8];
        cursor
            .read_exact(&mut first)
            .map_err(|_| Error::new(0, "truncated MOT directory candidate"))?;
        // Native table addressing drops the low two bits; retain those bits in
        // the parsed record while testing the actual referenced region.
        let directory_end = (u32::from_le_bytes(first[4..].try_into().unwrap()) / 4 * 4) as usize;
        if directory_end < 8 || !directory_end.is_multiple_of(8) {
            return Err(Error::new(4, "MOT candidate record region is not aligned"));
        }
        let record_count = directory_end / 8;
        if record_count > max_total_slots {
            return Err(Error::new(
                4,
                "MOT candidate record count exceeds caller budget",
            ));
        }
        let directory = MotionArchive::parse_with_budget(bytes, record_count, max_total_slots)?;
        let mut tables_end = directory_end;
        let mut motions = BTreeSet::new();
        for (index, group) in directory.groups.iter().enumerate() {
            let table_start = (group.offsets_offset / 4 * 4) as usize;
            if table_start < directory_end {
                return Err(Error::new(
                    index * 8 + 4,
                    "MOT candidate table overlaps its records",
                ));
            }
            // parse_with_budget has already checked multiplication, addition
            // and the complete table extent against the borrowed source.
            tables_end = tables_end.max(table_start + 4 * group.motion_offsets.len());
            motions.extend(group.motion_offsets.iter().flatten().copied());
        }
        if directory
            .groups
            .iter()
            .all(|group| group.motion_offsets.is_empty())
        {
            return Err(Error::new(
                0,
                "MOT directory candidate has no declared slots",
            ));
        }
        let mut previous_end = tables_end;
        for offset in motions {
            let offset = offset as usize;
            if offset < previous_end {
                return Err(Error::new(
                    offset,
                    "MOT candidate motion overlaps a table or another motion",
                ));
            }
            let motion = Motion::parse_at(bytes, offset)?;
            if !matches!(motion.header.kind & 0xff, 1 | 2) {
                return Err(Error::new(
                    offset,
                    "MOT candidate has an unrecognized motion kind",
                ));
            }
            previous_end = offset + motion.as_bytes().len();
        }
        Ok(Self { directory })
    }

    /// Observed file records, including empty records; not a native group count.
    pub fn record_count(&self) -> usize {
        self.directory.groups.len()
    }
}

impl<'a> MotionArchive<'a> {
    /// `108FD1D0` receives `group_count` from its caller, rather than reading a
    /// magic value or a group count from this byte stream.
    ///
    /// Decodes at most one slot per four source bytes across all groups. Use
    /// `parse_with_budget` to allow more slots in heavily aliased directories.
    pub fn parse(bytes: &'a [u8], group_count: usize) -> Result<Self> {
        Self::parse_with_budget(bytes, group_count, bytes.len() / 4)
    }

    /// Limit the total number of allocated `Option<u32>` slots across all
    /// groups, counting aliased or overlapping table entries each time they are
    /// decoded. The group directory allocation is bounded by the source size.
    pub fn parse_with_budget(
        bytes: &'a [u8],
        group_count: usize,
        max_total_slots: usize,
    ) -> Result<Self> {
        let directory_size = group_count
            .checked_mul(8)
            .ok_or_else(|| Error::new(0, "MOT directory size overflow"))?;
        if directory_size > bytes.len() {
            return Err(Error::new(0, "truncated MOT group directory"));
        }
        let mut groups = Vec::with_capacity(group_count);
        let mut remaining_slots = max_total_slots;
        for (group, record) in bytes[..directory_size]
            .as_chunks::<8>()
            .0
            .iter()
            .enumerate()
        {
            let count = u32::from_le_bytes(record[..4].try_into().unwrap()) as usize;
            let offsets_offset = u32::from_le_bytes(record[4..].try_into().unwrap());
            // The native load divides this field by four before indexing DWORDs.
            // Preserve the stored low bits instead of rewriting the directory.
            let table_offset = (offsets_offset / 4 * 4) as usize;
            let size = count
                .checked_mul(4)
                .ok_or_else(|| Error::new(group * 8, "MOT offset count overflow"))?;
            if table_offset > bytes.len() || size > bytes.len() - table_offset {
                return Err(Error::new(
                    group * 8 + 4,
                    "MOT offset table exceeds the resource",
                ));
            }
            remaining_slots = remaining_slots.checked_sub(count).ok_or_else(|| {
                Error::new(group * 8, "MOT total slot count exceeds caller budget")
            })?;
            let mut motion_offsets = Vec::with_capacity(count);
            for (index, value) in bytes[table_offset..table_offset + size]
                .as_chunks::<4>()
                .0
                .iter()
                .enumerate()
            {
                let offset = table_offset + index * 4;
                let value = u32::from_le_bytes(*value);
                if value != u32::MAX {
                    let at = value as usize;
                    if at > bytes.len() || Motion::HEADER_SIZE > bytes.len() - at {
                        return Err(Error::new(
                            offset,
                            "MOT animation offset exceeds the resource",
                        ));
                    }
                }
                motion_offsets.push((value != u32::MAX).then_some(value));
            }
            groups.push(MotionGroup {
                offsets_offset,
                motion_offsets,
            });
        }
        Ok(Self { bytes, groups })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn motion(&self, group: usize, slot: usize) -> Result<Option<Motion<'a>>> {
        let group_record = self
            .groups
            .get(group)
            .ok_or_else(|| Error::new(0, "MOT group index out of range"))?;
        let offset = group_record
            .motion_offsets
            .get(slot)
            .ok_or_else(|| Error::new(group * 8, "MOT slot index out of range"))?;
        offset
            .map(|offset| Motion::parse_at(self.bytes, offset as usize))
            .transpose()
    }

    /// Native action numbering uses a group of one hundred, independent of how
    /// many slots the particular group stores.
    pub fn motion_by_native_id(&self, id: usize) -> Result<Option<Motion<'a>>> {
        self.motion(id / 100, id % 100)
    }
}

#[derive(Clone, Debug)]
pub struct Motion<'a> {
    bytes: &'a [u8],
    /// Offset into the supplied archive; zero when parsed as a standalone clip.
    pub offset: usize,
    pub header: BlockHeader,
    /// Native code checks this DWORD and copies the next DWORD when nonzero.
    /// Its complete semantics are intentionally left uninterpreted.
    pub metadata_present: u32,
    pub metadata: u32,
    pub tracks: Vec<Track<'a>>,
    /// Bytes inside the declared motion length following the declared tracks.
    pub trailing_bytes: &'a [u8],
}

impl<'a> Motion<'a> {
    pub const HEADER_SIZE: usize = 20;

    /// Conservatively recognize a complete standalone clip without a filename
    /// or model-package hint. All bytes must belong to known native records;
    /// unknown encodings or unexplained tails still use the explicit parser.
    /// Empty tracks are real skeleton slots and are retained in native order.
    pub fn probe(bytes: &'a [u8]) -> Result<Self> {
        // 100018B0 selects the compiler from the first byte of this DWORD.
        // Reject unrelated payloads before allocating any track/channel tree.
        let header = BlockHeader::read(&mut Cursor::new(bytes))?;
        if !matches!(header.kind as u8, 1 | 2)
            || header.count == 0
            || header.byte_size as usize != bytes.len()
        {
            return Err(Error::new(0, "unrecognized standalone MOT layout"));
        }
        let motion = Self::parse(bytes)?;
        if !motion.trailing_bytes.is_empty() {
            return Err(Error::new(0, "unexplained standalone MOT tail"));
        }
        for track in &motion.tracks {
            if !track.trailing_bytes.is_empty() {
                return Err(Error::new(
                    track.offset,
                    "unexplained standalone MOT track tail",
                ));
            }
            for channel in &track.channels {
                if channel.target_slot().is_none()
                    || channel.encoding().stride().is_none()
                    || channel.header.count != u32::from(channel.native_key_count())
                    || !channel.trailing_bytes().is_empty()
                {
                    return Err(Error::new(
                        channel.offset,
                        "standalone MOT channel layout is not fully recognized",
                    ));
                }
            }
        }
        Ok(motion)
    }

    /// Parse an exact standalone motion. Use `parse_at` for a directory member.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let result = Self::parse_at(bytes, 0)?;
        if result.bytes.len() != bytes.len() {
            return Err(Error::new(
                result.bytes.len(),
                "bytes follow the declared MOT motion; parse it as an archive member",
            ));
        }
        Ok(result)
    }

    pub fn parse_at(source: &'a [u8], offset: usize) -> Result<Self> {
        if offset > source.len() || Self::HEADER_SIZE > source.len() - offset {
            return Err(Error::new(offset, "truncated MOT motion header"));
        }
        let mut cursor = Cursor::new(source);
        cursor.set_position(offset as u64);
        let header = BlockHeader::read(&mut cursor)?;
        if header.byte_size < Self::HEADER_SIZE as u32 {
            return Err(Error::new(
                offset + 8,
                "MOT motion is smaller than its 20-byte header",
            ));
        }
        let end = offset
            .checked_add(header.byte_size as usize)
            .ok_or_else(|| Error::new(offset + 8, "MOT motion size overflow"))?;
        let bytes = source
            .get(offset..end)
            .ok_or_else(|| Error::new(offset + 8, "MOT motion exceeds the resource"))?;
        let mut metadata = [0; 8];
        cursor
            .read_exact(&mut metadata)
            .map_err(|_| Error::new(offset + 12, "truncated MOT metadata"))?;
        if header.count as usize > (bytes.len() - Self::HEADER_SIZE) / BlockHeader::SIZE {
            return Err(Error::new(offset + 4, "MOT track count exceeds the motion"));
        }
        let mut tracks = Vec::with_capacity(header.count as usize);
        for _ in 0..header.count {
            let track_offset = cursor.position() as usize;
            let track = Track::parse_at(&source[..end], track_offset)?;
            cursor.set_position((track_offset + track.bytes.len()) as u64);
            tracks.push(track);
        }
        let trailing_bytes = &source[cursor.position() as usize..end];
        Ok(Self {
            bytes,
            offset,
            header,
            metadata_present: u32::from_le_bytes(metadata[..4].try_into().unwrap()),
            metadata: u32::from_le_bytes(metadata[4..].try_into().unwrap()),
            tracks,
            trailing_bytes,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    /// Return a copy with exactly one encoded key replaced. Record size,
    /// directory offsets, unused bytes and every other key remain unchanged.
    pub fn with_key(
        &self,
        track: usize,
        channel: usize,
        index: usize,
        key: Keyframe,
    ) -> Result<Vec<u8>> {
        let track = self
            .tracks
            .get(track)
            .ok_or_else(|| Error::new(self.offset, "MOT track index out of range"))?;
        let channel = track
            .channels
            .get(channel)
            .ok_or_else(|| Error::new(track.offset, "MOT channel index out of range"))?;
        if key.encoding() != channel.encoding() {
            return Err(Error::new(
                channel.offset,
                "replacement key has a different MOT encoding",
            ));
        }
        // Validate the key exists before calculating its output position.
        channel.key(index)?;
        let encoded = key.to_bytes();
        let start = channel.offset - self.offset + BlockHeader::SIZE + index * encoded.len();
        let mut bytes = self.bytes.to_vec();
        bytes[start..start + encoded.len()].copy_from_slice(&encoded);
        Ok(bytes)
    }
}

#[derive(Clone, Debug)]
pub struct Track<'a> {
    bytes: &'a [u8],
    pub offset: usize,
    /// Includes the original channel-mask bits; this is not a bone ID.
    pub header: BlockHeader,
    pub channels: Vec<Channel<'a>>,
    pub trailing_bytes: &'a [u8],
}

impl<'a> Track<'a> {
    fn parse_at(source: &'a [u8], offset: usize) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        cursor.set_position(offset as u64);
        let header = BlockHeader::read(&mut cursor)?;
        let end = offset
            .checked_add(header.byte_size as usize)
            .ok_or_else(|| Error::new(offset + 8, "MOT track size overflow"))?;
        let bytes = source
            .get(offset..end)
            .ok_or_else(|| Error::new(offset + 8, "MOT track exceeds its motion"))?;
        if header.count as usize > (bytes.len() - BlockHeader::SIZE) / BlockHeader::SIZE {
            return Err(Error::new(
                offset + 4,
                "MOT channel count exceeds its track",
            ));
        }
        let mut channels = Vec::with_capacity(header.count as usize);
        for _ in 0..header.count {
            let channel_offset = cursor.position() as usize;
            let channel = Channel::parse_at(&source[..end], channel_offset)?;
            cursor.set_position((channel_offset + channel.bytes.len()) as u64);
            channels.push(channel);
        }
        let trailing_bytes = &source[cursor.position() as usize..end];
        Ok(Self {
            bytes,
            offset,
            header,
            channels,
            trailing_bytes,
        })
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }
}

/// Encodings copied by native functions `10001B80` through `10001E40`.
/// Names describe storage only; interpolation behavior is not inferred.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum KeyEncoding {
    I16Pair,
    I16Quad,
    Mixed12,
    F32Pair,
    F32Quad,
    F32Five,
    Unknown(u8),
    Disabled,
}

impl KeyEncoding {
    pub fn stride(self) -> Option<usize> {
        match self {
            Self::I16Pair => Some(4),
            Self::I16Quad | Self::F32Pair => Some(8),
            Self::Mixed12 => Some(12),
            Self::F32Quad => Some(16),
            Self::F32Five => Some(20),
            Self::Unknown(_) | Self::Disabled => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Channel<'a> {
    bytes: &'a [u8],
    pub offset: usize,
    /// `count` retains all 32 bits even though the native key copy uses its low
    /// 16 bits. Unknown encodings stay available as their complete raw block.
    pub header: BlockHeader,
}

impl<'a> Channel<'a> {
    fn parse_at(source: &'a [u8], offset: usize) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        cursor.set_position(offset as u64);
        let header = BlockHeader::read(&mut cursor)?;
        let end = offset
            .checked_add(header.byte_size as usize)
            .ok_or_else(|| Error::new(offset + 8, "MOT channel size overflow"))?;
        let bytes = source
            .get(offset..end)
            .ok_or_else(|| Error::new(offset + 8, "MOT channel exceeds its track"))?;
        let channel = Self {
            bytes,
            offset,
            header,
        };
        if let Some(stride) = channel.encoding().stride() {
            let required = usize::from(channel.native_key_count()) * stride;
            if required > bytes.len() - BlockHeader::SIZE {
                return Err(Error::new(
                    offset + 4,
                    "MOT encoded key count exceeds its channel",
                ));
            }
        }
        Ok(channel)
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn payload(&self) -> &'a [u8] {
        &self.bytes[BlockHeader::SIZE..]
    }

    pub fn native_key_count(&self) -> u16 {
        self.header.count as u16
    }

    /// Exact low-nine-bit mapping performed by `10001EE0`. This does not invent
    /// a skeleton index or apply a coordinate-system conversion.
    pub fn target_slot(&self) -> Option<u8> {
        let mask = self.header.kind & 0x1ff;
        mask.is_power_of_two().then(|| mask.trailing_zeros() as u8)
    }

    pub fn encoding(&self) -> KeyEncoding {
        if self.header.kind & 0x8000_0000 == 0 {
            return KeyEncoding::Disabled;
        }
        match (self.header.kind >> 16) as u8 {
            0x11 => KeyEncoding::I16Pair,
            0x12 => KeyEncoding::I16Quad,
            0x13 => KeyEncoding::Mixed12,
            0x21 => KeyEncoding::F32Pair,
            0x22 => KeyEncoding::F32Quad,
            0x23 => KeyEncoding::F32Five,
            other => KeyEncoding::Unknown(other),
        }
    }

    pub fn key(&self, index: usize) -> Result<Keyframe> {
        let encoding = self.encoding();
        let stride = encoding
            .stride()
            .ok_or_else(|| Error::new(self.offset, "MOT channel encoding is not decoded"))?;
        if index >= usize::from(self.native_key_count()) {
            return Err(Error::new(self.offset + 4, "MOT key index out of range"));
        }
        let start = BlockHeader::SIZE + index * stride;
        Ok(Keyframe::decode(
            encoding,
            &self.bytes[start..start + stride],
        ))
    }

    pub fn trailing_bytes(&self) -> &'a [u8] {
        let decoded = self
            .encoding()
            .stride()
            .map_or(0, |stride| stride * usize::from(self.native_key_count()));
        &self.payload()[decoded..]
    }
}

/// Float values use their on-disk bits so NaN payloads and signed zero round-trip.
/// The frame positions are confirmed by `100017F0` and `100018B0`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Keyframe {
    I16Pair {
        value: i16,
        frame: i16,
    },
    I16Quad {
        value: i16,
        frame: i16,
        parameters: [i16; 2],
    },
    Mixed12 {
        unknown_00: u32,
        value: i16,
        frame: i16,
        parameters: [i16; 2],
    },
    F32Pair {
        value_bits: u32,
        frame_bits: u32,
    },
    F32Quad {
        value_bits: u32,
        frame_bits: u32,
        parameter_bits: [u32; 2],
    },
    F32Five {
        unknown_00: u32,
        value_bits: u32,
        frame_bits: u32,
        parameter_bits: [u32; 2],
    },
}

impl Keyframe {
    fn decode(encoding: KeyEncoding, bytes: &[u8]) -> Self {
        match encoding {
            KeyEncoding::I16Pair => Self::I16Pair {
                value: i16::from_le_bytes(bytes[0..2].try_into().unwrap()),
                frame: i16::from_le_bytes(bytes[2..4].try_into().unwrap()),
            },
            KeyEncoding::I16Quad => Self::I16Quad {
                value: i16::from_le_bytes(bytes[0..2].try_into().unwrap()),
                frame: i16::from_le_bytes(bytes[2..4].try_into().unwrap()),
                parameters: std::array::from_fn(|i| {
                    i16::from_le_bytes(bytes[4 + i * 2..6 + i * 2].try_into().unwrap())
                }),
            },
            KeyEncoding::Mixed12 => Self::Mixed12 {
                unknown_00: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                value: i16::from_le_bytes(bytes[4..6].try_into().unwrap()),
                frame: i16::from_le_bytes(bytes[6..8].try_into().unwrap()),
                parameters: std::array::from_fn(|i| {
                    i16::from_le_bytes(bytes[8 + i * 2..10 + i * 2].try_into().unwrap())
                }),
            },
            KeyEncoding::F32Pair => Self::F32Pair {
                value_bits: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                frame_bits: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
            },
            KeyEncoding::F32Quad => Self::F32Quad {
                value_bits: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                frame_bits: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                parameter_bits: std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[8 + i * 4..12 + i * 4].try_into().unwrap())
                }),
            },
            KeyEncoding::F32Five => Self::F32Five {
                unknown_00: u32::from_le_bytes(bytes[..4].try_into().unwrap()),
                value_bits: u32::from_le_bytes(bytes[4..8].try_into().unwrap()),
                frame_bits: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
                parameter_bits: std::array::from_fn(|i| {
                    u32::from_le_bytes(bytes[12 + i * 4..16 + i * 4].try_into().unwrap())
                }),
            },
            KeyEncoding::Unknown(_) | KeyEncoding::Disabled => {
                unreachable!("encoding checked by Channel::key")
            }
        }
    }

    pub fn encoding(self) -> KeyEncoding {
        match self {
            Self::I16Pair { .. } => KeyEncoding::I16Pair,
            Self::I16Quad { .. } => KeyEncoding::I16Quad,
            Self::Mixed12 { .. } => KeyEncoding::Mixed12,
            Self::F32Pair { .. } => KeyEncoding::F32Pair,
            Self::F32Quad { .. } => KeyEncoding::F32Quad,
            Self::F32Five { .. } => KeyEncoding::F32Five,
        }
    }

    /// Raw native timeline coordinate, with no assumed FPS or rescaling.
    pub fn frame(self) -> f32 {
        match self {
            Self::I16Pair { frame, .. }
            | Self::I16Quad { frame, .. }
            | Self::Mixed12 { frame, .. } => f32::from(frame),
            Self::F32Pair { frame_bits, .. }
            | Self::F32Quad { frame_bits, .. }
            | Self::F32Five { frame_bits, .. } => f32::from_bits(frame_bits),
        }
    }

    pub fn to_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.encoding().stride().unwrap());
        match self {
            Self::I16Pair { value, frame } => {
                bytes.extend(value.to_le_bytes());
                bytes.extend(frame.to_le_bytes());
            }
            Self::I16Quad {
                value,
                frame,
                parameters,
            } => {
                for value in [value, frame, parameters[0], parameters[1]] {
                    bytes.extend(value.to_le_bytes());
                }
            }
            Self::Mixed12 {
                unknown_00,
                value,
                frame,
                parameters,
            } => {
                bytes.extend(unknown_00.to_le_bytes());
                for value in [value, frame, parameters[0], parameters[1]] {
                    bytes.extend(value.to_le_bytes());
                }
            }
            Self::F32Pair {
                value_bits,
                frame_bits,
            } => {
                bytes.extend(value_bits.to_le_bytes());
                bytes.extend(frame_bits.to_le_bytes());
            }
            Self::F32Quad {
                value_bits,
                frame_bits,
                parameter_bits,
            } => {
                for value in [value_bits, frame_bits, parameter_bits[0], parameter_bits[1]] {
                    bytes.extend(value.to_le_bytes());
                }
            }
            Self::F32Five {
                unknown_00,
                value_bits,
                frame_bits,
                parameter_bits,
            } => {
                for value in [
                    unknown_00,
                    value_bits,
                    frame_bits,
                    parameter_bits[0],
                    parameter_bits[1],
                ] {
                    bytes.extend(value.to_le_bytes());
                }
            }
        }
        bytes
    }
}
