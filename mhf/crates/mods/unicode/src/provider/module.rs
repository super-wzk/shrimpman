use super::{HookState, ime, resources};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module, Result};
use mhf_ui::{HostIme, InputCaptureState};
use std::{cell::RefCell, rc::Rc, sync::Arc};
use windows::Win32::Foundation::HMODULE;

/// Native text and IME state, assembled with Base's UI component.
pub struct UnicodeMod {
    translation_enabled: bool,
    ime_adapter: Rc<RefCell<Option<Arc<dyn HostIme>>>>,
    capture: Rc<RefCell<Option<InputCaptureState>>>,
    ime: Option<Arc<ime::GameIme>>,
    ime_hook: Option<HookGuard<ime::HookState>>,
    text: Option<HookGuard<HookState>>,
    resources: Option<HookGuard<resources::HookState>>,
}

impl UnicodeMod {
    pub fn new(
        translation_enabled: bool,
        ime_adapter: Rc<RefCell<Option<Arc<dyn HostIme>>>>,
        capture: Rc<RefCell<Option<InputCaptureState>>>,
    ) -> Self {
        Self {
            translation_enabled,
            ime_adapter,
            capture,
            ime: None,
            ime_hook: None,
            text: None,
            resources: None,
        }
    }
}

impl Module for UnicodeMod {
    fn check(&mut self, context: &Context) -> Result<()> {
        let ime = unsafe { ime::GameIme::new(HMODULE(context.game().module_base)) }?;
        *self.ime_adapter.borrow_mut() = Some(ime.clone());
        self.ime = Some(ime);
        Ok(())
    }

    fn attach(&mut self, context: &Context) -> Result<()> {
        let module = HMODULE(context.game().module_base);
        let capture = self
            .capture
            .borrow()
            .clone()
            .ok_or("Unicode IME requires the initialized built-in UI input state")?;
        let ime = self
            .ime
            .as_ref()
            .ok_or("Unicode IME must be checked before attach")?;
        self.ime_hook = Some(unsafe { ime.install(capture) }?);
        let translation = self
            .translation_enabled
            .then(|| {
                context
                    .interface(mhf_translation::PROVIDER_ID, mhf_translation::INTERFACE_ID)
                    .map(|table| table.cast())
            })
            .transpose()?;
        self.resources = Some(unsafe { resources::install(module, translation) }?);
        self.text = Some(unsafe { super::install(module) }?);
        Ok(())
    }

    fn stop(&mut self, _: &Context) -> Result<()> {
        if let Some(hook) = &mut self.ime_hook {
            hook.uninstall()?;
        }
        Ok(())
    }

    fn detach(&mut self, _: &Context) -> Result<()> {
        let text = self.text.as_mut().map_or(Ok(()), HookGuard::uninstall);
        let resources = self.resources.as_mut().map_or(Ok(()), HookGuard::uninstall);
        match (text, resources) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(error), Ok(())) | (Ok(()), Err(error)) => Err(error),
            (Err(first), Err(second)) => Err(format!("{first}; {second}")),
        }
    }

    fn prepare_release(&mut self, _: &Context) -> Result<()> {
        if let Some(state) = self.text.as_mut().and_then(HookGuard::retired_state_mut) {
            unsafe { state.prepare_release() }?;
        }
        if let Some(state) = self
            .resources
            .as_mut()
            .and_then(HookGuard::retired_state_mut)
        {
            unsafe { state.prepare_release() }?;
        }
        if let Some(ime) = &self.ime {
            unsafe { ime.prepare_release() }?;
        }
        Ok(())
    }
}
