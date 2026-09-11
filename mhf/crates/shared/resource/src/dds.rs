//! DDS headers and stored subresource ranges. No pixel decoding or rewriting.
//!
//! Layout follows Microsoft's DDS_HEADER / DDS_PIXELFORMAT / DDS_HEADER_DXT10.
//! Unrecognized formats remain inspectable; `surfaces` reports that their byte
//! layout is unsupported instead of guessing a stride.

use std::io::{Cursor, Read};

use crate::{Error, Result};

pub const MAGIC: [u8; 4] = *b"DDS ";
pub const HEADER_SIZE: usize = 124;
pub const PIXEL_FORMAT_SIZE: u32 = 32;
pub const DX10_HEADER_SIZE: usize = 20;
pub const DDPF_FOURCC: u32 = 4;
pub const DDSD_PITCH: u32 = 8;
pub const DDSCAPS2_CUBEMAP: u32 = 0x200;
pub const DDSCAPS2_VOLUME: u32 = 0x20_0000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelFormat {
    pub size: u32,
    pub flags: u32,
    pub four_cc: [u8; 4],
    pub rgb_bit_count: u32,
    pub r_bit_mask: u32,
    pub g_bit_mask: u32,
    pub b_bit_mask: u32,
    pub a_bit_mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub size: u32,
    pub flags: u32,
    pub height: u32,
    pub width: u32,
    pub pitch_or_linear_size: u32,
    pub depth: u32,
    pub mip_map_count: u32,
    pub reserved_1: [u32; 11],
    pub pixel_format: PixelFormat,
    pub caps: u32,
    pub caps_2: u32,
    pub caps_3: u32,
    pub caps_4: u32,
    pub reserved_2: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Dx10Header {
    /// Original DXGI_FORMAT integer; unknown/future enum values are preserved.
    pub dxgi_format: u32,
    /// D3D10_RESOURCE_DIMENSION, usually 2 (1D), 3 (2D), or 4 (3D).
    pub resource_dimension: u32,
    pub misc_flag: u32,
    pub array_size: u32,
    pub misc_flags_2: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Encoding {
    Dxt1,
    Dxt2,
    Dxt3,
    Dxt4,
    Dxt5,
    Bc4Unorm,
    Bc4Snorm,
    Bc5Unorm,
    Bc5Snorm,
    Dxgi(u32),
    /// Uncompressed RGB/luminance/alpha formats use the exact header masks.
    Masks(PixelFormat),
    FourCc([u8; 4]),
    Unknown(PixelFormat),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CubeFace {
    PositiveX,
    NegativeX,
    PositiveY,
    NegativeY,
    PositiveZ,
    NegativeZ,
}

#[derive(Clone, Debug)]
pub struct Surface<'a> {
    pub array_index: u32,
    pub cube_face: Option<CubeFace>,
    pub mip_level: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
    pub row_pitch: usize,
    pub slice_pitch: usize,
    pub offset: usize,
    pub bytes: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct Dds<'a> {
    pub header: Header,
    pub dx10: Option<Dx10Header>,
    pub data_offset: usize,
    source: &'a [u8],
}

impl<'a> Dds<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        let mut magic = [0; 4];
        cursor
            .read_exact(&mut magic)
            .map_err(|_| Error::new(0, "truncated DDS magic"))?;
        if magic != MAGIC {
            return Err(Error::new(0, "expected DDS signature"));
        }
        let mut bytes = [0; HEADER_SIZE];
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| Error::new(4, "truncated DDS header"))?;
        let values: [u32; 31] = std::array::from_fn(|i| {
            u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
        });
        let pixel_format = PixelFormat {
            size: values[18],
            flags: values[19],
            four_cc: values[20].to_le_bytes(),
            rgb_bit_count: values[21],
            r_bit_mask: values[22],
            g_bit_mask: values[23],
            b_bit_mask: values[24],
            a_bit_mask: values[25],
        };
        let header = Header {
            size: values[0],
            flags: values[1],
            height: values[2],
            width: values[3],
            pitch_or_linear_size: values[4],
            depth: values[5],
            mip_map_count: values[6],
            reserved_1: values[7..18].try_into().unwrap(),
            pixel_format,
            caps: values[26],
            caps_2: values[27],
            caps_3: values[28],
            caps_4: values[29],
            reserved_2: values[30],
        };
        if header.size != HEADER_SIZE as u32 {
            return Err(Error::new(4, "DDS header size must be 124"));
        }
        if pixel_format.size != PIXEL_FORMAT_SIZE {
            return Err(Error::new(76, "DDS pixel format size must be 32"));
        }
        let dx10 = if pixel_format.flags & DDPF_FOURCC != 0 && pixel_format.four_cc == *b"DX10" {
            let mut bytes = [0; DX10_HEADER_SIZE];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| Error::new(128, "truncated DDS DX10 header"))?;
            let values: [u32; 5] = std::array::from_fn(|i| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            Some(Dx10Header {
                dxgi_format: values[0],
                resource_dimension: values[1],
                misc_flag: values[2],
                array_size: values[3],
                misc_flags_2: values[4],
            })
        } else {
            None
        };
        Ok(Self {
            header,
            dx10,
            data_offset: cursor.position() as usize,
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn pixel_data(&self) -> &'a [u8] {
        &self.source[self.data_offset..]
    }

    pub fn encoding(&self) -> Encoding {
        let pixel = self.header.pixel_format;
        if let Some(dx10) = self.dx10 {
            return Encoding::Dxgi(dx10.dxgi_format);
        }
        if pixel.flags & DDPF_FOURCC != 0 {
            match &pixel.four_cc {
                b"DXT1" => Encoding::Dxt1,
                b"DXT2" => Encoding::Dxt2,
                b"DXT3" => Encoding::Dxt3,
                b"DXT4" => Encoding::Dxt4,
                b"DXT5" => Encoding::Dxt5,
                b"ATI1" | b"BC4U" => Encoding::Bc4Unorm,
                b"BC4S" => Encoding::Bc4Snorm,
                b"ATI2" | b"BC5U" => Encoding::Bc5Unorm,
                b"BC5S" => Encoding::Bc5Snorm,
                _ => Encoding::FourCc(pixel.four_cc),
            }
        } else if pixel.flags & (0x40 | 0x2 | 0x2_0000) != 0 {
            Encoding::Masks(pixel)
        } else {
            Encoding::Unknown(pixel)
        }
    }

    /// Checked slices for supported DDS storage layouts. This validates the
    /// complete mip/array/face/depth data ranges without allocating image pixels.
    /// Header parsing itself permits unsupported encodings for inspection.
    pub fn surfaces(&self, max_surfaces: usize) -> Result<Vec<Surface<'a>>> {
        let h = self.header;
        if h.width == 0 || h.height == 0 {
            return Err(Error::new(12, "DDS dimensions must be nonzero"));
        }
        let mut arrays = 1;
        let mut depth = 1;
        let mut faces = vec![None];
        const FACES: [CubeFace; 6] = [
            CubeFace::PositiveX,
            CubeFace::NegativeX,
            CubeFace::PositiveY,
            CubeFace::NegativeY,
            CubeFace::PositiveZ,
            CubeFace::NegativeZ,
        ];
        if let Some(dx10) = self.dx10 {
            arrays = dx10.array_size;
            if arrays == 0 {
                return Err(Error::new(140, "DDS DX10 array size is zero"));
            }
            match dx10.resource_dimension {
                2 if h.height == 1 && dx10.misc_flag & 4 == 0 => {}
                3 => {
                    if dx10.misc_flag & 4 != 0 {
                        faces = FACES.into_iter().map(Some).collect();
                    }
                }
                4 if arrays == 1 && h.depth != 0 && dx10.misc_flag & 4 == 0 => {
                    depth = h.depth;
                }
                _ => {
                    return Err(Error::new(
                        132,
                        "invalid or unsupported DDS resource dimensions",
                    ));
                }
            }
        } else if h.caps_2 & DDSCAPS2_VOLUME != 0 {
            if h.depth == 0 || h.caps_2 & DDSCAPS2_CUBEMAP != 0 {
                return Err(Error::new(24, "invalid DDS volume depth/cubemap flags"));
            }
            depth = h.depth;
        } else if h.caps_2 & DDSCAPS2_CUBEMAP != 0 {
            faces = FACES
                .into_iter()
                .enumerate()
                .filter_map(|(i, face)| (h.caps_2 & (0x400 << i) != 0).then_some(Some(face)))
                .collect();
            if faces.is_empty() {
                return Err(Error::new(112, "DDS cubemap has no stored faces"));
            }
        }
        let mips = h.mip_map_count.max(1);
        if mips > 32 - h.width.max(h.height).max(depth).leading_zeros() {
            return Err(Error::new(28, "DDS mip count exceeds dimension chain"));
        }
        let count = (arrays as usize)
            .checked_mul(faces.len())
            .and_then(|n| n.checked_mul(mips as usize))
            .ok_or_else(|| Error::new(28, "DDS surface count overflow"))?;
        if count > max_surfaces {
            return Err(Error::new(28, "DDS surface count exceeds caller budget"));
        }
        let storage = Storage::from_encoding(self.encoding())
            .ok_or_else(|| Error::new(84, "DDS pixel storage layout is not supported"))?;
        let mut result = Vec::new();
        result
            .try_reserve_exact(count)
            .map_err(|_| Error::new(28, "cannot allocate DDS surface directory"))?;
        let mut offset = self.data_offset;
        for array_index in 0..arrays {
            for &cube_face in &faces {
                for mip_level in 0..mips {
                    let width = (h.width >> mip_level).max(1);
                    let height = (h.height >> mip_level).max(1);
                    let depth = (depth >> mip_level).max(1);
                    let (minimum_pitch, rows) = match storage {
                        Storage::Blocks(block_bytes) => (
                            u64::from(width).div_ceil(4) * u64::from(block_bytes),
                            u64::from(height).div_ceil(4),
                        ),
                        Storage::Bits(bits) => (
                            (u64::from(width) * u64::from(bits)).div_ceil(8),
                            u64::from(height),
                        ),
                    };
                    let row_pitch = if mip_level == 0
                        && matches!(storage, Storage::Bits(_))
                        && h.flags & DDSD_PITCH != 0
                    {
                        if u64::from(h.pitch_or_linear_size) < minimum_pitch {
                            return Err(Error::new(
                                20,
                                "DDS row pitch is smaller than its pixel row",
                            ));
                        }
                        u64::from(h.pitch_or_linear_size)
                    } else {
                        minimum_pitch
                    };
                    let slice_pitch = row_pitch
                        .checked_mul(rows)
                        .ok_or_else(|| Error::new(offset, "DDS slice size overflow"))?;
                    let length = slice_pitch
                        .checked_mul(u64::from(depth))
                        .and_then(|n| usize::try_from(n).ok())
                        .ok_or_else(|| Error::new(offset, "DDS surface size overflow"))?;
                    let end = offset
                        .checked_add(length)
                        .ok_or_else(|| Error::new(offset, "DDS surface range overflow"))?;
                    let bytes = self
                        .source
                        .get(offset..end)
                        .ok_or_else(|| Error::new(offset, "truncated DDS surface data"))?;
                    result.push(Surface {
                        array_index,
                        cube_face,
                        mip_level,
                        width,
                        height,
                        depth,
                        row_pitch: row_pitch as usize,
                        slice_pitch: slice_pitch as usize,
                        offset,
                        bytes,
                    });
                    offset = end;
                }
            }
        }
        Ok(result)
    }
}

#[derive(Clone, Copy)]
enum Storage {
    Blocks(u32),
    Bits(u32),
}

impl Storage {
    fn from_encoding(encoding: Encoding) -> Option<Self> {
        match encoding {
            Encoding::Dxt1 | Encoding::Bc4Unorm | Encoding::Bc4Snorm => Some(Self::Blocks(8)),
            Encoding::Dxt2
            | Encoding::Dxt3
            | Encoding::Dxt4
            | Encoding::Dxt5
            | Encoding::Bc5Unorm
            | Encoding::Bc5Snorm => Some(Self::Blocks(16)),
            Encoding::Dxgi(70..=72 | 79..=81) => Some(Self::Blocks(8)),
            Encoding::Dxgi(73..=78 | 82..=84 | 94..=99) => Some(Self::Blocks(16)),
            // Typed, typeless, and sRGB variants share their stored bit width.
            Encoding::Dxgi(1..=4) => Some(Self::Bits(128)),
            Encoding::Dxgi(9..=14) => Some(Self::Bits(64)),
            Encoding::Dxgi(27..=32 | 87..=93) => Some(Self::Bits(32)),
            Encoding::Dxgi(48..=52) => Some(Self::Bits(16)),
            Encoding::Dxgi(60..=65) => Some(Self::Bits(8)),
            Encoding::Masks(pixel) if (1..=128).contains(&pixel.rgb_bit_count) => {
                Some(Self::Bits(pixel.rgb_bit_count))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
#[path = "tests/dds.rs"]
mod tests;
