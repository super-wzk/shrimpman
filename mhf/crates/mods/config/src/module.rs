//! Lifecycle of the configuration service and the game's INI bridge.

use crate::{ConfigService, ConfigTable, INTERFACE_ID, Store, native};
use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module};
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

pub struct ConfigMod {
    store: Arc<Mutex<Store>>,
    ini_name: String,
    hook: Option<HookGuard<native::HookState>>,
    // Published table storage stays outside lifecycle &mut borrows.
    service: Rc<ConfigService>,
}

impl ConfigMod {
    pub fn new(store: Arc<Mutex<Store>>, ini_name: String) -> Self {
        Self {
            service: Rc::new(ConfigService::new(store.clone())),
            store,
            ini_name,
            hook: None,
        }
    }
}

impl Module for ConfigMod {
    fn prepare(&mut self, context: &Context) -> Result<(), String> {
        unsafe {
            context.register(
                INTERFACE_ID,
                (self.service.api() as *const ConfigTable).cast(),
            )?;
        }
        self.hook = Some(native::install(&self.ini_name, self.store.clone())?);
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<(), String> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }
}
