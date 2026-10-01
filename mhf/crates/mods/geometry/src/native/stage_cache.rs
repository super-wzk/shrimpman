//! Grow stage buffers at native read/decode boundaries, before consumers run.

use super::{BASE, SLOT, State, abi, memory};
use crate::{
    cache::{Buffer, Completion, Use},
    stage_cache::{
        self, BUILD_CALLERS, DECODE_CALLERS, DECODED_GLOBAL, ORIGINAL_RAW, PATCHES, RAW_GLOBAL,
        SHARED_RESET, shared_reset,
    },
};
use mhf_hooks::HookSet;
use std::{
    ffi::{CStr, c_void},
    mem::transmute,
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{
        PoisonError,
        atomic::{AtomicUsize, Ordering},
    },
};

const HD_LOAD: usize = 0x0089f121;
const SD_LOAD: usize = 0x0089f14f;
const DECODE: usize = 0x008df4b0;
pub(super) static HD_RETURN: AtomicUsize = AtomicUsize::new(0);
pub(super) static SD_RETURN: AtomicUsize = AtomicUsize::new(0);
pub(super) static FSKL_POINTER: AtomicUsize = AtomicUsize::new(0);

// Valid zero-object FMOD and zero-bone FSKL, also used for empty segments.
const EMPTY_FMOD: [u32; 6] = [1, 1, 24, 2, 0, 12];
const EMPTY_FSKL: [u32; 6] = [0xc0000000, 1, 24, 0, 0, 12];

pub(super) unsafe fn validate(base: usize) -> Result<(), String> {
    unsafe { memory::check(base + SHARED_RESET, &shared_reset(base)) }?;
    for patch in PATCHES {
        unsafe { memory::check(base + patch.rva, &patch.original_at(base)) }?;
    }
    for caller in BUILD_CALLERS {
        let mut call = [0xe8, 0, 0, 0, 0];
        call[1..].copy_from_slice(&(0x008f88e0i32 - caller as i32).to_le_bytes());
        unsafe { memory::check(base + caller - 5, &call) }?;
    }
    unsafe {
        memory::check(base + HD_LOAD, &[0xe8, 0x2a, 0x37, 0x04, 0x00])?;
        memory::check(base + SD_LOAD, &[0xe8, 0x7c, 0x36, 0x04, 0x00])?;
        memory::check(
            base + DECODE,
            &[
                0x55, 0x8b, 0xec, 0x83, 0xe4, 0xf8, 0x8b, 0x4e, 0x04, 0x03, 0xce,
            ],
        )?;
        memory::check(
            base + 0xb0b0,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x08, 0x01, 0x00, 0x00],
        )?;
        let mut campaign = [0xa1, 0, 0, 0, 0, 0x8b, 0x0d, 0, 0, 0, 0];
        campaign[1..5].copy_from_slice(&((base + 0x01c06f10) as u32).to_le_bytes());
        campaign[7..11].copy_from_slice(&((base + 0x01c06f08) as u32).to_le_bytes());
        memory::check(base + 0x003d1090, &campaign)?;
    }
    Ok(())
}

pub(super) unsafe fn create(hooks: &mut HookSet<State>, base: usize) -> Result<usize, String> {
    unsafe {
        hooks.create(
            "stage HD PAC loading",
            (base + HD_LOAD) as _,
            abi::stage_hd_load_detour as *mut c_void,
        )?;
        hooks.create(
            "stage PAC loading",
            (base + SD_LOAD) as _,
            abi::stage_load_detour as *mut c_void,
        )?;
        let original = hooks.create(
            "stage decoded model buffers",
            (base + DECODE) as _,
            abi::stage_decode_detour as *mut c_void,
        )?;
        HD_RETURN.store(base + HD_LOAD + 5, Ordering::Release);
        SD_RETURN.store(base + SD_LOAD + 5, Ordering::Release);
        Ok(original as usize)
    }
}

pub(super) struct Caches {
    base: usize,
    raw: Buffer,
    fmod: Buffer,
    fskl: Buffer,
    empty: Buffer,
    empty_address: usize,
    original_raw: usize,
    original_decoded: usize,
    bound: bool,
    reservations: Vec<mhf_hooks::PatchReservation>,
    pending: Option<[Use; 2]>,
}

impl Caches {
    /// Install before the game entrypoint, while all native callers are stopped.
    pub(super) unsafe fn install(base: usize) -> Result<Self, String> {
        let mut guard = Self {
            base,
            raw: Buffer::default(),
            fmod: Buffer::default(),
            fskl: Buffer::default(),
            empty: Buffer::default(),
            empty_address: 0,
            original_raw: unsafe { memory::get(base + RAW_GLOBAL) },
            original_decoded: unsafe { memory::get(base + DECODED_GLOBAL) },
            bound: false,
            reservations: Vec::new(),
            pending: None,
        };
        guard.empty_address = guard.empty.prepare(48)? as usize;
        unsafe {
            memory::put(guard.empty_address, EMPTY_FMOD);
            memory::put(guard.empty_address + 24, EMPTY_FSKL);
        }
        // A partial instruction install must not expose the previous guard's cell.
        FSKL_POINTER.store(guard.empty_address + 24, Ordering::Release);
        guard
            .reservations
            .try_reserve_exact(PATCHES.len())
            .map_err(|e| e.to_string())?;
        for patch in PATCHES {
            guard
                .reservations
                .push(mhf_hooks::PatchReservation::reserve(
                    "stage cache instruction",
                    base + patch.rva,
                    patch.original.len(),
                )?);
            // Record before a write that may fail after copying bytes.
            unsafe {
                memory::write(
                    base,
                    patch.rva,
                    &patch.replacement(base, &FSKL_POINTER as *const _ as usize),
                )
            }?;
        }
        guard.bound = true;
        unsafe { memory::put(base + RAW_GLOBAL, base + ORIGINAL_RAW) };
        guard.reject_pair();
        Ok(guard)
    }

    fn prepare_raw(&mut self, required: usize) -> Result<*mut u8, String> {
        let raw = self.raw.prepare(required)?;
        unsafe { memory::put(self.base + RAW_GLOBAL, raw as usize) };
        Ok(raw)
    }

    fn prepare_pair(&mut self, sizes: [usize; 2]) -> Result<[usize; 2], String> {
        self.pending = None;
        let fmod = self.fmod.prepare(sizes[0].max(24))? as usize;
        let fskl = self.fskl.prepare(sizes[1].max(24))? as usize;
        unsafe {
            memory::put(fmod, EMPTY_FMOD);
            memory::put(fskl, EMPTY_FSKL);
        }
        // Both allocations succeeded before publishing either destination.
        self.publish(fmod, fskl);
        self.pending = self
            .fmod
            .begin_use(fmod)
            .zip(self.fskl.begin_use(fskl))
            .map(|(a, b)| [a, b]);
        self.fmod.reclaim(fmod);
        self.fskl.reclaim(fskl);
        Ok([fmod, fskl])
    }

    fn publish(&self, fmod: usize, fskl: usize) {
        unsafe { memory::put(self.base + DECODED_GLOBAL, fmod) };
        FSKL_POINTER.store(fskl, Ordering::Release);
    }

    fn reject_pair(&mut self) {
        self.pending = None;
        self.publish(self.empty_address, self.empty_address + 24);
    }

    fn finish(&mut self, uses: [Use; 2]) {
        if self.pending == Some(uses) {
            self.pending = None;
            self.fmod.complete(uses[0]);
            self.fskl.complete(uses[1]);
            self.fmod.reclaim(uses[0].address);
            self.fskl.reclaim(uses[1].address);
        }
    }

    /// Restore only after all game callers and pending native jobs have stopped.
    pub(super) fn restore(&mut self) -> Result<(), String> {
        while !self.reservations.is_empty() {
            let patch = &PATCHES[self.reservations.len() - 1];
            unsafe { memory::write(self.base, patch.rva, &patch.original_at(self.base)) }?;
            self.reservations
                .pop()
                .expect("stage patch owns its reservation")
                .release();
        }
        if self.bound {
            unsafe {
                memory::put(self.base + RAW_GLOBAL, self.original_raw);
                memory::put(self.base + DECODED_GLOBAL, self.original_decoded);
            }
            self.bound = false;
        }
        Ok(())
    }
}

pub(super) unsafe fn construction<'a>(
    state: &'a State,
    registers: &abi::Registers,
) -> Option<Completion<impl FnOnce() + 'a + use<'a>>> {
    let caller = unsafe { memory::get::<usize>(registers.esp as usize + 4) }
        .wrapping_sub(state.module.base());
    if !BUILD_CALLERS.contains(&caller) {
        return None;
    }
    let uses = state
        .stage_caches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending?;
    if uses[0].address != unsafe { registers.argument(1) } as usize
        || uses[1].address != registers.ecx as usize
    {
        return None;
    }
    Some(Completion::on_return(move || {
        state
            .stage_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .finish(uses);
    }))
}

impl Drop for Caches {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("stage cache cleanup failed: {error}");
        }
        if self.bound || !self.reservations.is_empty() {
            self.raw.retain_for_native();
            self.fmod.retain_for_native();
            self.fskl.retain_for_native();
            self.empty.retain_for_native();
        }
    }
}

pub(super) unsafe extern "C" fn load(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let caller = unsafe { memory::get::<usize>(registers.esp as usize + 4) };
    let reader = if caller == HD_RETURN.load(Ordering::Acquire) {
        0x8e2850
    } else {
        0x8e27d0
    };
    let path = registers.eax as *const u8;
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax = unsafe {
            abi::read_equipment_file(
                BASE.load(Ordering::Acquire) + reader,
                path,
                registers.argument(0) as *mut u8,
            )
        };
        return;
    };
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let path = CStr::from_ptr(path.cast());
        let size: unsafe extern "fastcall" fn(*const u8) -> i32 = transmute(state.address(0xb0b0));
        let file_size = size(path.as_ptr().cast());
        if file_size <= 0 {
            // Preserve the native HD -> SD -> default-stage fallback.
            return Ok(0);
        }
        let stage = memory::get::<u32>(registers.ebp as usize + 8);
        let campaign = if matches!(stage, 244 | 310) {
            let campaign_id: unsafe extern "C" fn() -> u32 = transmute(state.address(0x3d1090));
            let path = format!("campaign\\x{:02}.txb\0", campaign_id() as u16);
            let bytes = size(path.as_ptr());
            (bytes > 0).then_some((bytes as usize, path.len() - 1))
        } else {
            None
        };
        let required = stage_cache::raw_size(file_size as usize, path.to_bytes().len(), campaign)?;
        let buffer = state
            .stage_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prepare_raw(required)?;
        Ok::<_, String>(abi::read_equipment_file(
            state.address(reader),
            path.as_ptr().cast(),
            buffer,
        ))
    }));
    registers.eax = match result {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(error)) => {
            eprintln!("stage PAC cache: {error}");
            0
        }
        Err(_) => {
            eprintln!("stage PAC cache loading panicked");
            0
        }
    };
}

/// The four callers still hold the selected outer PAC entry at this boundary.
unsafe fn pair_source(registers: &abi::Registers, caller: usize) -> Result<&[u8], String> {
    let (outer, index) = match caller {
        0x004114bb | 0x0060e0b9 => (registers.ebx as usize, 0),
        0x0089f19f => (registers.ebx as usize, registers.edx as usize),
        0x0089f3a2 => (registers.ecx as usize, registers.edx as usize + 8),
        _ => return Err("unrecognised stage decoder caller".into()),
    };
    let count = unsafe { memory::get::<u32>(outer) } as usize;
    if index >= count {
        return Err("stage PAC model index out of bounds".into());
    }
    let entry = index
        .checked_mul(8)
        .and_then(|n| n.checked_add(outer))
        .and_then(|n| n.checked_add(4))
        .ok_or("stage PAC entry overflow")?;
    let offset = unsafe { memory::get::<u32>(entry) } as usize;
    let size = unsafe { memory::get::<u32>(entry + 4) } as usize;
    crate::mesh::byte_size(size, 1)?;
    if outer.checked_add(offset) != Some(registers.esi as usize)
        || (registers.esi as usize).checked_add(size).is_none()
    {
        return Err("stage PAC model span overflow".into());
    }
    Ok(unsafe { std::slice::from_raw_parts(registers.esi as *const u8, size) })
}

pub(super) unsafe extern "C" fn decode(registers: *mut abi::Registers) {
    let registers = unsafe { &mut *registers };
    let invocation = SLOT.enter();
    let Some(state) = invocation.state() else {
        registers.eax = unsafe {
            abi::decode_stage_pair(
                BASE.load(Ordering::Acquire) + DECODE,
                registers.eax as usize,
                registers.edi as usize,
                registers.esi as usize,
            )
        };
        return;
    };
    let caller = unsafe { memory::get::<usize>(registers.esp as usize + 4) }
        .wrapping_sub(state.module.base());
    if !DECODE_CALLERS.contains(&caller) {
        registers.eax = unsafe {
            abi::decode_stage_pair(
                state.stage_decode_original,
                registers.eax as usize,
                registers.edi as usize,
                registers.esi as usize,
            )
        };
        return;
    }
    let result = catch_unwind(AssertUnwindSafe(|| unsafe {
        let sizes = stage_cache::pair_sizes(pair_source(registers, caller)?)?;
        let destinations = state
            .stage_caches
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .prepare_pair(sizes)?;
        Ok::<_, String>(abi::decode_stage_pair(
            state.stage_decode_original,
            destinations[0],
            destinations[1],
            registers.esi as usize,
        ))
    }));
    let error = match result {
        Ok(Ok(value)) => {
            registers.eax = value;
            return;
        }
        Ok(Err(error)) => error,
        Err(_) => "stage model decoding panicked".into(),
    };
    state
        .stage_caches
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .reject_pair();
    registers.eax = 0;
    eprintln!("stage model cache: {error}");
}

#[cfg(test)]
pub(super) unsafe fn verify_dynamic_buffers(base: usize) {
    let invocation = SLOT.enter();
    let state = invocation.state().unwrap();
    {
        let mut caches = state.stage_caches.lock().unwrap();
        let old = caches.prepare_raw(crate::stage_cache::RAW_MINIMUM).unwrap();
        unsafe { old.write(0x5a) };
        let new = caches
            .prepare_raw(crate::stage_cache::RAW_MINIMUM * 2)
            .unwrap();
        assert_ne!(old, new);
        assert_eq!(unsafe { old.read() }, 0x5a);
        assert_eq!(
            unsafe { memory::get::<usize>(base + RAW_GLOBAL) },
            new as usize
        );
        assert_eq!(caches.prepare_raw(1024).unwrap(), new);
    }
    let mut previous = [0usize; 2];
    for sizes in [[24usize, 24], [1024 * 1024 + 24, 2 * 1024 * 1024 + 24]] {
        // An outer PAC with nine entries, containing a plaintext FMOD/FSKL pair.
        let mut outer = vec![0u8; 76 + 20 + sizes.iter().sum::<usize>()];
        let root = outer.as_mut_ptr() as usize;
        unsafe {
            memory::put(root, 9u32);
            for index in [0, 1, 8] {
                memory::put(root + 4 + index * 8, 76u32);
                memory::put(root + 8 + index * 8, (outer.len() - 76) as u32);
            }
            memory::put(
                root + 76,
                [
                    2,
                    20,
                    sizes[0] as u32,
                    (20 + sizes[0]) as u32,
                    sizes[1] as u32,
                ],
            );
            memory::put(root + 96, EMPTY_FMOD);
            memory::put(root + 96 + sizes[0], EMPTY_FSKL);
        }
        outer[96 + sizes[0] - 1] = 0x5a;
        outer[96 + sizes[0] + sizes[1] - 1] = 0xa5;
        for caller in DECODE_CALLERS {
            let frame = [0x202u32, (base + caller) as u32];
            let mut registers = abi::Registers {
                edi: 0,
                esi: (root + 76) as u32,
                ebp: 0,
                esp: frame.as_ptr() as u32,
                ebx: root as u32,
                edx: u32::from(caller == 0x0089f19f),
                ecx: root as u32,
                eax: 0,
                flags: 0x202,
            };
            unsafe { decode(&mut registers) };
            let destinations = [
                unsafe { memory::get::<usize>(base + DECODED_GLOBAL) },
                FSKL_POINTER.load(Ordering::Acquire),
            ];
            for index in 0..2 {
                assert_eq!(
                    unsafe { memory::get::<u8>(destinations[index] + sizes[index] - 1) },
                    [0x5a, 0xa5][index]
                );
                if previous[index] != 0 && previous[index] != destinations[index] {
                    assert_eq!(
                        unsafe { memory::get::<u8>(previous[index] + 23) },
                        [0x5a, 0xa5][index]
                    );
                }
            }
            // Reject an invalid directory span without running the native decoder.
            unsafe { memory::put(root + 8, u32::MAX) };
            registers.edx = 0;
            let rejected_frame = [0x202u32, (base + DECODE_CALLERS[0]) as u32];
            registers.esp = rejected_frame.as_ptr() as u32;
            unsafe { decode(&mut registers) };
            let caches = state.stage_caches.lock().unwrap();
            assert_eq!(
                unsafe { memory::get::<[u32; 6]>(memory::get(base + DECODED_GLOBAL)) },
                EMPTY_FMOD
            );
            assert_eq!(
                unsafe { memory::get::<[u32; 6]>(FSKL_POINTER.load(Ordering::Acquire)) },
                EMPTY_FSKL
            );
            assert_eq!(
                unsafe { memory::get::<usize>(base + DECODED_GLOBAL) },
                caches.empty_address
            );
            unsafe { memory::put(root + 8, (outer.len() - 76) as u32) };
            previous = destinations;
        }
    }
    let (old_counts, destinations) = {
        let mut caches = state.stage_caches.lock().unwrap();
        let counts = [
            caches.fmod.allocation_count(),
            caches.fskl.allocation_count(),
        ];
        let destinations = caches
            .prepare_pair([3 * 1024 * 1024, 5 * 1024 * 1024])
            .unwrap();
        (counts, destinations)
    };
    let frame = [
        0x202u32,
        (base + BUILD_CALLERS[0]) as u32,
        0,
        destinations[0] as u32,
    ];
    let registers = abi::Registers {
        edi: 0,
        esi: 0,
        ebp: 0,
        esp: frame.as_ptr() as u32,
        ebx: 0,
        edx: 0,
        ecx: destinations[1] as u32,
        eax: 0,
        flags: 0x202,
    };
    let uses = state.stage_caches.lock().unwrap().pending.unwrap();
    {
        let _completion = unsafe { construction(state, &registers) }
            .expect("matching constructor captures both uses");
        assert_eq!(state.stage_caches.lock().unwrap().pending, Some(uses));
    }
    assert!(state.stage_caches.lock().unwrap().pending.is_none());
    let mut caches = state.stage_caches.lock().unwrap();
    let current = caches
        .prepare_pair([5 * 1024 * 1024, 9 * 1024 * 1024])
        .unwrap();
    assert_eq!(caches.fmod.allocation_count(), old_counts[0] + 1);
    assert_eq!(caches.fskl.allocation_count(), old_counts[1] + 1);
    assert_eq!(
        unsafe { memory::get::<usize>(base + DECODED_GLOBAL) },
        current[0]
    );
    assert_eq!(FSKL_POINTER.load(Ordering::Acquire), current[1]);
    let pending = caches.pending;
    caches.finish(uses);
    assert_eq!(
        caches.pending, pending,
        "late constructor cannot complete a newer pair"
    );
}
