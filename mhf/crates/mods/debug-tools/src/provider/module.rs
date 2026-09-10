use super::{DebugControl, DebugService, State, install, overlay};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module, Result};
use mhf_ui::{OverlayRegistration, OverlayRegistry};
use std::{rc::Rc, sync::Arc};
use windows::Win32::Foundation::HMODULE;

pub struct DebugToolsMod {
    control: Arc<DebugControl>,
    service: Rc<DebugService>,
    registry: OverlayRegistry,
    registration: Option<OverlayRegistration>,
    hook: Option<HookGuard<State>>,
}

impl DebugToolsMod {
    pub fn new(registry: OverlayRegistry) -> Self {
        let control = DebugControl::new();
        Self {
            service: Rc::new(DebugService::new(control.clone())),
            control,
            registry,
            registration: None,
            hook: None,
        }
    }
}

impl Module for DebugToolsMod {
    fn attach(&mut self, context: &Context) -> Result<()> {
        let session = unsafe {
            mhf_quest::bind_control(
                context
                    .interface(mhf_quest::PROVIDER_ID, mhf_quest::CONTROL_INTERFACE_ID)?
                    .cast(),
            )
        };
        self.hook = Some(unsafe {
            install(
                HMODULE(context.game().module_base),
                session,
                self.control.clone(),
            )
        }?);
        self.registration = Some(self.registry.register(overlay(self.control.clone())));
        unsafe {
            context.register(
                crate::api::INTERFACE_ID,
                (self.service.api() as *const crate::api::DebugTable).cast(),
            )
        }
    }

    fn stop(&mut self, _context: &Context) -> Result<()> {
        if let Some(registration) = &mut self.registration {
            registration.unregister();
        }
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        if let Some(state) = self.hook.as_mut().and_then(HookGuard::retired_state_mut) {
            unsafe { state.prepare_release() }?;
        }
        Ok(())
    }
}
