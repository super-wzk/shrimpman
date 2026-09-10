//! Temporary hunter startup and the in-game debugging tools.

#[cfg(not(all(windows, target_arch = "x86")))]
compile_error!("mhf-debug only supports i686 Windows");

use mhf_debug_tools::DebugToolsMod;
use mhf_mod_host::{Context, LaunchProvider, Module, Result};
use mhf_ui::OverlayRegistry;
use serde::Deserialize;
use std::path::{Path, PathBuf};

pub struct DebugModule {
    startup: Option<LaunchProvider>,
    tools: DebugToolsMod,
}

impl DebugModule {
    pub fn new(registry: OverlayRegistry) -> Self {
        Self {
            startup: None,
            tools: DebugToolsMod::new(registry),
        }
    }
}

#[derive(Deserialize)]
struct Settings {
    quest: Option<PathBuf>,
}

impl Module for DebugModule {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        self.tools.prepare(context)?;
        let settings: Settings = toml::from_str(context.config())
            .map_err(|error| format!("invalid debug settings: {error}"))?;
        let path = settings
            .quest
            .map(|path| Path::new(context.resource_root()).join(path));
        let table = context.interface(mhf_quest::PROVIDER_ID, mhf_quest::LAUNCH_INTERFACE_ID)?;
        // The Base dependency retains the launch table through this callback's destruction.
        let launch: mhf_quest::QuestLaunch<'static> =
            unsafe { mhf_quest::bind_launch(table.cast()) };
        let startup = self
            .startup
            .insert(LaunchProvider::new(move |params, _global_data| {
                let bytes = match &path {
                    Some(path) => {
                        let bytes = std::fs::read(path).map_err(|error| {
                            format!("failed to read {}: {error}", path.display())
                        })?;
                        if bytes.is_empty() {
                            return Err(format!("quest file {} is empty", path.display()));
                        }
                        bytes
                    }
                    None => include_bytes!("../resources/quests/test-map.bin").to_vec(),
                };
                launch
                    .prepare_local(&bytes)
                    .map_err(|error| error.to_string())?;
                params.selected_character_id_1 = 1;
                params.selected_character_id_2 = 1;
                params.character_ids.fill(0);
                params.character_ids[0] = 1;
                params.fixed_1d58_one = 1;
                params.fixed_200c_one = 1;
                params.selected_character_name.fill(0);
                params.selected_character_name[..5].copy_from_slice(b"Debug");
                Ok(true)
            }));
        startup.register(context)
    }

    fn check(&mut self, context: &Context) -> Result<()> {
        self.tools.check(context)
    }

    fn attach(&mut self, context: &Context) -> Result<()> {
        self.tools.attach(context)
    }

    fn stop(&mut self, context: &Context) -> Result<()> {
        self.tools.stop(context)
    }

    fn detach(&mut self, context: &Context) -> Result<()> {
        self.tools.detach(context)
    }

    fn prepare_release(&mut self, context: &Context) -> Result<()> {
        self.tools.prepare_release(context)
    }
}
