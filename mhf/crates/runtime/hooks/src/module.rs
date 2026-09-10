use std::sync::{Mutex, PoisonError};

use windows::{
    Win32::{
        Foundation::{FreeLibrary, HMODULE},
        System::LibraryLoader::{GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GetModuleHandleExA},
    },
    core::PCSTR,
};

/// An owned Windows module reference, independent of native callback state.
///
/// After hooks stop, the host can release this reference while keeping retired
/// buffers alive through the final module owner's `FreeLibrary` and `DllMain`.
pub struct ModuleReference {
    base: usize,
    owned: Mutex<bool>,
}

impl ModuleReference {
    /// Retains the loaded module containing `module`.
    ///
    /// # Safety
    /// `module` must identify a loaded module throughout this call.
    pub unsafe fn acquire(module: HMODULE) -> Result<Self, String> {
        let mut retained = HMODULE::default();
        unsafe {
            GetModuleHandleExA(
                GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS,
                PCSTR(module.0.cast()),
                &mut retained,
            )
        }
        .map_err(|error| format!("failed to retain native module: {error}"))?;
        Ok(unsafe { Self::from_owned(retained) })
    }

    /// Takes ownership of an existing module reference without incrementing it.
    ///
    /// # Safety
    /// `module` must own one unreleased reference, obtained from `LoadLibrary`
    /// or a retaining `GetModuleHandleEx` call. Its previous owner must no longer
    /// release that reference.
    pub unsafe fn from_owned(module: HMODULE) -> Self {
        Self {
            base: module.0 as usize,
            owned: Mutex::new(true),
        }
    }

    /// Returns the module's original base address, including after release.
    pub fn base(&self) -> usize {
        self.base
    }

    /// Releases this reference once without destroying associated callback state.
    /// A failed release retains ownership so the caller can retry.
    ///
    /// # Safety
    /// All callers that rely on this reference must have stopped. Restore native
    /// pointers before the final module owner is released, and retain any buffers
    /// that `DllMain` may inspect until that final release has completed.
    pub unsafe fn release(&self) -> Result<(), String> {
        let mut owned = self.owned.lock().unwrap_or_else(PoisonError::into_inner);
        if *owned {
            unsafe { FreeLibrary(HMODULE(self.base as *mut _)) }
                .map_err(|error| format!("failed to release native module: {error}"))?;
            *owned = false;
        }
        Ok(())
    }
}

impl Drop for ModuleReference {
    fn drop(&mut self) {
        if let Err(error) = unsafe { self.release() } {
            eprintln!("native module cleanup failed: {error}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryA};

    #[test]
    fn explicit_release_is_idempotent_and_does_not_consume_other_owners() {
        let loaded = unsafe { LoadLibraryA(windows::core::s!("version.dll")) }.unwrap();
        let owner = unsafe { ModuleReference::from_owned(loaded) };
        let retained = unsafe { ModuleReference::acquire(loaded) }.unwrap();
        assert_eq!(retained.base(), owner.base());
        unsafe { retained.release() }.unwrap();
        unsafe { retained.release() }.unwrap();
        drop(retained);
        assert!(
            unsafe { GetProcAddress(loaded, windows::core::s!("GetFileVersionInfoSizeW")) }
                .is_some()
        );
        unsafe { owner.release() }.unwrap();
    }
}
