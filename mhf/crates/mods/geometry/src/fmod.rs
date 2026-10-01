//! Checked views of the recursive FMOD blocks consumed by 10002AF0.

use crate::mesh::{self, MATERIAL, VARIANT};

const HEADER: usize = 12;

#[derive(Clone, Copy)]
struct Block<'a> {
    kind: u32,
    count: u32,
    bytes: &'a [u8],
}

pub(crate) struct Geometry<'a> {
    pub vertex_count: u32,
    pub strips: Vec<Strip<'a>>,
}

pub(crate) struct Strip<'a> {
    pub reversed: bool,
    pub material: u32,
    pub variant: u32,
    // FMOD stores little-endian words; the source slice need not be aligned.
    pub indices: &'a [u8],
}

pub(crate) fn word(bytes: &[u8], offset: usize) -> Result<u32, String> {
    let end = offset.checked_add(4).ok_or("FMOD offset overflow")?;
    let bytes = bytes.get(offset..end).ok_or("truncated FMOD data")?;
    Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
}

impl<'a> Block<'a> {
    fn read(bytes: &'a [u8]) -> Result<Self, String> {
        let kind = word(bytes, 0)?;
        let count = word(bytes, 4)?;
        let size = word(bytes, 8)? as usize;
        if size < HEADER {
            return Err("FMOD block is smaller than its header".into());
        }
        Ok(Self {
            kind,
            count,
            bytes: bytes.get(..size).ok_or("truncated FMOD block")?,
        })
    }

    fn children(self) -> Result<impl Iterator<Item = Self>, String> {
        let mut bytes = &self.bytes[HEADER..];
        if self.count as usize > bytes.len() / HEADER {
            return Err("FMOD child count exceeds block size".into());
        }
        // Validate every declared sibling before callers select a matching block.
        // Early iterator termination must not hide malformed later siblings.
        for _ in 0..self.count {
            let block = Self::read(bytes)?;
            bytes = &bytes[block.bytes.len()..];
        }
        bytes = &self.bytes[HEADER..];
        Ok((0..self.count).map(move |_| {
            let block = Self::read(bytes).expect("FMOD children were validated");
            bytes = &bytes[block.bytes.len()..];
            block
        }))
    }
}

/// Read the same MAIN/OBJECT selection as the native FMOD loader. Indices and
/// strip counts in the file are 32-bit, including for unmodified game assets.
pub(crate) fn read(bytes: &[u8], object_index: u32) -> Result<Geometry<'_>, String> {
    let root = Block::read(bytes)?;
    let main = root
        .children()?
        .find(|b| b.kind == 2)
        .ok_or("FMOD has no MAIN block")?;
    let object = main
        .children()?
        .nth(object_index as usize)
        .ok_or("FMOD object index out of range")?;
    if object.kind != 4 {
        return Err("FMOD child is not an OBJECT block".into());
    }
    let mut vertices = None;
    let mut face = None;
    let mut materials = None;
    for child in object.children()? {
        match child.kind {
            0x70000 if vertices.is_none() => vertices = Some(child),
            5 if face.is_none() => face = Some(child),
            0x60000 if materials.is_none() => materials = Some(child),
            _ => {}
        }
    }
    let vertices = vertices.ok_or("FMOD object has no vertex block")?;
    if vertices.count as usize > (vertices.bytes.len() - HEADER) / 12 {
        return Err("truncated FMOD vertex array".into());
    }
    let face = face.ok_or("FMOD object has no face block")?;
    let mut strips = Vec::new();
    for group in face.children()? {
        let variant = match group.kind {
            0x30000 => 0,
            0x40000 => 1,
            _ => return Err(format!("unsupported FMOD strip block {:#x}", group.kind)),
        };
        let mut cursor = HEADER;
        if group.count as usize > (group.bytes.len() - HEADER) / 16 {
            return Err("FMOD strip count exceeds block size".into());
        }
        strips
            .try_reserve(group.count as usize)
            .map_err(|e| e.to_string())?;
        for _ in 0..group.count {
            let packed = word(group.bytes, cursor)?;
            let count = (packed & 0x7fff_ffff) as usize;
            cursor += 4;
            if count < 3 || count > (group.bytes.len() - cursor) / 4 {
                return Err("invalid FMOD triangle strip length".into());
            }
            let material = if let Some(map) = materials {
                if strips.len() >= map.count as usize {
                    return Err("FMOD material map is shorter than its strip array".into());
                }
                word(map.bytes, HEADER + 4 * strips.len())?
            } else {
                0
            };
            // Native material tables remain WORD-addressed. This extension
            // changes geometry counts, not the material-table ABI.
            if material > u16::MAX as u32 {
                return Err("FMOD material identifier exceeds the native material table".into());
            }
            let indices = &group.bytes[cursor..cursor + count * 4];
            for bytes in indices.as_chunks::<4>().0 {
                let index = u32::from_le_bytes(*bytes);
                if index >= vertices.count {
                    return Err(format!(
                        "FMOD vertex index {index} exceeds vertex count {}",
                        vertices.count
                    ));
                }
            }
            cursor += indices.len();
            strips.push(Strip {
                reversed: packed & 0x8000_0000 != 0,
                material,
                variant,
                indices,
            });
        }
    }
    if strips.is_empty() || vertices.count == 0 {
        return Err("FMOD object has no drawable geometry".into());
    }
    Ok(Geometry {
        vertex_count: vertices.count,
        strips,
    })
}

impl Geometry<'_> {
    /// Replace the temporary packed WORD stream produced by the native loader.
    /// Lengths no longer share the 14-bit field used by that intermediate format.
    pub(crate) fn encode(&self, flags: u32) -> Result<Vec<u32>, String> {
        let extra = mesh::descriptor_stride(flags);
        let mut length = 0usize;
        for strip in &self.strips {
            length = length
                .checked_add(extra)
                .and_then(|n| n.checked_add(strip.indices.len() / 4))
                .ok_or("32-bit geometry size overflow")?;
        }
        mesh::byte_size(length, 4)?;
        let mut stream = Vec::new();
        stream
            .try_reserve_exact(length)
            .map_err(|e| e.to_string())?;
        for strip in &self.strips {
            let indices = strip.indices.as_chunks::<4>().0;
            stream.push(indices.len() as u32 | if strip.reversed { 0x8000_0000 } else { 0 });
            if flags & MATERIAL != 0 {
                stream.push(strip.material);
            }
            if flags & VARIANT != 0 {
                stream.push(strip.variant);
            }
            stream.extend(indices.iter().copied().map(u32::from_le_bytes));
        }
        Ok(stream)
    }
}
