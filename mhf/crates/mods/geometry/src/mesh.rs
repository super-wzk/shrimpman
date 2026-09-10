//! GPU indices, draw descriptors and culling bounds use the same 32-bit data.

pub(crate) const MATERIAL: u32 = 0x10_0000;
pub(crate) const VARIANT: u32 = 0x20_0000;

pub(crate) fn descriptor_stride(flags: u32) -> usize {
    1 + usize::from(flags & MATERIAL != 0) + usize::from(flags & VARIANT != 0)
}

pub(crate) fn byte_size(count: usize, stride: usize) -> Result<u32, String> {
    let size = count
        .checked_mul(stride)
        .ok_or("geometry allocation overflow")?;
    if size > i32::MAX as usize {
        return Err("geometry exceeds the 32-bit client's addressable allocation size".into());
    }
    Ok(size as u32)
}

#[repr(C)]
pub(crate) struct Bounds {
    pub bone: u32,
    pub center: [f32; 3],
    pub radius: f32,
}

pub(crate) struct Vertices<'a> {
    pub bytes: &'a [u8],
    pub count: u32,
    pub format: u32,
}

impl Vertices<'_> {
    fn stride(&self) -> usize {
        let mut size = 12;
        if self.format & 4 != 0 {
            size += 12;
        }
        if self.format & 0x20 != 0 {
            size += 4;
        }
        if self.format & 0x40 != 0 {
            size += 4;
        }
        if self.format & 0x10 != 0 {
            size += 8;
        } else if self.format & 0x80 != 0 {
            size += 16;
        }
        if self.format & 0x4000 != 0 {
            size += 16;
        }
        if self.format & 0xf00 != 0 {
            size += 8;
        }
        size
    }

    fn get(&self, index: u32, stride: usize) -> Result<&[u8], String> {
        if index >= self.count {
            return Err(format!("vertex index {index} is out of range"));
        }
        let start = index as usize * stride;
        self.bytes
            .get(start..start + stride)
            .ok_or_else(|| "truncated native vertex array".into())
    }
}

pub(crate) struct Mesh {
    pub indices: Vec<u32>,
    pub descriptors: Vec<u32>,
    pub bounds: Vec<Bounds>,
}

struct Extents {
    min: [f32; 3],
    max: [f32; 3],
    weights: [u64; 32],
}

impl Extents {
    fn new() -> Self {
        Self {
            min: [f32::INFINITY; 3],
            max: [f32::NEG_INFINITY; 3],
            weights: [0; 32],
        }
    }

    fn include(&mut self, vertex: &[u8], skinned: bool) -> Result<(), String> {
        for axis in 0..3 {
            let value = f32::from_le_bytes(vertex[4 * axis..4 * axis + 4].try_into().unwrap());
            if !value.is_finite() {
                return Err("non-finite vertex position".into());
            }
            self.min[axis] = self.min[axis].min(value);
            self.max[axis] = self.max[axis].max(value);
        }
        if skinned {
            let offset = vertex.len() - 8;
            for influence in 0..4 {
                let bone = vertex[offset + influence] as usize;
                let weight = vertex[offset + influence + 4] as u64;
                let total = self
                    .weights
                    .get_mut(bone)
                    .ok_or("bone index exceeds the native 32-bone palette")?;
                *total += weight;
            }
        }
        Ok(())
    }

    fn bounds(&self, radius_scale: f32) -> Bounds {
        let center = std::array::from_fn(|axis| (self.min[axis] + self.max[axis]) * 0.5);
        let delta: [f32; 3] = std::array::from_fn(|axis| self.max[axis] - self.min[axis]);
        let diagonal = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
        let mut bone = 0;
        for index in 1..self.weights.len() {
            if self.weights[index] > self.weights[bone] {
                bone = index;
            }
        }
        Bounds {
            bone: bone as u32,
            center,
            radius: (diagonal + 1.0) * 0.5 * radius_scale,
        }
    }
}

/// Join adjacent strips with the original winding and degenerate connectors.
/// Descriptors contain a DWORD length followed by optional DWORD material and
/// skinning-variant selectors. Geometry and draw counts have no WORD truncation.
pub(crate) fn compile(
    stream: &[u32],
    strip_count: u32,
    flags: u32,
    vertices: &Vertices<'_>,
) -> Result<Mesh, String> {
    let stride = vertices.stride();
    byte_size(vertices.count as usize, stride)?;
    if vertices.bytes.len() < vertices.count as usize * stride {
        return Err("truncated native vertex array".into());
    }
    let descriptor_stride = descriptor_stride(flags);
    if strip_count == 0 || strip_count as usize > stream.len() / (descriptor_stride + 3) {
        return Err("invalid native strip count".into());
    }
    let capacity = stream
        .len()
        .checked_add(3 * strip_count as usize)
        .ok_or("index count overflow")?;
    byte_size(capacity, 4)?;
    byte_size(strip_count as usize, descriptor_stride * 4)?;
    byte_size(strip_count as usize, std::mem::size_of::<Bounds>())?;
    let mut mesh = Mesh {
        indices: Vec::new(),
        descriptors: Vec::new(),
        bounds: Vec::new(),
    };
    mesh.indices
        .try_reserve_exact(capacity)
        .map_err(|e| e.to_string())?;
    mesh.descriptors
        .try_reserve_exact(strip_count as usize * descriptor_stride)
        .map_err(|e| e.to_string())?;
    mesh.bounds
        .try_reserve_exact(strip_count as usize)
        .map_err(|e| e.to_string())?;
    let skinned = vertices.format & 0xf00 != 0 && flags & 0xf00 != 0;
    let radius_scale = if skinned { 2.0 } else { 1.0 };
    let mut cursor = 0;
    let mut batch_start = 0;
    let mut previous_key = None;
    let mut previous_winding = false;
    let mut extents = Extents::new();
    for _ in 0..strip_count {
        let packed = *stream.get(cursor).ok_or("truncated strip header")?;
        cursor += 1;
        let count = (packed & 0x7fff_ffff) as usize;
        let reversed = packed & 0x8000_0000 != 0;
        let mut key = [0; 2];
        for (slot, mask) in [MATERIAL, VARIANT].into_iter().enumerate() {
            if flags & mask != 0 {
                key[slot] = *stream.get(cursor).ok_or("truncated strip selector")?;
                cursor += 1;
            }
        }
        let end = cursor.checked_add(count).ok_or("strip length overflow")?;
        let indices = stream.get(cursor..end).ok_or("truncated strip indices")?;
        if indices.len() < 3 {
            return Err("triangle strip has fewer than three indices".into());
        }
        cursor = end;
        if previous_key != Some(key) {
            if previous_key.is_some() {
                let descriptor = mesh.descriptors.len() - descriptor_stride;
                mesh.descriptors[descriptor] = (mesh.indices.len() - batch_start) as u32;
                mesh.bounds.push(extents.bounds(radius_scale));
            }
            batch_start = mesh.indices.len();
            extents = Extents::new();
            mesh.descriptors.push(0);
            if flags & MATERIAL != 0 {
                mesh.descriptors.push(key[0]);
            }
            if flags & VARIANT != 0 {
                mesh.descriptors.push(key[1]);
            }
            if reversed {
                mesh.indices.push(indices[0]);
            }
        } else {
            let last = *mesh.indices.last().unwrap();
            mesh.indices.push(last);
            if previous_winding == reversed {
                mesh.indices.push(last);
            }
            mesh.indices.push(indices[0]);
        }
        mesh.indices.extend_from_slice(indices);
        for &index in indices {
            extents.include(vertices.get(index, stride)?, skinned)?;
        }
        previous_key = Some(key);
        previous_winding = if count % 2 == 1 { reversed } else { !reversed };
    }
    if cursor != stream.len() {
        return Err("trailing data in native index stream".into());
    }
    let descriptor = mesh.descriptors.len() - descriptor_stride;
    mesh.descriptors[descriptor] = (mesh.indices.len() - batch_start) as u32;
    mesh.bounds.push(extents.bounds(radius_scale));
    Ok(mesh)
}
