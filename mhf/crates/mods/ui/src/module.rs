use crate::{
    HostIme, INTERFACE_ID, InputCaptureState, OverlayRegistry, UiHostTable, UiService, native,
};
use mhf_mod_host::{Context, Module, Result};
use std::{cell::RefCell, rc::Rc, sync::Arc};

/// UI component of Base, sharing its native editor and input capture state.
pub struct UiMod {
    registry: OverlayRegistry,
    service: Rc<UiService>,
    hook: Option<native::OverlayHook>,
    ime_adapter: Rc<RefCell<Option<Arc<dyn HostIme>>>>,
    capture: Rc<RefCell<Option<InputCaptureState>>>,
}

impl UiMod {
    /// The editor is populated during check and read during attach. Capture is
    /// published after successful attachment for consumers to install their own
    /// input adapters. Those consumers must stop before this provider stops.
    pub fn new(
        registry: OverlayRegistry,
        ime_adapter: Rc<RefCell<Option<Arc<dyn HostIme>>>>,
        capture: Rc<RefCell<Option<InputCaptureState>>>,
    ) -> Self {
        Self {
            service: Rc::new(UiService::new(registry.clone())),
            registry,
            hook: None,
            ime_adapter,
            capture,
        }
    }
}

impl Module for UiMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        unsafe {
            context.register(
                INTERFACE_ID,
                (&self.service.api as *const UiHostTable).cast(),
            )
        }
    }

    fn attach(&mut self, _context: &Context) -> Result<()> {
        let adapter = self.ime_adapter.borrow().clone();
        match unsafe { native::install(self.registry.clone(), adapter) } {
            Ok((hook, capture)) => {
                self.hook = Some(hook);
                *self.capture.borrow_mut() = Some(capture);
                Ok(())
            }
            Err(error) => {
                self.hook = Some(error.hook);
                Err(error.error)
            }
        }
    }

    fn stop(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        self.capture.borrow_mut().take();
        Ok(())
    }
}
