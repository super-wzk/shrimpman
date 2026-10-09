//! GPU indices, draw descriptors and culling bounds use the same 32-bit data.

use std::{cell::Cell, ops::Deref};

pub(crate) const MATERIAL: u32 = 0x10_0000;
pub(crate) const VARIANT: u32 = 0x20_0000;
const MESH_CACHE_LIMIT: usize = 8 * 1024 * 1024;

thread_local! {
    static MESH_CACHE: Cell<Option<Mesh>> = const { Cell::new(None) };
}

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

    #[inline(always)]
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

#[derive(Default)]
pub(crate) struct Mesh {
    pub indices: Vec<u32>,
    pub descriptors: Vec<u32>,
    pub bounds: Vec<Bounds>,
}

impl Mesh {
    fn capacity_bytes(&self) -> Option<usize> {
        [
            (self.indices.capacity(), std::mem::size_of::<u32>()),
            (self.descriptors.capacity(), std::mem::size_of::<u32>()),
            (self.bounds.capacity(), std::mem::size_of::<Bounds>()),
        ]
        .into_iter()
        .try_fold(0usize, |total, (capacity, stride)| {
            total.checked_add(capacity.checked_mul(stride)?)
        })
    }
}

pub(crate) struct CompiledMesh(Mesh);

impl Deref for CompiledMesh {
    type Target = Mesh;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for CompiledMesh {
    fn drop(&mut self) {
        let mut mesh = std::mem::take(&mut self.0);
        mesh.indices.clear();
        mesh.descriptors.clear();
        mesh.bounds.clear();
        let Some(bytes) = mesh.capacity_bytes() else {
            return;
        };
        if bytes > MESH_CACHE_LIMIT {
            return;
        }
        // A reentrant construction owns separate storage. Retain only the
        // larger returned mesh, and never access a destroyed thread-local slot.
        let _ = MESH_CACHE.try_with(|slot| {
            if let Some(cached) = slot.take()
                && cached
                    .capacity_bytes()
                    .is_some_and(|cached_bytes| (bytes..=MESH_CACHE_LIMIT).contains(&cached_bytes))
            {
                mesh = cached;
            }
            slot.set(Some(mesh));
        });
    }
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

    #[inline(always)]
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
) -> Result<CompiledMesh, String> {
    // Take ownership before compiling or calling native code. A nested call can
    // use its own mesh without borrowing or overwriting the outer call's data.
    let mesh = MESH_CACHE
        .try_with(Cell::take)
        .ok()
        .flatten()
        .unwrap_or_default();
    let mut compiled = CompiledMesh(mesh);
    let mesh = &mut compiled.0;
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
    Ok(compiled)
}

#[cfg(test)]
mod tests {
    use super::*;

    const POSITIONS: [u8; 36] = [0; 36];
    const STREAM: [u32; 4] = [3, 0, 1, 2];

    fn vertices() -> Vertices<'static> {
        Vertices {
            bytes: &POSITIONS,
            count: 3,
            format: 1,
        }
    }

    fn clear_cache() {
        MESH_CACHE.with(Cell::take);
    }

    fn cached_bytes() -> Option<usize> {
        MESH_CACHE.with(|slot| {
            let mesh = slot.take()?;
            assert!(mesh.indices.is_empty());
            assert!(mesh.descriptors.is_empty());
            assert!(mesh.bounds.is_empty());
            let bytes = mesh.capacity_bytes();
            slot.set(Some(mesh));
            bytes
        })
    }

    fn addresses(mesh: &Mesh) -> [usize; 3] {
        [
            mesh.indices.as_ptr() as usize,
            mesh.descriptors.as_ptr() as usize,
            mesh.bounds.as_ptr() as usize,
        ]
    }

    #[test]
    fn completed_mesh_storage_is_reused_without_previous_output() {
        clear_cache();
        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        let pointers = addresses(&mesh);
        assert_eq!(mesh.indices, [0, 1, 2]);
        drop(mesh);
        assert!(cached_bytes().is_some());

        let mesh = compile(&[0x8000_0003, 2, 1, 0], 1, 0, &vertices()).unwrap();
        assert_eq!(addresses(&mesh), pointers);
        assert_eq!(mesh.indices, [2, 2, 1, 0]);
        assert_eq!(mesh.descriptors, [4]);
        assert_eq!(mesh.bounds.len(), 1);
        assert_eq!(mesh.bounds[0].center, [0.0; 3]);
        assert_eq!(mesh.bounds[0].radius, 0.5);
    }

    #[test]
    fn errors_and_unwinding_return_cleared_storage() {
        clear_cache();
        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        let pointers = addresses(&mesh);
        drop(mesh);

        assert!(compile(&[3, 0, 1, 3], 1, 0, &vertices()).is_err());
        assert!(cached_bytes().is_some());
        let result = std::panic::catch_unwind(|| {
            let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
            assert_eq!(addresses(&mesh), pointers);
            panic!("simulate unwinding after native upload preparation");
        });
        assert!(result.is_err());
        assert!(cached_bytes().is_some());

        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        assert_eq!(addresses(&mesh), pointers);
        assert_eq!(mesh.indices, [0, 1, 2]);
        assert_eq!(mesh.descriptors, [3]);
        assert_eq!(mesh.bounds.len(), 1);
    }

    #[test]
    fn reentrant_compiles_keep_independent_data_and_retain_larger_storage() {
        for inner_first in [true, false] {
            clear_cache();
            let outer = compile(&[3, 0, 1, 2, 3, 2, 1, 0], 2, 0, &vertices()).unwrap();
            let pointers = addresses(&outer);
            let indices = outer.indices.clone();
            let inner = compile(&[0x8000_0003, 2, 1, 0], 1, 0, &vertices()).unwrap();
            for (outer, inner) in pointers.into_iter().zip(addresses(&inner)) {
                assert_ne!(outer, inner);
            }
            assert_eq!(outer.indices, indices);
            assert_eq!(inner.indices, [2, 2, 1, 0]);
            if inner_first {
                drop(inner);
                assert_eq!(outer.indices, indices);
                drop(outer);
            } else {
                drop(outer);
                assert_eq!(inner.indices, [2, 2, 1, 0]);
                drop(inner);
            }
            let next = compile(&STREAM, 1, 0, &vertices()).unwrap();
            assert_eq!(addresses(&next), pointers);
        }
    }

    #[test]
    fn threads_reuse_their_own_mesh_storage() {
        clear_cache();
        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        let pointers = addresses(&mesh);
        std::thread::spawn(move || {
            let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
            let child_pointers = addresses(&mesh);
            for (parent, child) in pointers.into_iter().zip(child_pointers) {
                assert_ne!(parent, child);
            }
            drop(mesh);
            let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
            assert_eq!(addresses(&mesh), child_pointers);
        })
        .join()
        .unwrap();
        assert_eq!(mesh.indices, [0, 1, 2]);
        drop(mesh);
        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        assert_eq!(addresses(&mesh), pointers);
    }

    #[test]
    fn aggregate_capacity_over_budget_is_discarded_without_evicting_cached_mesh() {
        clear_cache();
        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        let pointers = addresses(&mesh);
        drop(mesh);

        let oversized = Mesh {
            indices: Vec::with_capacity(MESH_CACHE_LIMIT / 8),
            descriptors: Vec::with_capacity(MESH_CACHE_LIMIT / 8),
            bounds: Vec::with_capacity(1),
        };
        assert!(oversized.capacity_bytes().unwrap() > MESH_CACHE_LIMIT);
        assert!(oversized.indices.capacity() * std::mem::size_of::<u32>() < MESH_CACHE_LIMIT);
        assert!(oversized.descriptors.capacity() * std::mem::size_of::<u32>() < MESH_CACHE_LIMIT);
        drop(CompiledMesh(oversized));
        assert!(cached_bytes().unwrap() <= MESH_CACHE_LIMIT);

        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        assert_eq!(addresses(&mesh), pointers);
    }

    #[test]
    fn warmed_mesh_growing_over_budget_is_released_after_compilation() {
        clear_cache();
        drop(compile(&STREAM, 1, 0, &vertices()).unwrap());
        assert!(cached_bytes().is_some());

        let bytes_per_strip = 10 * std::mem::size_of::<u32>() + std::mem::size_of::<Bounds>();
        let strip_count = (MESH_CACHE_LIMIT / bytes_per_strip + 1) as u32;
        let stream: Vec<_> = (0..strip_count)
            .flat_map(|strip| [3, strip % 2, 0, 1, 2])
            .collect();
        let mesh = compile(&stream, strip_count, MATERIAL, &vertices()).unwrap();
        assert_eq!(mesh.bounds.len(), strip_count as usize);
        assert!(mesh.capacity_bytes().unwrap() > MESH_CACHE_LIMIT);
        drop(mesh);
        assert_eq!(cached_bytes(), None);

        let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
        assert_eq!(mesh.indices, [0, 1, 2]);
        drop(mesh);
        assert!(cached_bytes().unwrap() <= MESH_CACHE_LIMIT);
    }

    #[test]
    fn mesh_guard_can_drop_after_the_thread_cache_is_destroyed() {
        thread_local! {
            static HOLDER: Cell<Option<CompiledMesh>> = const { Cell::new(None) };
        }
        std::thread::spawn(|| {
            // Register HOLDER's destructor first, so MESH_CACHE is destroyed
            // before HOLDER drops its guard when this thread exits.
            HOLDER.with(|_| {});
            let mesh = compile(&STREAM, 1, 0, &vertices()).unwrap();
            HOLDER.with(|slot| slot.set(Some(mesh)));
        })
        .join()
        .unwrap();
    }
}
