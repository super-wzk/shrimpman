use crate::patches::{PATCHES, Patch};
use mhf_hooks::{ModuleReference, PatchReservation};
use std::{ffi::c_void, ptr};
use windows::Win32::{
    Foundation::HMODULE,
    System::{
        Diagnostics::Debug::FlushInstructionCache,
        Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
        Threading::GetCurrentProcess,
    },
};

struct Edit {
    reservation: PatchReservation,
    // Preserve the original protection across a failed RWX restoration.
    protection: Option<PAGE_PROTECTION_FLAGS>,
}

pub(super) struct MonsterPatches {
    base: usize,
    module: Option<ModuleReference>,
    edits: Vec<Edit>,
    applied: usize,
}

impl MonsterPatches {
    /// Reserve all spans before changing code. The game entrypoint must not
    /// have started, and `module` must remain loaded through this call.
    pub(super) unsafe fn prepare(module: HMODULE) -> Result<Self, String> {
        let base = module.0 as usize;
        unsafe { validate(base) }?;
        let mut patches = Self {
            base,
            module: Some(unsafe { ModuleReference::acquire(module) }?),
            edits: Vec::with_capacity(PATCHES.len()),
            applied: 0,
        };
        for patch in PATCHES {
            patches.edits.push(Edit {
                reservation: PatchReservation::reserve(
                    "monster species limit",
                    base + patch.rva,
                    patch.original.len(),
                )?,
                protection: None,
            });
        }
        Ok(patches)
    }

    pub(super) fn apply(&mut self) -> Result<(), String> {
        if self.applied != 0 || self.edits.len() != PATCHES.len() {
            return Err("monster patches are not ready for installation".into());
        }
        for (patch, edit) in PATCHES.iter().zip(&mut self.edits) {
            // Include this span in rollback even if writing fails after the copy.
            self.applied += 1;
            unsafe { write(self.base, patch, edit, patch.replacement) }?;
        }
        Ok(())
    }

    /// Call only after the game's threads have stopped. Failed restoration
    /// retains the pending edit, all reservations and the DLL for a retry.
    pub(super) fn restore(&mut self) -> Result<(), String> {
        while self.applied > 0 {
            let index = self.applied - 1;
            let patch = &PATCHES[index];
            unsafe { write(self.base, patch, &mut self.edits[index], patch.original) }?;
            self.applied -= 1;
        }
        for edit in self.edits.drain(..) {
            edit.reservation.release();
        }
        Ok(())
    }

    /// Release only after restoration, with all game callers stopped.
    pub(super) unsafe fn prepare_release(&self) -> Result<(), String> {
        if !self.edits.is_empty() {
            return Err("monster patches have not finished detaching".into());
        }
        if let Some(module) = &self.module {
            unsafe { module.release() }?;
        }
        Ok(())
    }
}

impl Drop for MonsterPatches {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("monster patch cleanup failed: {error}");
            // Keep the DLL alive if cleanup fails outside normal host teardown.
            if let Some(module) = self.module.take() {
                std::mem::forget(module);
            }
        }
    }
}

unsafe fn read<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}

unsafe fn validate(base: usize) -> Result<(), String> {
    if base == 0 || unsafe { read::<u16>(base) } != 0x5a4d {
        return Err("monster patches require a loaded i686 game DLL".into());
    }
    let pe = unsafe { read::<u32>(base + 0x3c) } as usize;
    if pe > 0x1000
        || unsafe { read::<u32>(base + pe) } != 0x4550
        || unsafe { read::<u16>(base + pe + 4) } != 0x14c
        || unsafe { read::<u16>(base + pe + 24) } != 0x10b
        || unsafe { read::<u32>(base + pe + 8) } != 0x5d6d7357
        || unsafe { read::<u32>(base + pe + 24 + 56) } < 0x0f11c000
    {
        return Err("monster patches support the verified ZZ HD client only".into());
    }
    for patch in PATCHES {
        let bytes = unsafe {
            std::slice::from_raw_parts((base + patch.rva) as *const u8, patch.original.len())
        };
        if bytes != patch.original {
            return Err(format!(
                "unsupported or already modified monster instruction at RVA {:#010x}",
                patch.rva
            ));
        }
    }
    Ok(())
}

unsafe fn write(base: usize, patch: &Patch, edit: &mut Edit, bytes: &[u8]) -> Result<(), String> {
    let address = (base + patch.rva) as *mut c_void;
    let mut previous = PAGE_PROTECTION_FLAGS::default();
    unsafe { VirtualProtect(address, bytes.len(), PAGE_EXECUTE_READWRITE, &mut previous) }
        .map_err(|error| format!("protect monster instruction {:#x}: {error}", patch.rva))?;
    let original = *edit.protection.get_or_insert(previous);
    unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast(), bytes.len()) };
    let mut ignored = PAGE_PROTECTION_FLAGS::default();
    let protection = unsafe { VirtualProtect(address, bytes.len(), original, &mut ignored) };
    if protection.is_ok() {
        edit.protection = None;
    }
    let cache = unsafe { FlushInstructionCache(GetCurrentProcess(), Some(address), bytes.len()) };
    protection
        .and(cache)
        .map_err(|error| format!("finish monster instruction {:#x}: {error}", patch.rva))
}
