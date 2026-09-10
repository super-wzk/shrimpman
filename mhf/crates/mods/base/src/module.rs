use crate::{MhfConfig, register_config};
use mhf_font::FontMod;
use mhf_geometry::GeometryMod;
use mhf_mod_host::{Context, Module, Result};
use mhf_quest::QuestMod;
use mhf_ui::{OverlayRegistry, UiMod};
use std::{cell::RefCell, rc::Rc};

/// One runtime owner for game settings, fonts, UI, geometry and quest services.
/// Children retain their callback storage until the host releases the game DLL.
pub struct BaseMod {
    quest: QuestMod,
    font: FontMod,
    geometry: GeometryMod,
    ui: UiMod,
    registry: OverlayRegistry,
}

impl BaseMod {
    pub fn new(settings: &MhfConfig) -> Result<Self> {
        let registry = OverlayRegistry::default();
        let ime_adapter = Rc::new(RefCell::new(None));
        let capture = Rc::new(RefCell::new(None));
        Ok(Self {
            quest: QuestMod::new(),
            font: FontMod::new(settings.font.name.clone(), None)?,
            geometry: GeometryMod::default(),
            ui: UiMod::new(registry.clone(), ime_adapter, capture),
            registry,
        })
    }

    /// Shared Rust registry for built-in overlays in the same host binary.
    /// External Mods use the published `mhf.ui.v1` capability instead.
    pub fn registry(&self) -> OverlayRegistry {
        self.registry.clone()
    }
}

impl Module for BaseMod {
    fn prepare(&mut self, context: &Context) -> Result<()> {
        let table = context.interface(mhf_config::PROVIDER_ID, mhf_config::INTERFACE_ID)?;
        // The declared configuration dependency retains its table through Base's lifecycle.
        let config = unsafe { mhf_config::bind(table.cast::<mhf_config::ConfigTable>()) };
        register_config(config)?;
        self.font
            .prepare(context)
            .map_err(|error| format!("font: {error}"))?;
        self.ui
            .prepare(context)
            .map_err(|error| format!("UI: {error}"))?;
        self.quest
            .prepare(context)
            .map_err(|error| format!("quest: {error}"))
    }

    fn attach(&mut self, context: &Context) -> Result<()> {
        self.font
            .attach(context)
            .map_err(|error| format!("font: {error}"))?;
        self.geometry
            .attach(context)
            .map_err(|error| format!("geometry: {error}"))?;
        self.ui
            .attach(context)
            .map_err(|error| format!("UI: {error}"))?;
        self.quest
            .attach(context)
            .map_err(|error| format!("quest: {error}"))
    }

    fn stop(&mut self, context: &Context) -> Result<()> {
        self.quest
            .stop(context)
            .map_err(|error| format!("quest: {error}"))?;
        self.ui
            .stop(context)
            .map_err(|error| format!("UI: {error}"))
    }

    fn detach(&mut self, context: &Context) -> Result<()> {
        // Guards are idempotent. On failure the host retains this entire Base,
        // including already-retired buffers, and retries cleanup before release.
        self.quest
            .detach(context)
            .map_err(|error| format!("quest: {error}"))?;
        self.font
            .detach(context)
            .map_err(|error| format!("font: {error}"))?;
        self.geometry
            .detach(context)
            .map_err(|error| format!("geometry: {error}"))?;
        self.ui
            .detach(context)
            .map_err(|error| format!("UI: {error}"))
    }

    fn prepare_release(&mut self, context: &Context) -> Result<()> {
        self.quest
            .prepare_release(context)
            .map_err(|error| format!("quest: {error}"))?;
        self.geometry
            .prepare_release(context)
            .map_err(|error| format!("geometry: {error}"))
    }
}
