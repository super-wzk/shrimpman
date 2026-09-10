use crate::{Selection, VersionReq};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

/// The `[mods]` section of the shared mhf.toml configuration.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default)]
pub struct RuntimeConfig {
    pub directory: PathBuf,
    #[serde(flatten)]
    pub modules: BTreeMap<String, ModSettings>,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            directory: PathBuf::from("mods"),
            modules: BTreeMap::new(),
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct ModSettings {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<VersionReq>,
    #[serde(skip_serializing_if = "toml::Table::is_empty")]
    pub settings: toml::Table,
}

impl RuntimeConfig {
    pub fn selections(&self) -> BTreeMap<String, Selection> {
        self.modules
            .iter()
            .map(|(id, settings)| {
                (
                    id.clone(),
                    Selection {
                        enabled: settings.enabled,
                        version: settings.version.clone(),
                    },
                )
            })
            .collect()
    }
}
