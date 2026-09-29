//! Fixed DPI for the ZZ HD client's 32x32 glyph rasterizer only.

use mhf_hooks::HookSet;
use std::{
    ffi::c_void,
    ptr, slice,
    sync::atomic::{AtomicUsize, Ordering},
};

const DPI_RVA: usize = 0x014D_3325;
// After GetDeviceCaps(LOGPIXELSY): convert EAX to float and back, then pass
// that numerator and 22 points to MulDiv. The denominator (72) is on the stack.
const SIGNATURE: &[u8] = &[
    0x0F, 0x57, 0xC0, 0xF3, 0x0F, 0x2A, 0xC0, 0xF3, 0x0F, 0x2C, 0xC0, 0x50, 0x6A, 0x16,
];
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// The host retains the loaded game DLL and stops native callers before detach.
pub(super) unsafe fn prepare<T: Send + Sync + 'static>(
    hooks: &mut HookSet<T>,
    module: *mut c_void,
) -> Result<(), String> {
    let base = module as usize;
    if base == 0 || unsafe { ptr::read_unaligned(base as *const u16) } != 0x5A4D {
        return Err("font DPI correction requires a loaded i686 game DLL".into());
    }
    let pe = unsafe { ptr::read_unaligned((base + 0x3C) as *const u32) } as usize;
    if pe > 0x1000
        || unsafe { ptr::read_unaligned((base + pe) as *const u32) } != 0x4550
        || unsafe { ptr::read_unaligned((base + pe + 4) as *const u16) } != 0x14C
        || unsafe { ptr::read_unaligned((base + pe + 24) as *const u16) } != 0x10B
        || unsafe { ptr::read_unaligned((base + pe + 8) as *const u32) } != 0x5D6D7357
        || (unsafe { ptr::read_unaligned((base + pe + 24 + 56) as *const u32) } as usize)
            < DPI_RVA + SIGNATURE.len()
    {
        return Err("font DPI correction supports the verified ZZ HD client only".into());
    }
    unsafe { prepare_at(hooks, (base + DPI_RVA) as *mut c_void) }
}

unsafe fn prepare_at<T: Send + Sync + 'static>(
    hooks: &mut HookSet<T>,
    target: *mut c_void,
) -> Result<(), String> {
    if unsafe { slice::from_raw_parts(target.cast::<u8>(), SIGNATURE.len()) } != SIGNATURE {
        return Err("unsupported or already modified glyph DPI instruction".into());
    }
    let original =
        unsafe { hooks.create("96 DPI glyph rasterization", target, detour as *mut c_void) }?;
    ORIGINAL.store(original as usize, Ordering::Release);
    Ok(())
}

// This is an instruction hook, not a function entry. Preserve the native stack,
// flags and registers except EAX, which is the DPI returned by GetDeviceCaps.
// The trampoline replays the displaced conversions and resumes MulDiv(22,96,72).
#[unsafe(naked)]
unsafe extern "C" fn detour() {
    core::arch::naked_asm!(
        "mov eax, 96",
        "jmp dword ptr [{original}]",
        original = sym ORIGINAL,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhf_hooks::HookSlot;

    static SLOT: HookSlot<()> = HookSlot::new();

    #[unsafe(naked)]
    unsafe extern "C" fn native_height(_dpi: i32) -> i32 {
        core::arch::naked_asm!(
            "mov eax, [esp + 4]",
            "push 72",
            "xorps xmm0, xmm0",
            "cvtsi2ss xmm0, eax",
            "cvttss2si eax, xmm0",
            "push eax",
            "push 22",
            "call {mul_div}",
            "neg eax",
            "ret",
            mul_div = sym windows_sys::Win32::System::WindowsProgramming::MulDiv,
        );
    }

    #[test]
    fn glyph_height_stays_at_96_dpi_and_uninstall_restores_scaling() {
        let cases = [(96, -29), (120, -37), (144, -44), (192, -59)];
        for (dpi, expected) in cases {
            assert_eq!(unsafe { native_height(dpi) }, expected);
        }
        let target = (native_height as *const () as usize + 6) as *mut c_void;
        let mut hooks = SLOT.prepare().unwrap();
        assert!(unsafe { prepare_at(&mut hooks, target.cast::<u8>().add(1).cast()) }.is_err());
        unsafe { prepare_at(&mut hooks, target) }.unwrap();
        let mut guard = unsafe { hooks.install(()) }.unwrap();
        for (dpi, _) in cases {
            assert_eq!(unsafe { native_height(dpi) }, -29);
        }
        guard.uninstall().unwrap();
        for (dpi, expected) in cases {
            assert_eq!(unsafe { native_height(dpi) }, expected);
        }
    }
}
