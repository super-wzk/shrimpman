use super::super::Client;
use mhf_hooks::{ModuleReference, PatchReservation};
use std::{ffi::c_void, ptr, slice};
use windows::Win32::{
    Foundation::HMODULE,
    System::{
        Diagnostics::Debug::FlushInstructionCache,
        Memory::{PAGE_EXECUTE_READWRITE, PAGE_PROTECTION_FLAGS, VirtualProtect},
        Threading::GetCurrentProcess,
    },
};

const ADDRESS: usize = 0x114d_8924;
const ORIGINAL: [u8; 5] = [0x8b, 0x51, 0x44, 0x6a, 0x00];
// Jump to 114D8947 before either Present call has pushed its arguments.
// Keep the preceding Clear and the following clock/resource initialization.
const REPLACEMENT: [u8; 5] = [0xe9, 0x1e, 0x00, 0x00, 0x00];

pub(super) struct ResetPresentation {
    address: usize,
    module: Option<ModuleReference>,
    reservation: Option<PatchReservation>,
    protection: Option<PAGE_PROTECTION_FLAGS>,
    applied: bool,
}

impl ResetPresentation {
    /// Install before the game's threads start; the client must remain loaded.
    pub(super) unsafe fn install(client: Client) -> Result<Self, String> {
        let module = unsafe { ModuleReference::acquire(HMODULE(client.base as *mut c_void)) }?;
        unsafe { Self::at(client.address(ADDRESS), Some(module)) }
    }

    unsafe fn at(address: usize, module: Option<ModuleReference>) -> Result<Self, String> {
        if unsafe { slice::from_raw_parts(address as *const u8, ORIGINAL.len()) } != ORIGINAL {
            return Err("不支持此游戏 DLL 的重置呈现接口：0x114d8924".into());
        }
        let mut guard = Self {
            address,
            module,
            reservation: Some(PatchReservation::reserve(
                "workbench reset presentation",
                address,
                ORIGINAL.len(),
            )?),
            protection: None,
            // A failed cache flush/protection restore must still undo the copy.
            applied: true,
        };
        unsafe { guard.write(&REPLACEMENT) }?;
        Ok(guard)
    }

    /// Restore after the game's callers stop and the hook callbacks drain.
    pub(super) fn restore(&mut self) -> Result<(), String> {
        if self.applied {
            unsafe { self.write(&ORIGINAL) }?;
            self.applied = false;
        }
        if let Some(module) = &self.module {
            unsafe { module.release() }?;
        }
        self.module = None;
        if let Some(reservation) = self.reservation.take() {
            reservation.release();
        }
        Ok(())
    }

    unsafe fn write(&mut self, bytes: &[u8; 5]) -> Result<(), String> {
        let address = self.address as *mut c_void;
        let mut previous = PAGE_PROTECTION_FLAGS::default();
        unsafe { VirtualProtect(address, bytes.len(), PAGE_EXECUTE_READWRITE, &mut previous) }
            .map_err(|error| format!("无法写入工作台重置呈现指令：{error}"))?;
        // Retain the original page protection if an earlier restoration failed.
        let original = *self.protection.get_or_insert(previous);
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), address.cast(), bytes.len()) };
        let mut ignored = PAGE_PROTECTION_FLAGS::default();
        let protection = unsafe { VirtualProtect(address, bytes.len(), original, &mut ignored) };
        if protection.is_ok() {
            self.protection = None;
        }
        let cache =
            unsafe { FlushInstructionCache(GetCurrentProcess(), Some(address), bytes.len()) };
        protection
            .and(cache)
            .map_err(|error| format!("无法完成工作台重置呈现指令更新：{error}"))
    }
}

impl Drop for ResetPresentation {
    fn drop(&mut self) {
        if let Err(error) = self.restore() {
            eprintln!("workbench reset presentation cleanup failed: {error}");
            // PatchReservation remains occupied when dropped without release.
            if let Some(module) = self.module.take() {
                std::mem::forget(module);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, MEMORY_BASIC_INFORMATION, PAGE_EXECUTE_READ,
        PAGE_READWRITE, VirtualAlloc, VirtualFree, VirtualQuery,
    };

    struct Page(*mut c_void);

    impl Page {
        fn protection(&self) -> PAGE_PROTECTION_FLAGS {
            let mut information = MEMORY_BASIC_INFORMATION::default();
            assert_ne!(
                unsafe {
                    VirtualQuery(
                        Some(self.0),
                        &mut information,
                        std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
                    )
                },
                0
            );
            information.Protect
        }
    }

    impl Drop for Page {
        fn drop(&mut self) {
            let _ = unsafe { VirtualFree(self.0, 0, MEM_RELEASE) };
        }
    }

    #[test]
    fn patch_skips_present_arguments_and_restores_only_its_owned_span() {
        let page =
            Page(unsafe { VirtualAlloc(None, 4096, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE) });
        assert!(!page.0.is_null());
        let mut original = [0xcc_u8; 80];
        original[16..21].copy_from_slice(&ORIGINAL);
        // The jump must land after both original Present blocks, at a return.
        let continuation = 16 + 5 + 0x1e;
        original[continuation..continuation + 6]
            .copy_from_slice(&[0xb8, 0x40, 0x30, 0x20, 0x10, 0xc3]);
        let mut ignored = PAGE_PROTECTION_FLAGS::default();
        unsafe {
            ptr::copy_nonoverlapping(original.as_ptr(), page.0.cast(), original.len());
            VirtualProtect(page.0, 4096, PAGE_EXECUTE_READ, &mut ignored).unwrap();
            FlushInstructionCache(GetCurrentProcess(), Some(page.0), original.len()).unwrap();
        }
        let address = page.0 as usize + 16;
        let mut patch = unsafe { ResetPresentation::at(address, None) }.unwrap();
        assert_eq!(page.protection(), PAGE_EXECUTE_READ);
        let installed = unsafe { slice::from_raw_parts(page.0.cast::<u8>(), original.len()) };
        assert_eq!(&installed[..16], &original[..16]);
        assert_eq!(&installed[21..], &original[21..]);
        let execute: unsafe extern "C" fn() -> u32 = unsafe { std::mem::transmute(address) };
        assert_eq!(unsafe { execute() }, 0x1020_3040);
        assert!(PatchReservation::reserve("overlap", address + 4, 1).is_err());
        PatchReservation::reserve("adjacent", address + 5, 1)
            .unwrap()
            .release();

        patch.restore().unwrap();
        patch.restore().unwrap();
        assert_eq!(page.protection(), PAGE_EXECUTE_READ);
        assert_eq!(
            unsafe { slice::from_raw_parts(page.0.cast::<u8>(), original.len()) },
            original
        );
        PatchReservation::reserve("restored", address, 5)
            .unwrap()
            .release();
    }
}
