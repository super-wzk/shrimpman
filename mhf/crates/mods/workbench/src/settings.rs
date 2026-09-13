use crate::preview::{DEFAULT_BACKGROUND_COLOR, PreviewOptions, lighting::LightingPreset};
use mhf_config::Config;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub data_root: Option<PathBuf>,
    pub export_root: Option<PathBuf>,
    pub view: ViewSettings,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct ViewSettings {
    #[serde(deserialize_with = "deserialize_rgb")]
    pub background_color: [u8; 3],
    pub lighting_preset: LightingPreset,
    pub compact: bool,
    pub show_resources: bool,
    pub show_encoding_layers: bool,
    pub show_inspector: bool,
    pub show_log: bool,
    pub preview_only: bool,
    pub show_bones: bool,
    pub show_grid: bool,
    pub show_axes: bool,
}

fn deserialize_rgb<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<[u8; 3], D::Error> {
    // The TOML tuple decoder accepts excess elements; check the complete array.
    let values = Vec::<u8>::deserialize(deserializer)?;
    values.try_into().map_err(|values: Vec<u8>| {
        serde::de::Error::invalid_length(values.len(), &"exactly three RGB components")
    })
}

impl Default for ViewSettings {
    fn default() -> Self {
        Self {
            background_color: DEFAULT_BACKGROUND_COLOR,
            lighting_preset: LightingPreset::default(),
            compact: true,
            show_resources: true,
            show_encoding_layers: false,
            show_inspector: true,
            show_log: true,
            preview_only: false,
            show_bones: false,
            show_grid: true,
            show_axes: true,
        }
    }
}

impl ViewSettings {
    pub fn preview_options(&self) -> PreviewOptions {
        PreviewOptions {
            background_color: self.background_color,
            lighting_preset: self.lighting_preset,
            show_grid: self.show_grid,
            show_axes: self.show_axes,
        }
    }

    pub fn save(&self, configuration: Config<'_>) -> Result<(), String> {
        let values = toml::to_string(self).map_err(|error| error.to_string())?;
        let patch = format!("[\"mhf.workbench\".settings.view]\n{values}");
        configuration
            .write("mods", &patch)
            .map_err(|error| format!("无法保存工作台视图设置：{error}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mhf_config::{ConfigService, Store};
    use std::{
        fs,
        sync::{Arc, Mutex},
    };

    #[test]
    fn view_changes_persist_in_the_active_file_and_preserve_other_settings() {
        let directory =
            std::env::temp_dir().join(format!("mhf-workbench-view-{}", std::process::id()));
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("custom.toml");
        let original = "[mods.'mhf.workbench']\nenabled = true\n\
            [mods.'mhf.workbench'.settings]\ndata_root = 'custom-dat'\n\
            [mods.'mhf.debug']\nenabled = false\n";
        fs::write(&path, original).unwrap();
        let service = ConfigService::new(Arc::new(Mutex::new(Store::load(path.clone()).unwrap())));
        let config = unsafe { mhf_config::bind(service.api()) };
        // A change made after startup must survive the view patch.
        fs::write(
            &path,
            format!("{original}\n[sign]\nendpoint = 'localhost:53310'\n"),
        )
        .unwrap();
        let view = ViewSettings {
            background_color: [230, 210, 190],
            lighting_preset: LightingPreset::Warm,
            compact: false,
            show_resources: false,
            show_grid: false,
            show_axes: false,
            ..ViewSettings::default()
        };
        view.save(config).unwrap();
        let reloaded = Store::load(path.clone()).unwrap();
        let document = reloaded.document();
        let mods = &document["mods"];
        assert_eq!(mods["mhf.workbench"]["enabled"].as_bool(), Some(true));
        assert_eq!(mods["mhf.debug"]["enabled"].as_bool(), Some(false));
        assert_eq!(
            document["sign"]["endpoint"].as_str(),
            Some("localhost:53310")
        );
        let settings: Settings = mods["mhf.workbench"]["settings"]
            .clone()
            .try_into()
            .unwrap();
        assert_eq!(settings.data_root, Some("custom-dat".into()));
        assert_eq!(settings.view, view);

        let invalid = "[broken";
        fs::write(&path, invalid).unwrap();
        assert!(ViewSettings::default().save(config).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), invalid);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn view_defaults_and_rgb_validation_apply_when_loading() {
        assert_eq!(
            toml::from_str::<Settings>("").unwrap().view,
            ViewSettings::default()
        );
        for color in ["[-1, 0, 0]", "[256, 0, 0]", "[1, 2]", "[1, 2, 3, 4]"] {
            assert!(
                toml::from_str::<Settings>(&format!("[view]\nbackground_color = {color}")).is_err(),
                "accepted invalid RGB: {color}"
            );
        }
    }
}
