use std::{
    cell::RefCell,
    collections::BTreeMap,
    ffi::c_void,
    sync::{Mutex, PoisonError},
};

use minhook::{MH_STATUS, MinHook};

thread_local! {
    static OWNER: RefCell<String> = RefCell::new("internal".to_owned());
}

struct Owner {
    id: String,
    description: String,
    end: usize,
}
static TARGETS: Mutex<BTreeMap<usize, Owner>> = Mutex::new(BTreeMap::new());

/// Ownership of an explicitly known byte-patch span. Release only after the
/// original bytes have been restored; dropping a failed edit keeps it occupied.
pub struct PatchReservation {
    start: usize,
}

impl PatchReservation {
    pub fn reserve(name: &str, start: usize, length: usize) -> Result<Self, String> {
        let owner = OWNER.with(|owner| owner.borrow().clone());
        let end = start
            .checked_add(length)
            .ok_or("patch range overflows address space")?;
        if length == 0 {
            return Err("cannot reserve an empty patch".into());
        }
        let mut targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((_, existing)) = targets
            .iter()
            .find(|(address, entry)| **address < end && start < entry.end)
        {
            return Err(format!(
                "patch conflict at {start:#x}: {} already occupies this range; requested by {owner} ({name})",
                existing.description
            ));
        }
        targets.insert(
            start,
            Owner {
                id: owner.clone(),
                description: format!("{owner} ({name})"),
                end,
            },
        );
        Ok(Self { start })
    }

    pub fn release(self) {
        TARGETS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove(&self.start);
    }
}

/// Detects a partially installed group whose caller could not retain its guard.
pub fn ensure_released(owner: &str) -> Result<(), String> {
    let targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
    let remaining = targets
        .iter()
        .filter(|(_, entry)| entry.id == owner)
        .map(|(address, entry)| format!("{} at {address:#x}", entry.description))
        .collect::<Vec<_>>();
    if remaining.is_empty() {
        Ok(())
    } else {
        Err(format!("Hook cleanup incomplete: {}", remaining.join(", ")))
    }
}

/// Associates hooks created synchronously during an installation with its Mod.
pub fn with_owner<R>(owner: &str, install: impl FnOnce() -> R) -> R {
    struct Restore(String);
    impl Drop for Restore {
        fn drop(&mut self) {
            OWNER.with(|current| *current.borrow_mut() = std::mem::take(&mut self.0));
        }
    }
    let _restore = Restore(OWNER.with(|current| current.replace(owner.to_owned())));
    install()
}

struct Target {
    address: *mut c_void,
    name: String,
}

/// Process-wide MinHook backend, shared by built-in groups and the Mod C API.
/// The caller must disable and drain callbacks before removing an enabled group.
pub struct NativeGroup {
    owner: String,
    targets: Vec<Target>,
    enabled: bool,
}

impl NativeGroup {
    pub fn new(owner: impl Into<String>) -> Self {
        Self {
            owner: owner.into(),
            targets: Vec::new(),
            enabled: false,
        }
    }

    pub(crate) fn current() -> Self {
        Self::new(OWNER.with(|owner| owner.borrow().clone()))
    }

    pub fn is_empty(&self) -> bool {
        self.targets.is_empty()
    }

    /// # Safety
    /// Target and detour must have matching native ABIs and remain executable
    /// through removal. The returned trampoline is borrowed from this group.
    pub unsafe fn create(
        &mut self,
        name: &str,
        target: *mut c_void,
        detour: *mut c_void,
    ) -> Result<*mut c_void, String> {
        let description = format!("{} ({name})", self.owner);
        {
            let mut targets = TARGETS.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some((_, owner)) = targets
                .iter()
                .find(|(start, entry)| **start <= target as usize && (target as usize) < entry.end)
            {
                return Err(format!(
                    "hook conflict at {target:p}: {} already owns the target; requested by {description}",
                    owner.description
                ));
            }
            // MinHook does not expose its rewritten span. Record the exact
            // entry, not a guessed patch length; explicit byte patches do carry
            // their complete ranges.
            targets.insert(
                target as usize,
                Owner {
                    id: self.owner.clone(),
                    description,
                    end: target as usize + 1,
                },
            );
        }
        let original = match unsafe { MinHook::create_hook(target, detour) } {
            Ok(original) => original,
            Err(status) => {
                TARGETS
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .remove(&(target as usize));
                return Err(hook_error("create", name, status));
            }
        };
        self.targets.push(Target {
            address: target,
            name: name.to_owned(),
        });
        Ok(original)
    }

    /// # Safety
    /// Complete callback state must be published before enabling any target.
    /// An error may leave earlier targets enabled; disable and drain the group.
    pub unsafe fn enable(&mut self) -> Result<(), String> {
        if self.targets.is_empty() {
            return Err("cannot install an empty hook group".to_owned());
        }
        for target in &self.targets {
            unsafe { MinHook::enable_hook(target.address) }
                .map_err(|status| hook_error("enable", &target.name, status))?;
            self.enabled = true;
        }
        Ok(())
    }

    pub fn disable(&self) -> Result<(), String> {
        let mut errors = Vec::new();
        for target in self.targets.iter().rev() {
            if let Err(status) = unsafe { MinHook::disable_hook(target.address) }
                && !matches!(
                    status,
                    MH_STATUS::MH_ERROR_DISABLED | MH_STATUS::MH_ERROR_NOT_CREATED
                )
            {
                errors.push(hook_error("disable", &target.name, status));
            }
        }
        errors_result(errors)
    }

    /// # Safety
    /// Hooks must be disabled and all users of their trampolines stopped.
    pub unsafe fn remove(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        for index in (0..self.targets.len()).rev() {
            let target = &self.targets[index];
            match unsafe { MinHook::remove_hook(target.address) } {
                Err(status) if status != MH_STATUS::MH_ERROR_NOT_CREATED => {
                    errors.push(hook_error("remove", &target.name, status));
                }
                _ => {
                    TARGETS
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .remove(&(target.address as usize));
                    self.targets.remove(index);
                }
            }
        }
        errors_result(errors)
    }
}

impl Drop for NativeGroup {
    fn drop(&mut self) {
        // An enabled group's owner must perform explicit disable/drain/remove.
        if !self.enabled
            && let Err(error) = unsafe { self.remove() }
        {
            eprintln!("hook preparation rollback failed: {error}");
        }
    }
}

fn hook_error(operation: &str, name: &str, status: MH_STATUS) -> String {
    format!("failed to {operation} {name} hook: {status:?}")
}

fn errors_result(errors: Vec<String>) -> Result<(), String> {
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors.join("; "))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patches_reserve_their_full_span_until_explicit_release() {
        let first = with_owner("test.patch-owner", || {
            PatchReservation::reserve("first", 0x7000_0000, 8)
        })
        .unwrap();
        let overlap = with_owner("test.patch-consumer", || {
            PatchReservation::reserve("second", 0x7000_0004, 8)
        });
        assert!(matches!(overlap, Err(error) if error.contains("test.patch-owner")));
        let adjacent = with_owner("test.patch-consumer", || {
            PatchReservation::reserve("adjacent", 0x7000_0008, 4)
        })
        .unwrap();
        assert!(ensure_released("test.patch-owner").is_err());
        first.release();
        adjacent.release();
        assert!(ensure_released("test.patch-owner").is_ok());
        assert!(ensure_released("test.patch-consumer").is_ok());
    }
}
