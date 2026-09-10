use crate::native::MonsterPatches;
use mhf_mod_host::{Context, Module, Result};
use windows::Win32::Foundation::HMODULE;

#[derive(Default)]
pub struct MonsterMod {
    patches: Option<MonsterPatches>,
}

impl Module for MonsterMod {
    fn attach(&mut self, context: &Context) -> Result<()> {
        if self.patches.is_some() {
            return Err("monster patches are already attached".into());
        }
        let patches = unsafe { MonsterPatches::prepare(HMODULE(context.game().module_base)) }?;
        // Base must own the state before a partial write can fail.
        self.patches.insert(patches).apply()
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(patches) = &mut self.patches {
            patches.restore()?;
        }
        Ok(())
    }

    fn prepare_release(&mut self, _context: &Context) -> Result<()> {
        if let Some(patches) = &self.patches {
            unsafe { patches.prepare_release() }?;
        }
        Ok(())
    }
}
