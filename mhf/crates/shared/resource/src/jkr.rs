//! JKR (also called JPK): raw, Huffman, LZ and Huffman-over-LZ streams.
//!
//! Encoding numbers and tree addressing are verified against ReFrontier's
//! `ReFrontier/Jpk/JPKDecode{RW,HFIRW,Lz,HFI}.cs`. See container-formats.md.

use std::io::{Cursor, Read};

use crate::{Decoded, Error, Result};

pub const MAGIC: [u8; 4] = *b"JKR\x1a";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum Encoding {
    Raw = 0,
    None = 1,
    Huffman = 2,
    Lz = 3,
    HuffmanLz = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Header {
    pub magic: [u8; 4],
    /// Usually 0x0108; retained without assuming other values are invalid.
    pub version: u16,
    pub encoding: u16,
    pub data_offset: u32,
    pub decoded_size: u32,
}

#[derive(Clone, Debug)]
pub struct Jkr<'a> {
    /// The exact encoded file, including padding and unused trailing bits.
    pub source: &'a [u8],
    pub header: Header,
}

impl<'a> Jkr<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let bytes = source
            .get(..16)
            .ok_or_else(|| Error::new(0, "truncated JKR header"))?;
        let header = Header {
            magic: bytes[0..4].try_into().unwrap(),
            version: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            encoding: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            data_offset: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            decoded_size: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        };
        if header.magic != MAGIC {
            return Err(Error::new(0, "expected JKR signature"));
        }
        if header.data_offset < 16 || header.data_offset as usize > source.len() {
            return Err(Error::new(8, "JKR data offset outside payload"));
        }
        Ok(Self { source, header })
    }

    pub fn encoding(&self) -> Result<Encoding> {
        match self.header.encoding {
            0 => Ok(Encoding::Raw),
            1 => Ok(Encoding::None),
            2 => Ok(Encoding::Huffman),
            3 => Ok(Encoding::Lz),
            4 => Ok(Encoding::HuffmanLz),
            value => Err(Error::new(6, format!("unsupported JKR encoding {value}"))),
        }
    }

    pub fn header_extension(&self) -> Result<&'a [u8]> {
        self.source
            .get(16..self.header.data_offset as usize)
            .ok_or_else(|| Error::new(8, "JKR data offset outside payload"))
    }

    /// Emit the supported uncompressed representation. Preserve the version,
    /// header extension, and a raw source's trailing bytes. Compressed stream
    /// padding has no independent meaning and is replaced with the stream.
    pub fn encode_stored(&self, payload: &[u8]) -> Result<Vec<u8>> {
        let encoding = self.encoding()?;
        let extension = self.header_extension()?;
        let size = u32::try_from(payload.len())
            .map_err(|_| Error::new(12, "JKR payload exceeds 32-bit size"))?;
        let trailer = if matches!(encoding, Encoding::Raw | Encoding::None) {
            let end = (self.header.data_offset as usize)
                .checked_add(self.header.decoded_size as usize)
                .ok_or_else(|| Error::new(8, "JKR data range overflow"))?;
            self.source
                .get(end..)
                .ok_or_else(|| Error::new(end, "truncated raw JKR payload"))?
        } else {
            &[]
        };
        let length = (self.header.data_offset as usize)
            .checked_add(payload.len())
            .and_then(|size| size.checked_add(trailer.len()))
            .ok_or_else(|| Error::new(12, "JKR encoded size overflow"))?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(length)
            .map_err(|_| Error::new(12, "cannot allocate JKR output"))?;
        output.extend_from_slice(&MAGIC);
        output.extend_from_slice(&self.header.version.to_le_bytes());
        let stored = if encoding == Encoding::None { 1u16 } else { 0 };
        output.extend_from_slice(&stored.to_le_bytes());
        output.extend_from_slice(&self.header.data_offset.to_le_bytes());
        output.extend_from_slice(&size.to_le_bytes());
        output.extend_from_slice(extension);
        output.extend_from_slice(payload);
        output.extend_from_slice(trailer);
        Ok(output)
    }

    /// Allocate at most the caller's output budget. No source bytes are changed.
    pub fn decode(self, max_output_bytes: usize) -> Result<Decoded<Self, Box<[u8]>>> {
        let size = self.header.decoded_size as usize;
        if size > max_output_bytes {
            return Err(Error::new(
                12,
                "JKR decoded size exceeds caller resource budget",
            ));
        }
        let offset = self.header.data_offset as usize;
        if offset < 16 || offset > self.source.len() {
            return Err(Error::new(8, "JKR data offset outside payload"));
        }
        let encoding = self.encoding()?;
        if matches!(encoding, Encoding::Raw | Encoding::None) {
            let mut output = Vec::new();
            output
                .try_reserve_exact(size)
                .map_err(|_| Error::new(12, "cannot allocate JKR output"))?;
            let end = offset
                .checked_add(size)
                .ok_or_else(|| Error::new(8, "JKR data range overflow"))?;
            let bytes = self
                .source
                .get(offset..end)
                .ok_or_else(|| Error::new(offset, "truncated raw JKR payload"))?;
            output.extend_from_slice(bytes);
            return Ok(Decoded::new(self, output.into_boxed_slice()));
        }
        let huffman = matches!(encoding, Encoding::Huffman | Encoding::HuffmanLz);
        let mut bytes = Symbols::new(self.source, offset, huffman)?;
        let mut output = Vec::new();
        output
            .try_reserve_exact(size)
            .map_err(|_| Error::new(12, "cannot allocate JKR output"))?;
        if encoding == Encoding::Huffman {
            for _ in 0..size {
                output.push(bytes.byte()?);
            }
        } else {
            decode_lz(&mut bytes, &mut output, size)?;
        }
        Ok(Decoded::new(self, output.into_boxed_slice()))
    }
}

/// Huffman references are symbol numbers: 0..255 are bytes, 256..root are
/// internal nodes. The pair for node N is at table + (N - 256) * 4.
#[derive(Clone, Debug)]
pub struct HuffmanTable<'a> {
    pub source: &'a [u8],
    pub offset: usize,
    pub root: u16,
    pub encoded_nodes: &'a [u8],
    pub data_offset: usize,
}

impl<'a> HuffmanTable<'a> {
    pub fn parse(source: &'a [u8], offset: usize) -> Result<Self> {
        let mut cursor = Cursor::new(source);
        cursor.set_position(offset as u64);
        let mut bytes = [0u8; 2];
        cursor
            .read_exact(&mut bytes)
            .map_err(|_| Error::new(offset, "truncated Huffman root"))?;
        let root = u16::from_le_bytes(bytes);
        // The native/reference tree uses signed 16-bit node references.
        if !(256..=32767).contains(&root) {
            return Err(Error::new(offset, "invalid JKR Huffman root"));
        }
        let table_offset = offset
            .checked_add(2)
            .ok_or_else(|| Error::new(offset, "Huffman offset overflow"))?;
        let size = (usize::from(root) - 255) * 4;
        let end = table_offset
            .checked_add(size)
            .ok_or_else(|| Error::new(offset, "Huffman table range overflow"))?;
        let encoded_nodes = source
            .get(table_offset..end)
            .ok_or_else(|| Error::new(table_offset, "truncated Huffman table"))?;
        let table = Self {
            source,
            offset,
            root,
            encoded_nodes,
            data_offset: table_offset + size,
        };
        table.validate_tree()?;
        Ok(table)
    }

    pub fn children(&self, node: u16) -> Result<[u16; 2]> {
        if !(256..=self.root).contains(&node) {
            return Err(Error::new(self.offset, "Huffman node index outside table"));
        }
        let offset = self
            .offset
            .checked_add(2 + (usize::from(node) - 256) * 4)
            .ok_or_else(|| Error::new(self.offset, "Huffman node offset overflow"))?;
        let end = offset
            .checked_add(4)
            .ok_or_else(|| Error::new(offset, "Huffman node range overflow"))?;
        let pair = self
            .source
            .get(offset..end)
            .ok_or_else(|| Error::new(offset, "Huffman node outside source"))?;
        Ok([
            u16::from_le_bytes(pair[..2].try_into().unwrap()),
            u16::from_le_bytes(pair[2..].try_into().unwrap()),
        ])
    }

    fn validate_tree(&self) -> Result<()> {
        let mut states = vec![0u8; usize::from(self.root) - 255];
        let mut stack = vec![(self.root, false)];
        while let Some((node, leaving)) = stack.pop() {
            if node < 256 {
                continue;
            }
            if node > self.root {
                return Err(Error::new(self.offset, "Huffman child outside table"));
            }
            let index = usize::from(node) - 256;
            if leaving {
                states[index] = 2;
                continue;
            }
            match states[index] {
                1 => return Err(Error::new(self.offset, "cyclic Huffman tree")),
                2 => continue,
                _ => {}
            }
            states[index] = 1;
            let children = self.children(node)?;
            stack.push((node, true));
            stack.push((children[1], false));
            stack.push((children[0], false));
        }
        Ok(())
    }
}

struct Symbols<'a> {
    cursor: Cursor<&'a [u8]>,
    table: Option<HuffmanTable<'a>>,
    flag: u8,
    bits_remaining: u8,
}

impl<'a> Symbols<'a> {
    fn new(source: &'a [u8], offset: usize, huffman: bool) -> Result<Self> {
        let table = huffman
            .then(|| HuffmanTable::parse(source, offset))
            .transpose()?;
        let position = table.as_ref().map_or(offset, |table| table.data_offset);
        let mut cursor = Cursor::new(source);
        cursor.set_position(position as u64);
        Ok(Self {
            cursor,
            table,
            flag: 0,
            bits_remaining: 0,
        })
    }

    fn byte(&mut self) -> Result<u8> {
        let Some(table) = self.table.as_ref() else {
            let mut byte = [0u8; 1];
            let position = self.cursor.position() as usize;
            self.cursor
                .read_exact(&mut byte)
                .map_err(|_| Error::new(position, "truncated JKR symbol stream"))?;
            return Ok(byte[0]);
        };
        let mut node = table.root;
        while node >= 256 {
            if self.bits_remaining == 0 {
                let mut byte = [0u8; 1];
                let position = self.cursor.position() as usize;
                self.cursor
                    .read_exact(&mut byte)
                    .map_err(|_| Error::new(position, "truncated Huffman bit stream"))?;
                self.flag = byte[0];
                self.bits_remaining = 8;
            }
            self.bits_remaining -= 1;
            let bit = (self.flag >> self.bits_remaining) & 1;
            node = table.children(node)?[usize::from(bit)];
        }
        Ok(node as u8)
    }
}

struct LzBits {
    flag: u8,
    remaining: u8,
}

impl LzBits {
    fn bit(&mut self, bytes: &mut Symbols<'_>) -> Result<usize> {
        if self.remaining == 0 {
            self.flag = bytes.byte()?;
            self.remaining = 8;
        }
        self.remaining -= 1;
        Ok(usize::from((self.flag >> self.remaining) & 1))
    }
}

fn decode_lz(bytes: &mut Symbols<'_>, output: &mut Vec<u8>, size: usize) -> Result<()> {
    let mut bits = LzBits {
        flag: 0,
        remaining: 0,
    };
    while output.len() < size {
        if bits.bit(bytes)? == 0 {
            output.push(bytes.byte()?);
            continue;
        }
        let (offset, length) = if bits.bit(bytes)? == 0 {
            let length = (bits.bit(bytes)? << 1) | bits.bit(bytes)?;
            (usize::from(bytes.byte()?), length + 3)
        } else {
            let high = usize::from(bytes.byte()?);
            let low = usize::from(bytes.byte()?);
            let offset = ((high & 31) << 8) | low;
            let length = high >> 5;
            if length != 0 {
                (offset, length + 2)
            } else if bits.bit(bytes)? == 0 {
                let mut length = 0;
                for _ in 0..4 {
                    length = (length << 1) | bits.bit(bytes)?;
                }
                (offset, length + 10)
            } else {
                let length = usize::from(bytes.byte()?);
                if length == 255 {
                    let length = offset + 27;
                    if length > size - output.len() {
                        return Err(Error::new(
                            bytes.cursor.position() as usize,
                            "JKR literal run exceeds decoded size",
                        ));
                    }
                    for _ in 0..length {
                        output.push(bytes.byte()?);
                    }
                    continue;
                }
                (offset, length + 26)
            }
        };
        if offset >= output.len() || length > size - output.len() {
            return Err(Error::new(
                bytes.cursor.position() as usize,
                "invalid JKR LZ back-reference",
            ));
        }
        for _ in 0..length {
            output.push(output[output.len() - offset - 1]);
        }
    }
    Ok(())
}
