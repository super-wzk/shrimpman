use crate::{FontService, FontTable, HookState, INTERFACE_ID, TextRenderer, native};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module, Result};
use std::{ffi::CString, rc::Rc};

/// Font component of Base. Text conversion callbacks are supplied by Base.
pub struct FontMod {
    name: CString,
    // Keep published storage outside later exclusive lifecycle borrows.
    service: Rc<FontService>,
    registration: Option<native::Registration>,
    hook: Option<HookGuard<HookState>>,
    renderer: Option<TextRenderer>,
}

impl FontMod {
    pub fn new(name: String, renderer: Option<TextRenderer>) -> Result<Self> {
        Ok(Self {
            name: CString::new(name.clone()).map_err(|error| error.to_string())?,
            service: Rc::new(FontService::new(name)),
            registration: None,
            hook: None,
            renderer,
        })
    }
}

impl Module for FontMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        self.registration =
            native::register_for(self.name.to_str().map_err(|error| error.to_string())?)?;
        unsafe { context.register(INTERFACE_ID, (&self.service.api as *const FontTable).cast()) }
    }

    fn attach(&mut self, _context: &Context) -> Result<()> {
        self.hook =
            Some(unsafe { native::install_game(self.name.as_bytes_with_nul(), self.renderer) }?);
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }
}
