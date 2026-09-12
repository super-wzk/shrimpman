//! Optional read-through replacement of files beneath the game's dat directory.

#![cfg(windows)]

mod native;
mod paths;

use mhf_hooks::HookGuard;
use mhf_mod_host::{Context, Module, Result};
use serde::Deserialize;
use std::path::PathBuf;

pub struct DatRedirectMod {
    game_dir: PathBuf,
    hook: Option<HookGuard<native::State>>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Settings {
    #[serde(default = "default_root")]
    root: PathBuf,
}

fn default_root() -> PathBuf {
    PathBuf::from("dat-redirect")
}

impl DatRedirectMod {
    pub fn new(game_dir: PathBuf) -> Self {
        Self {
            game_dir,
            hook: None,
        }
    }
}

impl Module for DatRedirectMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        let settings: Settings = toml::from_str(context.config())
            .map_err(|error| format!("invalid dat-redirect settings: {error}"))?;
        let paths = paths::Paths::new(&self.game_dir, &settings.root)?;
        // Install before loading the game DLL so its initialization reads are covered.
        self.hook = Some(native::install(paths)?);
        Ok(())
    }

    fn detach(&mut self, _context: &Context) -> Result<()> {
        if let Some(hook) = &mut self.hook {
            hook.uninstall()?;
        }
        Ok(())
    }
}
