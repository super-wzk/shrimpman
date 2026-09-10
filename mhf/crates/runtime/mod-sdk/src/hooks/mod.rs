use crate::{Host, Result};
use crate::{abi as api, host::check};
use std::ffi::c_void;

#[inline]
pub fn hooks(host: Host<'_>) -> Hooks<'_> {
    Hooks { host }
}

pub struct Hooks<'host> {
    host: Host<'host>,
}

impl<'host> Hooks<'host> {
    /// # Safety
    /// The state must remain valid until successful group cleanup. drain must
    /// stop and wait for all detour invocations and contain panics. No callback
    /// may still use trampoline or mod state after drain returns OK.
    pub unsafe fn prepare_group(
        self,
        name: &str,
        state: *mut c_void,
        drain: Option<api::DrainFn>,
    ) -> Result<HookGroup<'host>> {
        let mut raw = std::ptr::null_mut();
        let status = unsafe {
            (self.host.raw.hooks.prepare_group)(
                self.host.raw.context,
                api::Str::new(name),
                state,
                drain,
                &mut raw,
            )
        };
        check(self.host, status)?;
        Ok(HookGroup {
            host: self.host,
            raw,
        })
    }
}

/// A reference to a group owned by the host. Dropping this handle does not remove
/// enabled hooks; the host coordinates cleanup with the Mod lifecycle.
pub struct HookGroup<'host> {
    host: Host<'host>,
    raw: api::HookGroup,
}

impl HookGroup<'_> {
    /// # Safety
    /// Target must be a verified executable entry and detour must have its exact
    /// signature/calling convention. Store the trampoline before enabling; the
    /// detour must contain panics and obey the group's drain contract.
    pub unsafe fn create_hook(
        &mut self,
        target: *mut c_void,
        detour: *mut c_void,
    ) -> Result<*mut c_void> {
        let mut trampoline = std::ptr::null_mut();
        let status = unsafe {
            (self.host.raw.hooks.create_hook)(
                self.host.raw.context,
                self.raw,
                target,
                detour,
                &mut trampoline,
            )
        };
        check(self.host, status)?;
        Ok(trampoline)
    }

    pub fn enable(&mut self) -> Result<()> {
        let status = unsafe { (self.host.raw.hooks.enable_group)(self.host.raw.context, self.raw) };
        check(self.host, status)
    }

    pub fn discard(self) -> Result<()> {
        let status =
            unsafe { (self.host.raw.hooks.discard_group)(self.host.raw.context, self.raw) };
        check(self.host, status)
    }
}
