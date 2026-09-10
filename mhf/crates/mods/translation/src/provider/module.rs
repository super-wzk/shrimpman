use super::TranslationService;
use crate::{INTERFACE_ID, TranslationConfig, TranslationTable};
use mhf_config::Registration;
use mhf_mod_host::{Context, Module, Result};
use std::rc::Rc;

/// Owns the immutable dictionary table through consumer destruction.
#[derive(Default)]
pub struct TranslationMod {
    service: Option<Rc<TranslationService>>,
}

impl TranslationMod {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Module for TranslationMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        let table = context.interface("mhf.config", "mhf.config.v1")?;
        // The host keeps the configuration provider alive through our destruction.
        let config = unsafe { mhf_config::bind(table.cast()) };
        config
            .register("translation", &Registration::default())
            .map_err(|error| error.to_string())?;
        let section = config
            .read("translation")
            .map_err(|error| error.to_string())?;
        let values: toml::Table = toml::from_str(&section)
            .map_err(|error| format!("invalid translation settings: {error}"))?;
        let settings: Option<TranslationConfig> = if values.is_empty() {
            None
        } else {
            Some(
                values
                    .try_into()
                    .map_err(|error| format!("invalid translation settings: {error}"))?,
            )
        };
        let service = self
            .service
            .insert(Rc::new(TranslationService::new(settings.as_ref())?));
        // SAFETY: Rc keeps the immutable table outside lifecycle &mut borrows;
        // the host destroys dependent consumers before dropping this provider.
        unsafe {
            context.register(
                INTERFACE_ID,
                (service.api() as *const TranslationTable).cast(),
            )
        }
    }
}
