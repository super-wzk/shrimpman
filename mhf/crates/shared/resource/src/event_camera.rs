//! Per-frame event cameras loaded by `10829EE0` and relocated by `10829E70`.
//!
//! A 32-byte header references four independent arrays. `1082A6E0` indexes
//! them with strides 4/12/4/12 and writes FOV, position, roll and target into
//! the camera state. It may then transform position/target relative to actors;
//! this parser retains the original coordinates and float encodings.

use crate::{Error, Result};
use std::io::{Cursor, Read};

#[derive(Clone, Debug)]
pub struct EventCamera<'a> {
    bytes: &'a [u8],
    pub unknown_00: u32,
    pub unknown_04: u32,
    /// Loaded as a float by 10829E70, but not consumed by 1082A6E0. The local
    /// corpus contains 1.333 and 16/9; retain it without assigning a new role.
    pub unknown_08_bits: u32,
    pub frame_count: u32,
    /// Relative to the start of this bounded camera member, not its archive.
    pub array_offsets: [u32; 4],
    /// FOV, XYZ position, roll, XYZ target. Each array contains frame_count
    /// records; gaps, aliases and bytes after the arrays remain in as_bytes().
    pub arrays: [&'a [u8]; 4],
}

impl<'a> EventCamera<'a> {
    pub const HEADER_SIZE: usize = 32;
    pub const STRIDES: [usize; 4] = [4, 12, 4, 12];

    /// Parse the native offsets independently, preserving aliases and ordering.
    /// No sample allocation, resampling or float normalization is performed.
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(bytes);
        let mut header = [0; Self::HEADER_SIZE];
        cursor
            .read_exact(&mut header)
            .map_err(|_| Error::new(0, "truncated event-camera header"))?;
        let unknown_00 = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let unknown_04 = u32::from_le_bytes(header[4..8].try_into().unwrap());
        let unknown_08_bits = u32::from_le_bytes(header[8..12].try_into().unwrap());
        let frame_count = u32::from_le_bytes(header[12..16].try_into().unwrap());
        let array_offsets = std::array::from_fn(|index| {
            u32::from_le_bytes(header[16 + index * 4..20 + index * 4].try_into().unwrap())
        });
        let mut arrays = [&[][..]; 4];
        for (index, stride) in Self::STRIDES.into_iter().enumerate() {
            let offset = array_offsets[index] as usize;
            let size = (frame_count as usize)
                .checked_mul(stride)
                .ok_or_else(|| Error::new(12, "event-camera array size overflow"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(16 + index * 4, "event-camera array end overflow"))?;
            if offset < Self::HEADER_SIZE {
                return Err(Error::new(
                    16 + index * 4,
                    "event-camera array overlaps its header",
                ));
            }
            arrays[index] = bytes.get(offset..end).ok_or_else(|| {
                Error::new(16 + index * 4, "event-camera array exceeds its member")
            })?;
        }
        Ok(Self {
            bytes,
            unknown_00,
            unknown_04,
            unknown_08_bits,
            frame_count,
            array_offsets,
            arrays,
        })
    }

    /// Recognize the complete layout observed in native camera members without
    /// using their filename or directory position. Unknown layouts remain raw;
    /// an explicitly identified camera can still use the broader offset parser.
    pub fn probe(bytes: &'a [u8]) -> Result<Self> {
        let camera = Self::parse(bytes)?;
        let value_08 = f32::from_bits(camera.unknown_08_bits);
        if camera.unknown_00 != 0
            || camera.unknown_04 != 0
            || !value_08.is_finite()
            || value_08 <= 0.0
            || camera.frame_count == 0
        {
            return Err(Error::new(0, "unrecognized event-camera header"));
        }
        let mut end = Self::HEADER_SIZE;
        for (index, array) in camera.arrays.iter().enumerate() {
            let aligned = end
                .checked_next_multiple_of(16)
                .ok_or_else(|| Error::new(end, "event-camera alignment overflow"))?;
            if camera.array_offsets[index] as usize != aligned {
                return Err(Error::new(
                    16 + index * 4,
                    "event-camera candidate has a different array layout",
                ));
            }
            // parse() already validated this exact array extent.
            end = aligned + array.len();
        }
        if end.checked_next_multiple_of(16) != Some(bytes.len()) {
            return Err(Error::new(
                end,
                "event-camera candidate does not consume its complete member",
            ));
        }
        Ok(camera)
    }

    pub fn as_bytes(&self) -> &'a [u8] {
        self.bytes
    }

    pub fn frame(&self, index: usize) -> Result<CameraFrame> {
        if index >= self.frame_count as usize {
            return Err(Error::new(12, "event-camera frame index out of range"));
        }
        Ok(CameraFrame {
            field_of_view_bits: u32::from_le_bytes(
                self.arrays[0][index * 4..index * 4 + 4].try_into().unwrap(),
            ),
            position_bits: std::array::from_fn(|component| {
                let offset = index * 12 + component * 4;
                u32::from_le_bytes(self.arrays[1][offset..offset + 4].try_into().unwrap())
            }),
            roll_bits: u32::from_le_bytes(
                self.arrays[2][index * 4..index * 4 + 4].try_into().unwrap(),
            ),
            target_bits: std::array::from_fn(|component| {
                let offset = index * 12 + component * 4;
                u32::from_le_bytes(self.arrays[3][offset..offset + 4].try_into().unwrap())
            }),
        })
    }
}

/// Four discontiguous records at the same frame index. Encoding stays in bits
/// so NaN payloads, signed zero and native coordinates are not changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraFrame {
    pub field_of_view_bits: u32,
    pub position_bits: [u32; 3],
    pub roll_bits: u32,
    pub target_bits: [u32; 3],
}
