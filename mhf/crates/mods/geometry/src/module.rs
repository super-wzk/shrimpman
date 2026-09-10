use crate::native::{GeometryHooks, install};
use mhf_mod_host::{Context, Module, Result};
use windows::Win32::Foundation::HMODULE;

#[derive(Default)]
pub struct GeometryMod {
    hook: Option<GeometryHooks>,
}

impl Module for GeometryMod {
    fn attach(&mut self, context: &Context) -> Result<()> {
        self.hook = Some(unsafe { install(HMODULE(context.game().module_base)) }?);
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            unsafe { hook.prepare_release() }?;
        }
        Ok(())
    }
}
