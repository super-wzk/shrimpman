//! Checked views of the recursive FMOD blocks consumed by 10002AF0.

use crate::{
    mesh::{self, MATERIAL, VARIANT},
    vertex,
};
use std::{cell::RefCell, ops::Range};

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

pub(crate) struct Source<'a> {
    pub geometry: Geometry<'a>,
    pub vertices: vertex::Vertices<'a>,
    pub flags: u32,
    pub material_count: u16,
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
    fn payload(self) -> &'a [u8] {
        &self.bytes[HEADER..]
    }

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

    fn visit_children(
        self,
        mut visit: impl FnMut(Self) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut bytes = &self.bytes[HEADER..];
        if self.count as usize > bytes.len() / HEADER {
            return Err("FMOD child count exceeds block size".into());
        }
        let mut result = Ok(());
        for _ in 0..self.count {
            let block = Self::read(bytes)?;
            bytes = &bytes[block.bytes.len()..];
            // Finish validating siblings even after a visitor error. As before,
            // malformed later headers take precedence over payload errors.
            if result.is_ok() {
                result = visit(block);
            }
        }
        result
    }

    fn main(self) -> Result<Self, String> {
        let mut main = None;
        self.visit_children(|block| {
            if main.is_none() && block.kind == 2 {
                main = Some(block);
            }
            Ok(())
        })?;
        main.ok_or_else(|| "FMOD has no MAIN block".into())
    }
}

struct Object {
    kind: u32,
    count: u32,
    range: Range<usize>,
}

#[derive(Default)]
struct ObjectCache {
    source: (usize, usize),
    objects: Vec<Object>,
}

impl ObjectCache {
    fn new(bytes: &[u8]) -> Result<Self, String> {
        let main = Block::read(bytes)?.main()?;
        // Bound the reservation before trusting the declared count.
        if main.count as usize > (main.bytes.len() - HEADER) / HEADER {
            return Err("FMOD child count exceeds block size".into());
        }
        let mut objects = Vec::new();
        objects
            .try_reserve_exact(main.count as usize)
            .map_err(|e| e.to_string())?;
        main.visit_children(|block| {
            let start = block.bytes.as_ptr() as usize - bytes.as_ptr() as usize;
            objects.push(Object {
                kind: block.kind,
                count: block.count,
                range: start..start + block.bytes.len(),
            });
            Ok(())
        })?;
        Ok(Self {
            source: (bytes.as_ptr() as usize, bytes.len()),
            objects,
        })
    }

    fn object<'a>(&mut self, bytes: &'a [u8], index: u32) -> Result<Block<'a>, String> {
        if self.source != (bytes.as_ptr() as usize, bytes.len()) {
            *self = Self::new(bytes)?;
        }
        let object = self
            .objects
            .get(index as usize)
            .ok_or("FMOD object index out of range")?;
        Ok(Block {
            kind: object.kind,
            count: object.count,
            bytes: bytes
                .get(object.range.clone())
                .ok_or("truncated FMOD block")?,
        })
    }
}

thread_local! {
    static OBJECTS: RefCell<Option<ObjectCache>> = const { RefCell::new(None) };
}

/// Reuse object headers only during one synchronous native construction.
/// Nested constructions get their own cache; return/unwind releases it.
///
/// # Safety
/// Every FMOD parsed in this scope must remain alive and unchanged throughout this
/// call, including between reads. Each construction needs its own scope even
/// when it reuses the previous construction's source address and size.
/// The cache stores offsets, never borrowed or dereferenceable native pointers.
pub(crate) unsafe fn with_cached_objects<T>(construct: impl FnOnce() -> T) -> T {
    struct Scope(Option<ObjectCache>);
    impl Drop for Scope {
        fn drop(&mut self) {
            OBJECTS.with(|current| *current.borrow_mut() = self.0.take());
        }
    }
    let _scope = Scope(OBJECTS.with(|current| current.replace(Some(ObjectCache::default()))));
    construct()
}

fn object(bytes: &[u8], object_index: u32) -> Result<Block<'_>, String> {
    let object = OBJECTS.with(|cache| {
        if let Some(cache) = cache.borrow_mut().as_mut() {
            return cache.object(bytes, object_index);
        }
        // Direct callers outside a resource construction still validate the
        // complete directory, without retaining source identity across calls.
        let main = Block::read(bytes)?.main()?;
        let mut object = None;
        let mut index = 0;
        main.visit_children(|block| {
            if index == object_index {
                object = Some(block);
            }
            index += 1;
            Ok(())
        })?;
        object.ok_or_else(|| "FMOD object index out of range".to_owned())
    })?;
    if object.kind != 4 {
        return Err("FMOD child is not an OBJECT block".into());
    }
    Ok(object)
}

#[derive(Default)]
struct ObjectBlocks<'a> {
    face: Option<Block<'a>>,
    material_list: Option<Block<'a>>,
    materials: Option<Block<'a>>,
    positions: Option<Block<'a>>,
    normals: Option<Block<'a>>,
    uvs: Option<Block<'a>>,
    colors: Option<Block<'a>>,
    weights: Option<Block<'a>>,
    attribute: Option<Block<'a>>,
}

impl<'a> ObjectBlocks<'a> {
    fn read(bytes: &'a [u8], index: u32) -> Result<Self, String> {
        let mut blocks = Self::default();
        object(bytes, index)?.visit_children(|child| {
            let slot = match child.kind {
                5 => &mut blocks.face,
                0x50000 => &mut blocks.material_list,
                0x60000 => &mut blocks.materials,
                0x70000 => &mut blocks.positions,
                0x80000 => &mut blocks.normals,
                0xa0000 => &mut blocks.uvs,
                0xb0000 => &mut blocks.colors,
                0xc0000 => &mut blocks.weights,
                0x120000 => &mut blocks.attribute,
                _ => return Ok(()),
            };
            if slot.is_none() {
                *slot = Some(child);
            }
            Ok(())
        })?;
        Ok(blocks)
    }

    fn geometry(&self) -> Result<Geometry<'a>, String> {
        let vertices = self.positions.ok_or("FMOD object has no vertex block")?;
        if vertices.count as usize > (vertices.bytes.len() - HEADER) / 12 {
            return Err("truncated FMOD vertex array".into());
        }
        let face = self.face.ok_or("FMOD object has no face block")?;
        let mut strips = Vec::new();
        face.visit_children(|group| {
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
                let material = if let Some(map) = self.materials {
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
            Ok(())
        })?;
        if strips.is_empty() || vertices.count == 0 {
            return Err("FMOD object has no drawable geometry".into());
        }
        Ok(Geometry {
            vertex_count: vertices.count,
            strips,
        })
    }
}

/// Read indices independently for topology tests; production reads all source
/// attributes from the same validated object directory in `read_source`.
#[cfg(test)]
pub(crate) fn read(bytes: &[u8], object_index: u32) -> Result<Geometry<'_>, String> {
    ObjectBlocks::read(bytes, object_index)?.geometry()
}

pub(crate) fn read_source(bytes: &[u8], index: u32) -> Result<Source<'_>, String> {
    let blocks = ObjectBlocks::read(bytes, index)?;
    let geometry = blocks.geometry()?;
    let vertices = vertex::Vertices::new(
        geometry.vertex_count,
        blocks.positions.unwrap().payload(),
        blocks.normals.map(Block::payload),
        blocks.colors.map(Block::payload),
        blocks.uvs.map(Block::payload),
        blocks.weights.map(Block::payload),
        blocks.attribute.map(Block::payload),
    )?;
    let material = geometry.strips[0].material;
    let mut flags = 0x21000;
    if geometry
        .strips
        .iter()
        .any(|strip| strip.material != material)
    {
        flags |= MATERIAL;
    }
    if vertices.skinned() {
        flags |= VARIANT;
    }
    Ok(Source {
        geometry,
        vertices,
        flags,
        // The native loader stores the 0x50000 header count as a WORD.
        material_count: blocks.material_list.map_or(0, |block| block.count as u16),
    })
}

impl Geometry<'_> {
    /// Size the DWORD strip stream without the native WORD format's 14-bit limit.
    pub(crate) fn encoded_size(&self, flags: u32) -> Result<u32, String> {
        let extra = mesh::descriptor_stride(flags);
        let mut length = 0usize;
        for strip in &self.strips {
            length = length
                .checked_add(extra)
                .and_then(|n| n.checked_add(strip.indices.len() / 4))
                .ok_or("32-bit geometry size overflow")?;
        }
        mesh::byte_size(length, 4)
    }

    /// Initialize the destination sized by `encoded_size` without a staging copy.
    pub(crate) fn encode_into(&self, flags: u32, mut stream: &mut [std::mem::MaybeUninit<u32>]) {
        for strip in &self.strips {
            let indices = strip.indices.as_chunks::<4>().0;
            let (header, rest) = stream.split_at_mut(mesh::descriptor_stride(flags));
            header[0].write(indices.len() as u32 | if strip.reversed { 0x8000_0000 } else { 0 });
            let mut slot = 1;
            if flags & MATERIAL != 0 {
                header[slot].write(strip.material);
                slot += 1;
            }
            if flags & VARIANT != 0 {
                header[slot].write(strip.variant);
            }
            let (output, rest) = rest.split_at_mut(indices.len());
            for (output, index) in output.iter_mut().zip(indices) {
                output.write(u32::from_le_bytes(*index));
            }
            stream = rest;
        }
    }

    #[cfg(test)]
    pub(crate) fn encode(&self, flags: u32) -> Result<Vec<u32>, String> {
        let length = self.encoded_size(flags)? as usize / 4;
        let mut stream = Vec::with_capacity(length);
        self.encode_into(flags, &mut stream.spare_capacity_mut()[..length]);
        // encode_into initializes every word of the exact-sized destination.
        unsafe { stream.set_len(length) };
        Ok(stream)
    }
}
