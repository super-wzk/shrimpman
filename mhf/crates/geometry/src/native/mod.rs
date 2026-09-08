mod abi;
mod memory;

use crate::{
    fmod,
    mesh::{self, Vertices},
};
use memory::{CodePatches, Module, get, put};
use mhf_hooks::{HookGuard, HookSlot};
use std::{
    ffi::c_void,
    mem::{size_of, transmute},
    panic::{AssertUnwindSafe, catch_unwind},
    ptr, slice,
    sync::atomic::{AtomicUsize, Ordering},
};
use windows::{
    Win32::{
        Foundation::{HANDLE, HMODULE},
        Graphics::Direct3D9::{
            D3DCAPS9, D3DFMT_INDEX16, D3DFMT_INDEX32, D3DPOOL_MANAGED, D3DUSAGE_WRITEONLY,
            IDirect3DDevice9, IDirect3DIndexBuffer9, IDirect3DVertexBuffer9,
        },
        System::Threading::{CRITICAL_SECTION, EnterCriticalSection, LeaveCriticalSection},
    },
    core::Interface,
};

const LOAD: usize = 0x2af0;
const BUILD: usize = 0x7b60;
const DEVICE: usize = 0x0e811a3c;
const CAPS: usize = 0x01b7fcf0;
const REGISTRY: usize = 0x01aa3dd8;
static BASE: AtomicUsize = AtomicUsize::new(0);
static SLOT: HookSlot<State> = HookSlot::new();

struct State {
    module: Module,
    load_original: usize,
}

impl State {
    fn address(&self, rva: usize) -> usize {
        self.module.0 + rva
    }

    unsafe fn allocate(&self, size: u32) -> Result<Allocation<'_>, String> {
        let malloc: unsafe extern "C" fn(usize) -> *mut u8 =
            unsafe { transmute(self.address(0x015ab67e)) };
        let pointer = unsafe { malloc(size as usize) };
        if pointer.is_null() {
            return Err(format!("native geometry allocation failed ({size} bytes)"));
        }
        Ok(Allocation {
            pointer,
            state: self,
        })
    }

    unsafe fn free(&self, pointer: *mut u8) {
        let free: unsafe extern "C" fn(*mut u8) = unsafe { transmute(self.address(0x015ab644)) };
        unsafe { free(pointer) };
    }
}

struct Allocation<'a> {
    pointer: *mut u8,
    state: &'a State,
}

impl Allocation<'_> {
    fn into_raw(mut self) -> *mut u8 {
        let pointer = self.pointer;
        self.pointer = ptr::null_mut();
        pointer
    }
}

impl Drop for Allocation<'_> {
    fn drop(&mut self) {
        if !self.pointer.is_null() {
            unsafe { self.state.free(self.pointer) };
        }
    }
}

/// Owns the geometry hooks and their instruction edits independently of other
/// launcher features. Native model allocations retain the game's own ownership.
#[must_use = "dropping this guard removes the geometry extension"]
pub struct GeometryHooks {
    // Restore instructions while the hook state still retains the native DLL.
    patches: CodePatches,
    hooks: HookGuard<State>,
}

impl GeometryHooks {
    /// Stop native game callers before removing the extension.
    pub fn uninstall(&mut self) -> Result<(), String> {
        self.hooks.uninstall()?;
        self.patches.restore()
    }
}

impl Drop for GeometryHooks {
    fn drop(&mut self) {
        if let Err(error) = self.uninstall() {
            eprintln!("geometry cleanup failed: {error}");
        }
    }
}

/// Install 32-bit FMOD geometry for the verified ZZ HD client.
///
/// # Safety
/// `module` must be a loaded game DLL. Install before its game entrypoint runs,
/// and stop all native callers before uninstalling or dropping the guard.
pub unsafe fn install(module: HMODULE) -> Result<GeometryHooks, String> {
    let mut hooks = SLOT.prepare()?;
    let base = module.0 as usize;
    unsafe { memory::validate(base) }?;
    let retained = unsafe { Module::retain(module) }?;
    let load_original = unsafe {
        hooks.create(
            "32-bit FMOD indices",
            (base + LOAD) as _,
            abi::load_detour as *mut c_void,
        )
    }?;
    unsafe {
        hooks.create(
            "32-bit model buffers and batches",
            (base + BUILD) as _,
            abi::build_detour as *mut c_void,
        )
    }?;
    let patches = unsafe { CodePatches::install(base) }?;
    BASE.store(base, Ordering::Release);
    let hooks = unsafe {
        hooks.install(State {
            module: retained,
            load_original: load_original as usize,
        })
    }?;
    Ok(GeometryHooks { patches, hooks })
}

/// Temporary FMOD conversion output, owned and freed by the original caller.
/// Only the index stream changes representation; vertex/material fields keep
/// their native offsets and allocator.
#[repr(C)]
struct Source {
    flags: u32,
    strip_count: u32,
    indices: *mut u8,
    vertex_format: u32,
    vertices: *mut u8,
    vertex_count: u32,
    vertex_bytes: u32,
    index_bytes: u32,
    material_count: u16,
    reserved: u16,
}

/// Header shared with the native renderer, software skinning and destruction.
/// Descriptor cells grow to DWORDs; all header offsets remain unchanged.
#[repr(C)]
#[derive(Default)]
struct Model {
    format: u32,
    flags: u32,
    material_bank: u32,
    allocation_bytes: u32,
    primitive: u32,
    batch_offset: u32,
    batch_count: u32,
    vertex_buffer: u32,
    index_buffer: u32,
    fvf: u32,
    vertex_count: u32,
    vertex_stride: u32,
    reserved_30: u32,
    reserved_34: u32,
    reserved_38: u32,
    cpu_fvf: u32,
    cpu_vertex_count: u32,
    cpu_vertex_stride: u32,
    cpu_vertex_buffer: u32,
    cpu_vertex_offset: u32,
    material_offset: u32,
    reserved_54: u32,
    reserved_58: u32,
    bone_mask: u32,
}

const _: () = assert!(size_of::<Source>() == 36);
const _: () = assert!(size_of::<Model>() == 96);
const _: () = assert!(size_of::<mesh::Bounds>() == 20);

unsafe extern "C" fn load(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let index = registers.eax;
    let source = unsafe { registers.argument(0) } as *mut Source;
    let root = unsafe { registers.argument(1) } as *const u8;
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax =
            unsafe { abi::load_original(BASE.load(Ordering::Acquire) + LOAD, index, source, root) };
        return;
    };
    registers.eax = match catch_unwind(AssertUnwindSafe(|| unsafe {
        load_indices(state, index, source, root)
    })) {
        Ok(Ok(result)) => result,
        Ok(Err(error)) => {
            eprintln!("geometry object {index}: {error}");
            0
        }
        Err(_) => {
            eprintln!("geometry object {index}: index conversion panicked");
            0
        }
    };
}

unsafe fn load_indices(
    state: &State,
    index: u32,
    source: *mut Source,
    root: *const u8,
) -> Result<u32, String> {
    if root.is_null() || source.is_null() {
        return Err("null FMOD input".into());
    }
    let size = unsafe { get::<u32>(root as usize + 8) };
    mesh::byte_size(size as usize, 1)?;
    let geometry = fmod::read(unsafe { slice::from_raw_parts(root, size as usize) }, index)?;
    // The widest existing native vertex conversion uses 72 bytes per vertex.
    // Check before calling it, so its 32-bit allocation multiplication cannot wrap.
    mesh::byte_size(geometry.vertex_count as usize, 72)?;
    // Retain the native vertex, colour, tangent and skin-weight conversions.
    // Their temporary WORD indices are replaced before any consumer sees them.
    if unsafe { abi::load_original(state.load_original, index, source, root) } == 0 {
        return Ok(0);
    }
    let source = unsafe { &mut *source };
    if source.vertex_count != geometry.vertex_count
        || source.strip_count as usize != geometry.strips.len()
    {
        return Err("FMOD selection disagrees with the native converter".into());
    }
    let stream = geometry.encode(source.flags)?;
    let bytes = mesh::byte_size(stream.len(), 4)?;
    let allocation = unsafe { state.allocate(bytes) }?;
    unsafe {
        ptr::copy_nonoverlapping(
            stream.as_ptr().cast::<u8>(),
            allocation.pointer,
            bytes as usize,
        )
    };
    unsafe { state.free(source.indices) };
    source.indices = allocation.into_raw();
    source.index_bytes = bytes;
    Ok(1)
}

unsafe extern "C" fn build(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let source = registers.ecx as *const Source;
    let flags = unsafe { registers.argument(0) };
    let materials = unsafe { registers.argument(1) } as *const u32;
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax = unsafe {
            abi::build_original(
                BASE.load(Ordering::Acquire) + BUILD,
                source,
                flags,
                materials,
            )
        } as u32;
        return;
    };
    registers.eax = match catch_unwind(AssertUnwindSafe(|| unsafe {
        build_model(state, source, flags, materials)
    })) {
        Ok(Ok(handle)) => handle,
        Ok(Err(error)) => {
            eprintln!("geometry upload: {error}");
            u32::MAX
        }
        Err(_) => {
            eprintln!("geometry upload panicked");
            u32::MAX
        }
    };
}

unsafe fn build_model(
    state: &State,
    source: *const Source,
    flags: u32,
    materials: *const u32,
) -> Result<u32, String> {
    let source = unsafe { source.as_ref() }.ok_or("null native geometry source")?;
    if source.indices.is_null() || source.vertices.is_null() || source.index_bytes % 4 != 0 {
        return Err("invalid 32-bit native geometry source".into());
    }
    mesh::byte_size(source.index_bytes as usize, 1)?;
    mesh::byte_size(source.vertex_bytes as usize, 1)?;
    let vertices = Vertices {
        bytes: unsafe { slice::from_raw_parts(source.vertices, source.vertex_bytes as usize) },
        count: source.vertex_count,
        format: source.vertex_format,
    };
    let caps = unsafe { get::<D3DCAPS9>(state.address(CAPS)) };
    let cpu_skinning = caps.MaxVertexShaderConst <= 0x90
        && unsafe { abi::source_query(state.address(0x8220), source) } != 0;
    let mut format = source.flags | source.vertex_format;
    if cpu_skinning {
        format &= !0x100;
    }
    let mesh = mesh::compile(
        unsafe { slice::from_raw_parts(source.indices.cast(), source.index_bytes as usize / 4) },
        source.strip_count,
        format,
        &vertices,
    )?;
    if mesh.indices.iter().copied().max().unwrap_or(0) > caps.MaxVertexIndex {
        return Err(format!(
            "model exceeds the device's MaxVertexIndex ({})",
            caps.MaxVertexIndex
        ));
    }
    let descriptor_stride = mesh::descriptor_stride(source.flags);
    if mesh
        .descriptors
        .chunks_exact(descriptor_stride)
        .any(|batch| batch[0] - 2 > caps.MaxPrimitiveCount)
    {
        return Err(format!(
            "model batch exceeds the device's MaxPrimitiveCount ({})",
            caps.MaxPrimitiveCount
        ));
    }
    let native_fvf: unsafe extern "fastcall" fn(u32) -> u32 =
        unsafe { transmute(state.address(0x7050)) };
    let native_stride: unsafe extern "fastcall" fn(u32) -> u32 =
        unsafe { transmute(state.address(0x70c0)) };
    let original_fvf = unsafe { native_fvf(source.vertex_format) };
    let original_stride = unsafe { native_stride(original_fvf) };
    let fvf = if cpu_skinning {
        original_fvf & 0xffff_fff1 | 2
    } else {
        original_fvf
    };
    let vertex_stride = unsafe { native_stride(fvf) };
    let vertex_bytes = mesh::byte_size(source.vertex_count as usize, vertex_stride as usize)?;
    let cpu_bytes = if cpu_skinning {
        mesh::byte_size(source.vertex_count as usize, original_stride as usize)?
    } else {
        0
    };
    let bounds_bytes = mesh::byte_size(mesh.bounds.len(), size_of::<mesh::Bounds>())?;
    let batch_bytes = mesh::byte_size(mesh.descriptors.len(), 4)?;
    let material_bytes = if flags & 2 != 0 {
        u32::from(source.material_count) * 140
    } else {
        0
    };
    let batch_offset = (size_of::<Model>() as u32)
        .checked_add(bounds_bytes)
        .ok_or("model offset overflow")?;
    let material_offset = batch_offset
        .checked_add(batch_bytes)
        .ok_or("model offset overflow")?;
    let cpu_offset = material_offset
        .checked_add(material_bytes)
        .ok_or("model offset overflow")?;
    let allocation_bytes = cpu_offset
        .checked_add(cpu_bytes)
        .ok_or("model allocation overflow")?;
    let total = mesh::byte_size(allocation_bytes as usize + 16, 1)?;
    let allocation = unsafe { state.allocate(total) }?;
    let model = unsafe { allocation.pointer.add(16) };
    unsafe { ptr::write_bytes(allocation.pointer, 0, total as usize) };
    let materials = if materials.is_null() {
        state.address(0x01b804b0) as *const u32
    } else {
        materials
    };
    unsafe {
        ptr::copy_nonoverlapping(
            mesh.bounds.as_ptr().cast::<u8>(),
            model.add(size_of::<Model>()),
            bounds_bytes as usize,
        );
        ptr::copy_nonoverlapping(
            mesh.descriptors.as_ptr().cast::<u8>(),
            model.add(batch_offset as usize),
            batch_bytes as usize,
        );
        if material_bytes != 0 {
            ptr::copy_nonoverlapping(
                materials.add(2).cast::<u8>(),
                model.add(material_offset as usize),
                material_bytes as usize,
            );
        }
        if cpu_skinning {
            abi::convert_vertices(
                state.address(0x7120),
                model.add(cpu_offset as usize),
                source.vertices,
                original_fvf,
                source.vertex_count,
                source.vertex_format,
            );
        }
    }
    // D3D9 cannot draw INDEX32 buffers when MaxVertexIndex <= 65535. Keep
    // existing small models usable on those devices; all CPU counts stay DWORDs.
    // The maximum-index check above proves that this fallback cannot truncate.
    let index32 = caps.MaxVertexIndex > u16::MAX as u32;
    let mut request = BufferRequest {
        device: unsafe { get::<usize>(state.address(DEVICE)) },
        vertex_bytes,
        index_bytes: mesh::byte_size(mesh.indices.len(), if index32 { 4 } else { 2 })?,
        index32,
        fvf,
        result: None,
    };
    let created = unsafe { abi::schedule(state.address(0x0158ffd0), create_buffers, &mut request) };
    let (vertex_buffer, index_buffer) = match (created, request.result) {
        (_, Some(Err(error))) => return Err(error),
        (0, _) | (_, None) => return Err("native geometry buffer dispatch failed".into()),
        (_, Some(Ok(buffers))) => buffers,
    };
    let mut destination = ptr::null_mut();
    unsafe { vertex_buffer.Lock(0, vertex_bytes, &mut destination, 0) }
        .map_err(|e| format!("lock model vertex buffer: {e}"))?;
    if destination.is_null() {
        let _ = unsafe { vertex_buffer.Unlock() };
        return Err("D3D9 returned a null model vertex buffer mapping".into());
    }
    unsafe {
        abi::convert_vertices(
            state.address(0x7120),
            destination.cast(),
            source.vertices,
            fvf,
            source.vertex_count,
            source.vertex_format,
        )
    };
    unsafe { vertex_buffer.Unlock() }.map_err(|e| format!("unlock model vertex buffer: {e}"))?;
    destination = ptr::null_mut();
    unsafe { index_buffer.Lock(0, request.index_bytes, &mut destination, 0) }
        .map_err(|e| format!("lock model index buffer: {e}"))?;
    if destination.is_null() {
        let _ = unsafe { index_buffer.Unlock() };
        return Err("D3D9 returned a null model index buffer mapping".into());
    }
    unsafe {
        if index32 {
            ptr::copy_nonoverlapping(
                mesh.indices.as_ptr().cast::<u8>(),
                destination.cast(),
                request.index_bytes as usize,
            );
        } else {
            for (offset, &index) in mesh.indices.iter().enumerate() {
                ptr::write(destination.cast::<u16>().add(offset), index as u16);
            }
        }
    };
    unsafe { index_buffer.Unlock() }.map_err(|e| format!("unlock model index buffer: {e}"))?;
    let header = Model {
        format: if cpu_skinning {
            format & !mesh::VARIANT
        } else {
            format
        },
        flags,
        material_bank: if flags & 4 != 0 {
            unsafe { ptr::read(materials) }
        } else {
            0
        },
        allocation_bytes,
        primitive: 5,
        batch_offset,
        batch_count: mesh.bounds.len() as u32,
        fvf,
        vertex_count: source.vertex_count,
        vertex_stride,
        cpu_fvf: if cpu_skinning { original_fvf } else { 0 },
        cpu_vertex_count: if cpu_skinning { source.vertex_count } else { 0 },
        cpu_vertex_stride: if cpu_skinning { original_stride } else { 0 },
        cpu_vertex_offset: if cpu_skinning { cpu_offset } else { 0 },
        material_offset,
        bone_mask: unsafe { abi::source_query(state.address(0x82f0), source) },
        ..Model::default()
    };
    unsafe { ptr::write(model.cast::<Model>(), header) };
    let reserve: unsafe extern "C" fn() -> u32 = unsafe { transmute(state.address(0x7aa0)) };
    let handle = unsafe { reserve() };
    if handle == 0 {
        return Err("native model handle table is full".into());
    }
    // Publish only after all fallible work succeeds. The native cleanup paths
    // release both COM buffers, unlink this 16-byte header, and call this CRT's free.
    unsafe {
        (*model.cast::<Model>()).vertex_buffer = vertex_buffer.into_raw() as u32;
        (*model.cast::<Model>()).index_buffer = index_buffer.into_raw() as u32;
        let critical = state.address(0x0e73acf8) as *mut CRITICAL_SECTION;
        EnterCriticalSection(critical);
        let head = get::<usize>(state.address(0x0e73d370));
        put(allocation.pointer as usize, head);
        put(allocation.pointer as usize + 4, 0usize);
        if head != 0 {
            put(head + 4, allocation.pointer as usize);
        }
        put(state.address(0x0e73d370), allocation.pointer as usize);
        LeaveCriticalSection(critical);
        put(
            state.address(REGISTRY) + 4 * handle as usize,
            model as usize,
        );
    }
    let _ = allocation.into_raw();
    Ok(handle)
}

struct BufferRequest {
    device: usize,
    vertex_bytes: u32,
    index_bytes: u32,
    index32: bool,
    fvf: u32,
    result: Option<Result<(IDirect3DVertexBuffer9, IDirect3DIndexBuffer9), String>>,
}

unsafe extern "C" fn create_buffers(request: *mut BufferRequest) -> i32 {
    let request = unsafe { &mut *request };
    let result = (|| {
        let pointer = request.device as *mut c_void;
        let device = unsafe { IDirect3DDevice9::from_raw_borrowed(&pointer) }
            .ok_or("model D3D9 device is null")?;
        let mut vertex_buffer = None;
        unsafe {
            device.CreateVertexBuffer(
                request.vertex_bytes,
                D3DUSAGE_WRITEONLY as u32,
                request.fvf,
                D3DPOOL_MANAGED,
                &mut vertex_buffer,
                ptr::null_mut::<HANDLE>(),
            )
        }
        .map_err(|e| format!("create model vertex buffer: {e}"))?;
        let vertex_buffer = vertex_buffer.ok_or("D3D9 returned a null model vertex buffer")?;
        let mut index_buffer = None;
        unsafe {
            device.CreateIndexBuffer(
                request.index_bytes,
                D3DUSAGE_WRITEONLY as u32,
                if request.index32 {
                    D3DFMT_INDEX32
                } else {
                    D3DFMT_INDEX16
                },
                D3DPOOL_MANAGED,
                &mut index_buffer,
                ptr::null_mut::<HANDLE>(),
            )
        }
        .map_err(|e| format!("create model index buffer: {e}"))?;
        let index_buffer = index_buffer.ok_or("D3D9 returned a null model index buffer")?;
        Ok((vertex_buffer, index_buffer))
    })();
    let created = result.is_ok();
    request.result = Some(result);
    i32::from(created)
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::{
        Win32::System::LibraryLoader::{LOAD_WITH_ALTERED_SEARCH_PATH, LoadLibraryExW},
        core::PCWSTR,
    };

    #[test]
    #[ignore = "Windows: set MHF_GEOMETRY_TEST_CLIENT; loads the real DLL and its DllMain"]
    fn supported_client_installs_and_restores_geometry() {
        let path = std::env::var("MHF_GEOMETRY_TEST_CLIENT").expect("MHF_GEOMETRY_TEST_CLIENT");
        let path: Vec<_> = path.encode_utf16().chain(Some(0)).collect();
        let module =
            unsafe { LoadLibraryExW(PCWSTR(path.as_ptr()), None, LOAD_WITH_ALTERED_SEARCH_PATH) }
                .expect("load game DLL and its adjacent dependencies");
        let _module = Module(module.0 as usize);
        for _ in 0..2 {
            let mut hooks = unsafe { install(module) }.expect("install geometry hooks");
            assert!(
                unsafe { install(module) }.is_err(),
                "reject duplicate ownership"
            );
            for patch in crate::patches::PATCHES {
                let actual = unsafe {
                    slice::from_raw_parts(
                        (module.0 as usize + patch.rva) as *const u8,
                        patch.replacement.len(),
                    )
                };
                assert_eq!(actual, patch.replacement);
            }
            hooks.uninstall().expect("restore geometry hooks");
            unsafe { memory::validate(module.0 as usize) }
                .expect("all original instructions restored");
        }
    }
}
