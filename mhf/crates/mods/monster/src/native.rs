//! What both monster features share: typed access to the loaded game image and
//! the fingerprint of the one build this crate supports.
//!
//! Everything here runs inside the game process, so the module only exists on
//! the provider targets. The build is fixed — `mhfo-hd.dll`, image base
//! `0x10000000`, SHA-256
//! `95c580195f4080d2e9582c8c9df36abeb280476e088b6366583c5f138da8f301` — and
//! every patch RVA, hook signature and actor offset in this crate holds for
//! that build alone.

use std::ptr;

pub(crate) unsafe fn read<T: Copy>(address: usize) -> T {
    unsafe { ptr::read_unaligned(address as *const T) }
}

pub(crate) unsafe fn put<T>(address: usize, value: T) {
    unsafe { ptr::write_unaligned(address as *mut T, value) }
}

/// Check the mapped image's PE header before any feature writes to it.
///
/// The header alone cannot prove the build: a feature still compares the bytes
/// it is about to replace against its own table afterwards.
///
/// # Safety
///
/// `base` must be the base address of a mapped image that stays loaded for the
/// duration of the call.
pub(crate) unsafe fn verify_image(base: usize) -> Result<(), String> {
    if base == 0 || unsafe { read::<u16>(base) } != 0x5a4d {
        return Err("the monster adapter requires a loaded i686 game DLL".into());
    }
    let pe = unsafe { read::<u32>(base + 0x3c) } as usize;
    if pe > 0x1000
        || unsafe { read::<u32>(base + pe) } != 0x4550
        // machine i386, optional header PE32
        || unsafe { read::<u16>(base + pe + 4) } != 0x14c
        || unsafe { read::<u16>(base + pe + 24) } != 0x10b
        // the verified build's timestamp and a size-of-image lower bound
        || unsafe { read::<u32>(base + pe + 8) } != 0x5d6d7357
        || unsafe { read::<u32>(base + pe + 24 + 56) } < 0x0f11c000
    {
        return Err("the monster adapter supports the verified ZZ HD client only".into());
    }
    Ok(())
}
