//! Online Sign authentication and launch-data preparation.

#[cfg(not(all(windows, target_arch = "x86")))]
compile_error!("mhf-login only supports i686 Windows");

mod config;
mod credentials;
mod model;
mod sign;
mod startup;
mod ui;

use mhf_config::{Config, Registration};
use mhf_mod_host::{Context, LaunchProvider, Module, Result};

#[derive(Default)]
pub struct LoginModule {
    startup: Option<LaunchProvider>,
}

impl LoginModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for LoginModule {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        let table = context.interface("mhf.config", "mhf.config.v1")?;
        // The declared provider dependency outlives this module and its callback.
        let config: Config<'static> = unsafe { mhf_config::bind(table.cast()) };
        config
            .register("sign", &Registration::default())
            .map_err(|error| error.to_string())?;
        let startup = self
            .startup
            .insert(LaunchProvider::fallback(move |params, global_data| {
                startup::run(config, params, global_data)
            }));
        startup.register(context)
    }
}
