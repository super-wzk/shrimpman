use crate::patches::PATCHES;
use std::{ffi::c_void, ptr};
use windows::Win32::System::{
    Diagnostics::Debug::FlushInstructionCache,
    Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
    Threading::GetCurrentProcess,
};

pub(super) unsafe fn get<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}

pub(super) unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

pub(super) unsafe fn validate(base: usize) -> Result<(), String> {
    if base == 0 || unsafe { get::<u16>(base) } != 0x5a4d {
        return Err("geometry requires a loaded i686 game DLL".into());
    }
    let pe = unsafe { get::<u32>(base + 0x3c) } as usize;
    if pe > 0x1000
        || unsafe { get::<u32>(base + pe) } != 0x4550
        || unsafe { get::<u16>(base + pe + 4) } != 0x14c
        || unsafe { get::<u16>(base + pe + 24) } != 0x10b
        || unsafe { get::<u32>(base + pe + 8) } != 0x5d6d7357
        // The distributed DLL includes additional loader sections. Its game
        // RVAs and signatures are unchanged; the mapped image can be larger.
        || unsafe { get::<u32>(base + pe + 24 + 56) } < 0x0f11c000
    {
        return Err("32-bit geometry supports the verified ZZ HD client only".into());
    }
    for (rva, bytes) in [
        (
            0x2af0,
            &[0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d, 0x0c, 0x8b][..],
        ),
        (
            0x7b60,
            &[0x55, 0x8b, 0xec, 0x81, 0xec, 0x44, 0x03, 0x00, 0x00][..],
        ),
        (
            0x7120,
            &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x34, 0x53, 0x8b][..],
        ),
        (
            0x0158ffd0,
            &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x14, 0xff, 0x15][..],
        ),
        (
            0x015ab67e,
            &[0x8b, 0xff, 0x55, 0x8b, 0xec, 0x53, 0x8b, 0x5d][..],
        ),
        (
            0x015ab644,
            &[0x8b, 0xff, 0x55, 0x8b, 0xec, 0x83, 0x7d, 0x08][..],
        ),
        (
            0x8220,
            &[0x55, 0x8b, 0xec, 0x83, 0xec, 0x14, 0xf7, 0x40][..],
        ),
        (
            0x82f0,
            &[0x55, 0x8b, 0xec, 0x51, 0x56, 0x8b, 0xf0, 0xf7][..],
        ),
    ] {
        unsafe { check(base + rva, bytes) }?;
    }
    for patch in PATCHES {
        unsafe { check(base + patch.rva, patch.original) }?;
    }
    Ok(())
}

pub(super) unsafe fn check(address: usize, bytes: &[u8]) -> Result<(), String> {
    if unsafe { std::slice::from_raw_parts(address as *const u8, bytes.len()) } != bytes {
        return Err(format!(
            "unsupported or already modified geometry instruction at {address:#010x}"
        ));
    }
    Ok(())
}

/// Install/uninstall only while the game's callers are stopped. Patches retain
/// their original instruction spans, so relative branches need no relocation.
pub(super) struct CodePatches {
    base: usize,
    applied: usize,
    reservations: Vec<mhf_hooks::PatchReservation>,
}

impl CodePatches {
    pub unsafe fn install(base: usize) -> Result<Self, String> {
        let mut guard = Self {
            base,
            applied: 0,
            reservations: Vec::new(),
        };
        for patch in PATCHES {
            guard
                .reservations
                .push(mhf_hooks::PatchReservation::reserve(
                    "geometry instruction",
                    base + patch.rva,
                    patch.replacement.len(),
                )?);
            // Record the current edit before writing: cache/protection errors
            // after the copy must still restore this instruction during rollback.
            guard.applied += 1;
            unsafe { write(base, patch.rva, patch.replacement) }?;
        }
        Ok(guard)
    }

    pub fn restore(&mut self) -> Result<(), String> {
        while self.applied > 0 {
            let patch = &PATCHES[self.applied - 1];
            unsafe { write(self.base, patch.rva, patch.original) }?;
            self.reservations
                .pop()
                .expect("applied patch owns its reservation")
                .release();
            self.applied -= 1;
        }
        Ok(())
    }
}

impl Drop for CodePatches {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("geometry patch cleanup failed: {error}");
        }
    }
}

pub(super) unsafe fn write(base: usize, rva: usize, bytes: &[u8]) -> Result<(), String> {
    let address = (base + rva) as *mut c_void;
    let mut previous = PAGE_PROTECTION_FLAGS::default();
    unsafe { VirtualProtect(address, bytes.len(), PAGE_EXECUTE_READWRITE, &mut previous) }
        .map_err(|e| format!("protect geometry instruction {rva:#x}: {e}"))?;
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast(), bytes.len()) };
    let mut ignored = PAGE_PROTECTION_FLAGS::default();
    let protection = unsafe { VirtualProtect(address, bytes.len(), previous, &mut ignored) };
    let cache = unsafe { FlushInstructionCache(GetCurrentProcess(), Some(address), bytes.len()) };
    protection
        .and(cache)
        .map_err(|e| format!("finish geometry instruction {rva:#x}: {e}"))
}
