//! PNG file headers and exact chunk views for texture-bundle inspection.
//! Compressed IDAT data and unknown chunks are preserved, not decoded or rebuilt.

use std::io::{Cursor, Read};

use crate::{Error, Result};

pub const MAGIC: [u8; 8] = *b"\x89PNG\r\n\x1a\n";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub width: u32,
    pub height: u32,
    pub bit_depth: u8,
    pub color_type: u8,
    pub compression_method: u8,
    pub filter_method: u8,
    pub interlace_method: u8,
}

#[derive(Clone, Copy, Debug)]
pub struct Chunk<'a> {
    pub offset: usize,
    pub length: u32,
    pub kind: [u8; 4],
    pub data: &'a [u8],
    pub crc: u32,
    /// Header, type, payload, and checksum exactly as stored.
    pub source: &'a [u8],
}

impl Chunk<'_> {
    pub fn validate_crc(&self) -> Result<()> {
        let actual = crate::crypto::crc32(&self.source[4..self.source.len() - 4]);
        if actual != self.crc {
            return Err(Error::new(
                self.offset + self.source.len() - 4,
                "PNG chunk CRC mismatch",
            ));
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub struct Png<'a> {
    pub header: Header,
    pub chunks: Vec<Chunk<'a>>,
    /// Bytes following IEND remain part of the original resource.
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> Png<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        let mut signature = [0; 8];
        cursor
            .read_exact(&mut signature)
            .map_err(|_| Error::new(0, "truncated PNG signature"))?;
        if signature != MAGIC {
            return Err(Error::new(0, "expected PNG signature"));
        }
        let mut chunks = Vec::new();
        let mut header = None;
        loop {
            let offset = cursor.position() as usize;
            let mut bytes = [0; 8];
            cursor
                .read_exact(&mut bytes)
                .map_err(|_| Error::new(offset, "truncated PNG chunk header or missing IEND"))?;
            let length = u32::from_be_bytes(bytes[..4].try_into().unwrap());
            let kind: [u8; 4] = bytes[4..].try_into().unwrap();
            let data_start = cursor.position() as usize;
            let data_end = data_start
                .checked_add(length as usize)
                .ok_or_else(|| Error::new(offset, "PNG chunk range overflow"))?;
            let data = source
                .get(data_start..data_end)
                .ok_or_else(|| Error::new(offset, "PNG chunk payload exceeds file"))?;
            cursor.set_position(data_end as u64);
            let mut crc_bytes = [0; 4];
            cursor
                .read_exact(&mut crc_bytes)
                .map_err(|_| Error::new(data_end, "truncated PNG chunk CRC"))?;
            let end = cursor.position() as usize;
            if header.is_none() {
                if kind != *b"IHDR" || length != 13 {
                    return Err(Error::new(offset, "PNG must begin with a 13-byte IHDR"));
                }
                header = Some(Header {
                    width: u32::from_be_bytes(data[..4].try_into().unwrap()),
                    height: u32::from_be_bytes(data[4..8].try_into().unwrap()),
                    bit_depth: data[8],
                    color_type: data[9],
                    compression_method: data[10],
                    filter_method: data[11],
                    interlace_method: data[12],
                });
            }
            chunks.push(Chunk {
                offset,
                length,
                kind,
                data,
                crc: u32::from_be_bytes(crc_bytes),
                source: &source[offset..end],
            });
            if kind == *b"IEND" {
                if length != 0 {
                    return Err(Error::new(offset, "PNG IEND payload must be empty"));
                }
                return Ok(Self {
                    header: header.unwrap(),
                    chunks,
                    trailing: &source[end..],
                    source,
                });
            }
        }
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    /// Validate header fields and chunk checksums; this does not inflate IDAT
    /// or prove that the compressed scanline stream can be decoded.
    pub fn validate(&self) -> Result<()> {
        let h = self.header;
        if h.width == 0 || h.height == 0 || h.width > i32::MAX as u32 || h.height > i32::MAX as u32
        {
            return Err(Error::new(16, "invalid PNG dimensions"));
        }
        let valid_depth = match h.color_type {
            0 => matches!(h.bit_depth, 1 | 2 | 4 | 8 | 16),
            2 | 4 | 6 => matches!(h.bit_depth, 8 | 16),
            3 => matches!(h.bit_depth, 1 | 2 | 4 | 8),
            _ => false,
        };
        if !valid_depth {
            return Err(Error::new(24, "invalid PNG bit depth / color type"));
        }
        if h.compression_method != 0 || h.filter_method != 0 || h.interlace_method > 1 {
            return Err(Error::new(
                26,
                "unsupported PNG compression/filter/interlace method",
            ));
        }
        let mut palette_seen = false;
        let mut image_data_seen = false;
        let mut image_data_finished = false;
        for (index, chunk) in self.chunks.iter().enumerate() {
            if chunk.length > i32::MAX as u32 {
                return Err(Error::new(chunk.offset, "PNG chunk length exceeds 31 bits"));
            }
            chunk.validate_crc()?;
            match &chunk.kind {
                b"IHDR" if index != 0 => {
                    return Err(Error::new(chunk.offset, "duplicate PNG IHDR"));
                }
                b"PLTE" => {
                    if palette_seen
                        || image_data_seen
                        || chunk.length == 0
                        || chunk.length > 768
                        || chunk.length % 3 != 0
                        || matches!(h.color_type, 0 | 4)
                    {
                        return Err(Error::new(chunk.offset, "invalid PNG palette"));
                    }
                    if h.color_type == 3 && chunk.length / 3 > 1 << h.bit_depth {
                        return Err(Error::new(
                            chunk.offset,
                            "PNG palette exceeds indexed bit depth",
                        ));
                    }
                    palette_seen = true;
                }
                b"IDAT" => {
                    if image_data_finished || h.color_type == 3 && !palette_seen {
                        return Err(Error::new(chunk.offset, "invalid PNG IDAT ordering"));
                    }
                    image_data_seen = true;
                }
                _ => {
                    if image_data_seen {
                        image_data_finished = true;
                    }
                }
            }
        }
        if !image_data_seen {
            return Err(Error::new(8, "PNG has no IDAT chunks"));
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "tests/png.rs"]
mod tests;
