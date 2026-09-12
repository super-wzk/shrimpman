//! ECD/EXF resource wrappers. Headers and encrypted input remain available;
//! decoded payloads are separate allocations, never replacement source images.

use crate::{Decoded, Error, Result};

const ECD_PARAMETERS: [(u32, u32); 6] = [
    (0x4a4b_522e, 1),
    (0x0001_0dcd, 1),
    (0x0001_0dcd, 1),
    (0x0001_0dcd, 1),
    (0x0019_660d, 3),
    (0x7d2b_89dd, 1),
];
const EXF_PARAMETERS: [(u32, u32); 5] = [
    (0x4a4b_522e, 1),
    (0x0001_0dcd, 1),
    (0x0001_0dcd, 1),
    (0x0001_0dcd, 1),
    (0x02e9_0edd, 3),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EcdHeader {
    pub magic: [u8; 4],
    pub key_index: u16,
    /// Native 1158F510 checks this against the uppercase basename and payload
    /// CRC for key indices >= 4. Lower key indices preserve but do not use it.
    pub filename_checksum: u16,
    pub payload_size: u32,
    pub crc32: u32,
}

#[derive(Clone, Debug)]
pub struct Ecd<'a> {
    pub source: &'a [u8],
    pub header: EcdHeader,
}

impl<'a> Ecd<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let bytes = source
            .get(..16)
            .ok_or_else(|| Error::new(0, "truncated ECD header"))?;
        let header = EcdHeader {
            magic: bytes[..4].try_into().unwrap(),
            key_index: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            filename_checksum: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            payload_size: u32::from_le_bytes(bytes[8..12].try_into().unwrap()),
            crc32: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        };
        if header.magic != *b"ecd\x1a" {
            return Err(Error::new(0, "expected ECD signature"));
        }
        if header.payload_size as usize > source.len() - 16 {
            return Err(Error::new(8, "truncated ECD payload"));
        }
        Ok(Self { source, header })
    }

    pub fn trailing_bytes(&self) -> Result<&'a [u8]> {
        let end = (self.header.payload_size as usize)
            .checked_add(16)
            .ok_or_else(|| Error::new(8, "ECD payload range overflow"))?;
        self.source
            .get(end..)
            .ok_or_else(|| Error::new(8, "truncated ECD payload"))
    }

    /// Check the independent filename binding consumed before native decrypt.
    /// Decoding without a name validates only the payload CRC.
    pub fn validate_filename(&self, filename: &[u8]) -> Result<()> {
        if self.header.key_index >= 4 {
            let expected = filename_checksum(self.header.crc32, filename)?;
            if self.header.filename_checksum != expected {
                return Err(Error::new(
                    6,
                    format!(
                        "ECD filename checksum mismatch: stored {:04x}, expected {expected:04x}",
                        self.header.filename_checksum,
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Replace the decoded payload, preserving the key and trailer. The payload
    /// CRC seeds both encryption and the filename checksum. A changed payload
    /// under a filename-bound key requires its destination resource name.
    pub fn encode(&self, payload: &[u8], filename: Option<&[u8]>) -> Result<Vec<u8>> {
        let &(multiplier, increment) = ECD_PARAMETERS
            .get(usize::from(self.header.key_index))
            .ok_or_else(|| Error::new(4, "unsupported ECD key index"))?;
        let size = u32::try_from(payload.len())
            .map_err(|_| Error::new(8, "ECD payload exceeds 32-bit size"))?;
        let trailer = self.trailing_bytes()?;
        let mut output = encoded_buffer(payload.len(), trailer.len())?;
        let checksum = crc32(payload);
        let filename_checksum = match filename {
            Some(name) if self.header.key_index >= 4 => filename_checksum(checksum, name)?,
            None if self.header.key_index >= 4 && checksum != self.header.crc32 => {
                return Err(Error::new(
                    6,
                    "ECD payload changes require the destination filename",
                ));
            }
            _ => self.header.filename_checksum,
        };
        output.extend_from_slice(b"ecd\x1a");
        output.extend_from_slice(&self.header.key_index.to_le_bytes());
        output.extend_from_slice(&filename_checksum.to_le_bytes());
        output.extend_from_slice(&size.to_le_bytes());
        output.extend_from_slice(&checksum.to_le_bytes());
        let mut state = checksum.rotate_left(16) | 1;
        state = state.wrapping_mul(multiplier).wrapping_add(increment);
        let mut previous = state as u8;
        for &plain in payload {
            state = state.wrapping_mul(multiplier).wrapping_add(increment);
            let mut low = u32::from(plain >> 4);
            let mut high = u32::from(plain & 15);
            for shift in (0..8).rev() {
                (low, high) = ((high ^ low ^ (state >> (shift * 4))) & 15, low);
            }
            output.push(((high << 4) | low) as u8 ^ previous);
            previous = plain;
        }
        output.extend_from_slice(trailer);
        Ok(output)
    }

    /// Validate the stored CRC32 after decoding the declared payload.
    pub fn decode(self, max_output_bytes: usize) -> Result<Decoded<Self, Box<[u8]>>> {
        let &(multiplier, increment) = ECD_PARAMETERS
            .get(usize::from(self.header.key_index))
            .ok_or_else(|| Error::new(4, "unsupported ECD key index"))?;
        let size = self.header.payload_size as usize;
        let end = size
            .checked_add(16)
            .ok_or_else(|| Error::new(8, "ECD payload range overflow"))?;
        let encoded = self
            .source
            .get(16..end)
            .ok_or_else(|| Error::new(8, "truncated ECD payload"))?;
        let mut output = allocate(size, max_output_bytes)?;
        let mut state = self.header.crc32.rotate_left(16) | 1;
        let mut next = || {
            state = state.wrapping_mul(multiplier).wrapping_add(increment);
            state
        };
        let mut previous = next() as u8;
        for &encrypted in encoded {
            let mut pad = next();
            let mut low = u32::from(encrypted ^ previous);
            let mut high = low >> 4;
            for _ in 0..8 {
                let mixed = pad ^ low;
                low = high;
                high = (high ^ mixed) & 0xff;
                pad >>= 4;
            }
            previous = ((high & 15) | ((low & 15) << 4)) as u8;
            output.push(previous);
        }
        let actual = crc32(&output);
        if actual != self.header.crc32 {
            return Err(Error::new(
                12,
                format!(
                    "ECD CRC32 mismatch: expected {:08x}, decoded {actual:08x}",
                    self.header.crc32
                ),
            ));
        }
        Ok(Decoded::new(self, output.into_boxed_slice()))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExfHeader {
    pub magic: [u8; 4],
    pub key_index: u16,
    /// Native 114D9C70 checks the uppercase basename against seed for key 4.
    pub filename_checksum: u16,
    /// Neither the native stream opener (114D9C70) nor key derivation
    /// (114D9BD0) consumes this word. Its producer-side purpose is unconfirmed.
    pub unknown_08: [u8; 4],
    pub seed: u32,
}

#[derive(Clone, Debug)]
pub struct Exf<'a> {
    pub source: &'a [u8],
    pub header: ExfHeader,
}

impl<'a> Exf<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let bytes = source
            .get(..16)
            .ok_or_else(|| Error::new(0, "truncated EXF header"))?;
        let header = ExfHeader {
            magic: bytes[..4].try_into().unwrap(),
            key_index: u16::from_le_bytes(bytes[4..6].try_into().unwrap()),
            filename_checksum: u16::from_le_bytes(bytes[6..8].try_into().unwrap()),
            unknown_08: bytes[8..12].try_into().unwrap(),
            seed: u32::from_le_bytes(bytes[12..16].try_into().unwrap()),
        };
        if header.magic != *b"exf\x1a" {
            return Err(Error::new(0, "expected EXF signature"));
        }
        Ok(Self { source, header })
    }

    fn key(&self) -> Result<[u8; 16]> {
        let &(multiplier, increment) = EXF_PARAMETERS
            .get(usize::from(self.header.key_index))
            .ok_or_else(|| Error::new(4, "unsupported EXF key index"))?;
        let mut key = [0u8; 16];
        let mut state = self.header.seed;
        for chunk in key.as_chunks_mut::<4>().0 {
            state = state.wrapping_mul(multiplier).wrapping_add(increment);
            chunk.copy_from_slice(&(state ^ self.header.seed).to_le_bytes());
        }
        Ok(key)
    }

    pub fn validate_filename(&self, filename: &[u8]) -> Result<()> {
        if self.header.key_index == 4 {
            let expected = filename_checksum(self.header.seed, filename)?;
            if self.header.filename_checksum != expected {
                return Err(Error::new(
                    6,
                    format!(
                        "EXF filename checksum mismatch: stored {:04x}, expected {expected:04x}",
                        self.header.filename_checksum,
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Keep the stream seed and unknown word; a known destination name rebinds
    /// the filename checksum. Unlike ECD, the native stream loader does not
    /// require the seed to be recomputed from the complete decoded payload.
    pub fn encode(&self, payload: &[u8], filename: Option<&[u8]>) -> Result<Vec<u8>> {
        let key = self.key()?;
        let filename_checksum = match filename {
            Some(name) if self.header.key_index == 4 => filename_checksum(self.header.seed, name)?,
            _ => self.header.filename_checksum,
        };
        let mut output = encoded_buffer(payload.len(), 0)?;
        output.extend_from_slice(b"exf\x1a");
        output.extend_from_slice(&self.header.key_index.to_le_bytes());
        output.extend_from_slice(&filename_checksum.to_le_bytes());
        output.extend_from_slice(&self.header.unknown_08);
        output.extend_from_slice(&self.header.seed.to_le_bytes());
        for (position, &plain) in payload.iter().enumerate() {
            let high = ((plain >> 4) ^ key[position & 15]) & 15;
            let low = (plain ^ (key[usize::from(high)] >> 4)) & 15;
            output.push(((high << 4) | low) ^ position as u8);
        }
        Ok(output)
    }

    pub fn decode(self, max_output_bytes: usize) -> Result<Decoded<Self, Box<[u8]>>> {
        let key = self.key()?;
        let encoded = self
            .source
            .get(16..)
            .ok_or_else(|| Error::new(0, "truncated EXF header"))?;
        let mut output = allocate(encoded.len(), max_output_bytes)?;
        for (position, &encrypted) in encoded.iter().enumerate() {
            let mixed = encrypted ^ position as u8;
            let high = (mixed >> 4) ^ key[position & 15];
            let low = (key[usize::from(mixed >> 4)] >> 4) ^ mixed;
            output.push((low & 15) | ((high & 15) << 4));
        }
        Ok(Decoded::new(self, output.into_boxed_slice()))
    }
}

fn encoded_buffer(payload: usize, trailer: usize) -> Result<Vec<u8>> {
    let size = payload
        .checked_add(trailer)
        .and_then(|size| size.checked_add(16))
        .ok_or_else(|| Error::new(8, "encoded resource size overflow"))?;
    allocate(size, size)
}

fn allocate(size: usize, budget: usize) -> Result<Vec<u8>> {
    if size > budget {
        return Err(Error::new(8, "decoded size exceeds caller resource budget"));
    }
    let mut output = Vec::new();
    output
        .try_reserve_exact(size)
        .map_err(|_| Error::new(8, "cannot allocate decoded payload"))?;
    Ok(output)
}

/// IEEE CRC32, including initial/final inversion (the ECD payload checksum).
pub fn crc32(bytes: &[u8]) -> u32 {
    !crc32_state(u32::MAX, bytes.iter().copied())
}

/// Native ECD 1158F510 and EXF 114D9C70 fold an uppercase filename plus
/// extension into the stored payload CRC or stream seed, respectively.
/// The returned checksum does not alter the encryption stream.
pub fn filename_checksum(seed: u32, filename: &[u8]) -> Result<u16> {
    let basename = filename
        .rsplit(|&byte| matches!(byte, b'/' | b'\\'))
        .next()
        .unwrap();
    let basename = if basename.get(1) == Some(&b':') {
        &basename[2..]
    } else {
        basename
    };
    if basename.is_empty() || !basename.is_ascii() || basename.contains(&0) {
        return Err(Error::new(
            6,
            "resource filename binding requires a nonempty ASCII basename",
        ));
    }
    Ok((crc32_state(seed, basename.iter().map(u8::to_ascii_uppercase)) >> 7) as u16)
}

fn crc32_state(mut crc: u32, bytes: impl IntoIterator<Item = u8>) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut index = 0;
        while index < 256 {
            let mut value = index as u32;
            let mut bit = 0;
            while bit < 8 {
                value = (value >> 1) ^ (0xedb8_8320u32 & 0u32.wrapping_sub(value & 1));
                bit += 1;
            }
            table[index] = value;
            index += 1;
        }
        table
    };
    for byte in bytes {
        crc = (crc >> 8) ^ TABLE[usize::from(crc as u8 ^ byte)];
    }
    crc
}
