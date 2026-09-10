//! Owned MinHook groups with callback draining and transactional installation.

#![cfg(windows)]

use std::{
    ffi::{CStr, c_void},
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, TryLockError},
};

mod backend;
pub use backend::{NativeGroup, PatchReservation, ensure_released, with_owner};
mod module;
pub use module::ModuleReference;

#[cfg(test)]
use minhook::MinHook;
use windows::{
    Win32::System::LibraryLoader::{GetModuleHandleA, GetProcAddress},
    core::PCSTR,
};

/// A set of disabled hooks. Failed preparation removes only hooks it created.
pub struct HookSet<T: Send + Sync + 'static> {
    native: NativeGroup,
    slot: &'static HookSlot<T>,
    // Reserve the slot before resolving targets or publishing native pointers.
    preparation: Option<MutexGuard<'static, ()>>,
}

impl<T: Send + Sync + 'static> HookSet<T> {
    /// Creates a disabled hook and returns its trampoline.
    ///
    /// # Safety
    /// The target and detour must be executable and have matching ABIs. Keep
    /// their modules loaded until removal, and do not modify these hooks through
    /// another MinHook handle. Trampolines must not escape the group's lifetime.
    pub unsafe fn create(
        &mut self,
        name: &str,
        target: *mut c_void,
        detour: *mut c_void,
    ) -> Result<*mut c_void, String> {
        unsafe { self.native.create(name, target, detour) }
    }

    /// Resolves an exported function, then creates an owned, disabled hook.
    ///
    /// # Safety
    /// The same requirements as [`Self::create`] apply to the resolved function.
    pub unsafe fn create_api(
        &mut self,
        module: &CStr,
        name: &CStr,
        detour: *mut c_void,
    ) -> Result<*mut c_void, String> {
        let handle = unsafe { GetModuleHandleA(PCSTR(module.as_ptr().cast())) }
            .map_err(|error| format!("failed to find {}: {error}", module.to_string_lossy()))?;
        let target =
            unsafe { GetProcAddress(handle, PCSTR(name.as_ptr().cast())) }.ok_or_else(|| {
                format!(
                    "failed to find {}!{}",
                    module.to_string_lossy(),
                    name.to_string_lossy()
                )
            })?;
        unsafe { self.create(&name.to_string_lossy(), target as *mut c_void, detour) }
    }

    /// Publishes callback state before enabling only this set's targets.
    ///
    /// # Safety
    /// Detours must hold an [`Invocation`] across every use of their state and
    /// trampoline, and call the unhooked target when the slot is empty. Lifecycle
    /// operations must not be initiated inside a detour. Native assembly shims
    /// using a trampoline outside an invocation require the host to stop callers
    /// before uninstalling or dropping the returned guard.
    pub unsafe fn install(self, state: T) -> Result<HookGuard<T>, String> {
        if self.native.is_empty() {
            return Err("cannot install an empty hook group".to_owned());
        }
        {
            let mut callbacks = self
                .slot
                .callbacks
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            callbacks.occupied = true;
            callbacks.state = Some(Arc::new(state));
        }
        let mut guard = HookGuard {
            hooks: self,
            retired_state: None,
        };
        if let Err(error) = unsafe { guard.hooks.native.enable() } {
            return Err(match guard.uninstall() {
                Ok(()) => error,
                Err(cleanup) => format!("{error}; cleanup also failed: {cleanup}"),
            });
        }
        drop(guard.hooks.preparation.take());
        Ok(guard)
    }
}

/// Owns an installed group. Drop attempts the same cleanup as explicit removal.
#[must_use = "dropping the guard uninstalls its hooks"]
pub struct HookGuard<T: Send + Sync + 'static> {
    hooks: HookSet<T>,
    // Keep any pointers handed to the host valid until the owner is dropped.
    retired_state: Option<Arc<T>>,
}

impl<T: Send + Sync + 'static> HookGuard<T> {
    /// Access stopped state while keeping its native-facing buffers allocated.
    /// Only available after successful removal and callback draining.
    pub fn retired_state_mut(&mut self) -> Option<&mut T> {
        if !self.hooks.native.is_empty() {
            return None;
        }
        self.retired_state.as_mut().and_then(Arc::get_mut)
    }

    /// Disables the group, drains callbacks, and removes its trampolines.
    /// Cleanup failures retain the state needed by hooks that remain enabled.
    pub fn uninstall(&mut self) -> Result<(), String> {
        if self.hooks.native.is_empty() {
            return Ok(());
        }
        // Failed installation already owns the lifecycle lock.
        let _lifecycle = self.hooks.preparation.take().unwrap_or_else(|| {
            self.hooks
                .slot
                .lifecycle
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        });
        self.hooks.native.disable()?;
        if self.retired_state.is_none() {
            self.retired_state = self.hooks.slot.retire();
        }
        unsafe { self.hooks.native.remove() }?;
        self.hooks
            .slot
            .callbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .occupied = false;
        Ok(())
    }
}

impl<T: Send + Sync + 'static> Drop for HookGuard<T> {
    fn drop(&mut self) {
        if let Err(error) = self.uninstall() {
            eprintln!("hook cleanup failed: {error}");
            // Removal may fail after the slot has retired. Keep those native
            // pointers valid even when an owner cannot retain the guard itself.
            if let Some(state) = self.retired_state.take() {
                std::mem::forget(state);
            }
        }
    }
}

/// A process-local callback slot shared by a group and its detours.
pub struct HookSlot<T> {
    lifecycle: Mutex<()>,
    callbacks: Mutex<Callbacks<T>>,
    idle: Condvar,
}

struct Callbacks<T> {
    occupied: bool,
    state: Option<Arc<T>>,
    active: usize,
}

impl<T> HookSlot<T> {
    pub const fn new() -> Self {
        Self {
            lifecycle: Mutex::new(()),
            callbacks: Mutex::new(Callbacks {
                occupied: false,
                state: None,
                active: 0,
            }),
            idle: Condvar::new(),
        }
    }

    /// Reserves this group for preparation, installation, and rollback.
    /// A group whose cleanup failed must finish removal before reuse.
    pub fn prepare(&'static self) -> Result<HookSet<T>, String>
    where
        T: Send + Sync + 'static,
    {
        let preparation = match self.lifecycle.try_lock() {
            Ok(guard) => guard,
            Err(TryLockError::Poisoned(error)) => error.into_inner(),
            Err(TryLockError::WouldBlock) => {
                return Err("hook group is being installed or removed".to_owned());
            }
        };
        if self
            .callbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .occupied
        {
            return Err("hook group is already installed or awaiting cleanup".to_owned());
        }
        Ok(HookSet {
            native: NativeGroup::current(),
            slot: self,
            preparation: Some(preparation),
        })
    }

    /// Pins the current state and trampoline until this invocation returns.
    /// Empty slots let late detours fall back to the already unhooked target.
    pub fn enter(&self) -> Invocation<'_, T> {
        let mut callbacks = self
            .callbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let state = callbacks.state.clone();
        if state.is_some() {
            callbacks.active += 1;
        }
        Invocation { slot: self, state }
    }

    fn retire(&self) -> Option<Arc<T>> {
        let mut callbacks = self
            .callbacks
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let state = callbacks.state.take();
        while callbacks.active != 0 {
            callbacks = self
                .idle
                .wait(callbacks)
                .unwrap_or_else(PoisonError::into_inner);
        }
        state
    }
}

impl<T> Default for HookSlot<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// A callback's state lease, including its original-function call.
pub struct Invocation<'a, T> {
    slot: &'a HookSlot<T>,
    state: Option<Arc<T>>,
}

impl<T> Invocation<'_, T> {
    pub fn state(&self) -> Option<&T> {
        self.state.as_deref()
    }
}

impl<T> Drop for Invocation<'_, T> {
    fn drop(&mut self) {
        if self.state.take().is_some() {
            let mut callbacks = self
                .slot
                .callbacks
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            callbacks.active -= 1;
            if callbacks.active == 0 {
                self.slot.idle.notify_all();
            }
        }
    }
}

#[cfg(all(test, target_arch = "x86"))]
mod tests;
