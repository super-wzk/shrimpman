use crate::{config, sign};
use mhf_base::{MhfFontConfig, MhfScreenConfig};
use mhf_config::Config;
use mhf_mod_api::game::LaunchParams32;
use serde::de::DeserializeOwned;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsCategory {
    Font,
    Screen,
    Server,
}

pub(crate) enum Settings {
    Font(MhfFontConfig),
    Screen(MhfScreenConfig),
    Server { endpoint: String, overridden: bool },
}

impl Settings {
    pub(crate) fn load(
        configuration: Config<'_>,
        category: SettingsCategory,
    ) -> Result<Self, String> {
        match category {
            SettingsCategory::Font => read(configuration, "font").map(Self::Font),
            SettingsCategory::Screen => read(configuration, "screen").map(Self::Screen),
            SettingsCategory::Server => {
                let values: toml::Table = read(configuration, "sign")?;
                let endpoint_override = config::endpoint_override();
                Ok(Self::Server {
                    overridden: endpoint_override.is_some(),
                    endpoint: endpoint_override.unwrap_or_else(|| {
                        values
                            .get("endpoint")
                            .and_then(toml::Value::as_str)
                            .unwrap_or_default()
                            .to_owned()
                    }),
                })
            }
        }
    }

    pub(crate) fn save(&self, configuration: Config<'_>) -> Result<(), String> {
        let (section, patch) = match self {
            Self::Font(font) => {
                validate_font(font)?;
                (
                    "font",
                    toml::to_string(font).map_err(|error| error.to_string())?,
                )
            }
            Self::Screen(screen) => {
                validate_screen(screen)?;
                (
                    "screen",
                    toml::to_string(screen).map_err(|error| error.to_string())?,
                )
            }
            Self::Server {
                endpoint,
                overridden,
            } => {
                if *overridden || config::endpoint_override().is_some() {
                    return Err(
                        "登录服务器地址由 MHF_SIGN__ENDPOINT 环境变量指定，请修改该环境变量。"
                            .into(),
                    );
                }
                let endpoint = endpoint.trim();
                let mut values: toml::Table = read(configuration, "sign")?;
                values.insert("endpoint".into(), endpoint.into());
                let source = toml::to_string(&values).map_err(|error| error.to_string())?;
                let settings = config::load(&source)?;
                sign::Client::new(&settings.endpoint, settings.encoding)
                    .map_err(|error| error.to_string())?;
                // Only the address is editable; preserve the existing encoding.
                let patch = toml::Table::from_iter([("endpoint".into(), endpoint.into())]);
                (
                    "sign",
                    toml::to_string(&patch).map_err(|error| error.to_string())?,
                )
            }
        };
        configuration
            .write(section, &patch)
            .map_err(|error| format!("无法保存设置：{error}"))
    }
}

/// Refresh the fields prepared before the login UI, without touching the Sign session.
pub(crate) fn apply_game_settings(
    configuration: Config<'_>,
    params: &mut LaunchParams32,
) -> Result<(), String> {
    let font: MhfFontConfig = read(configuration, "font")?;
    let screen: MhfScreenConfig = read(configuration, "screen")?;
    validate_font(&font)?;
    validate_screen(&screen)?;
    params.font_name.fill(0);
    params.font_name[..font.name.len()].copy_from_slice(font.name.as_bytes());
    params.font_weight = font.weight.into();
    params.font_quality = font.quality.into();
    params.screen_mode = screen.mode.into();
    params.window_width = screen.window_resolution.width;
    params.window_height = screen.window_resolution.height;
    params.fullscreen_width = screen.fullscreen_resolution.width;
    params.fullscreen_height = screen.fullscreen_resolution.height;
    Ok(())
}

fn read<T: DeserializeOwned>(configuration: Config<'_>, section: &str) -> Result<T, String> {
    let source = configuration
        .read(section)
        .map_err(|error| error.to_string())?;
    toml::from_str(&source).map_err(|error| format!("无法读取 [{section}] 设置：{error}"))
}

fn validate_font(font: &MhfFontConfig) -> Result<(), String> {
    let capacity = LaunchParams32::default().font_name.len();
    if font.name.trim().is_empty() || font.name.contains('\0') {
        return Err("请输入有效的字体名称。".into());
    }
    if font.name.len() >= capacity {
        return Err(format!("字体名称过长，最多支持 {} 字节。", capacity - 1));
    }
    if font.weight > 1000 {
        return Err("字体字重必须在 0–1000 之间。".into());
    }
    Ok(())
}

fn validate_screen(screen: &MhfScreenConfig) -> Result<(), String> {
    for resolution in [screen.window_resolution, screen.fullscreen_resolution] {
        if resolution.width == 0 || resolution.height == 0 {
            return Err("分辨率的宽度和高度必须大于 0。".into());
        }
    }
    Ok(())
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
    fn saving_the_server_address_preserves_encoding_and_other_settings() {
        let path =
            std::env::temp_dir().join(format!("mhf-login-settings-{}.toml", std::process::id()));
        fs::write(&path, "[sign]\nendpoint = 'tcp://old.example:53000'\nencoding = 'shift_jis'\n[screen]\nmode = 'windowed'\n").unwrap();
        let service = ConfigService::new(Arc::new(Mutex::new(Store::load(path.clone()).unwrap())));
        // The service owns the table throughout this borrow.
        let configuration = unsafe { mhf_config::bind(service.api()) };
        Settings::Server {
            endpoint: " tcp://new.example:53312 ".into(),
            overridden: false,
        }
        .save(configuration)
        .unwrap();
        let saved = fs::read_to_string(&path).unwrap();
        let document: toml::Table = toml::from_str(&saved).unwrap();
        assert_eq!(
            document["sign"]["endpoint"].as_str(),
            Some("tcp://new.example:53312")
        );
        assert_eq!(document["sign"]["encoding"].as_str(), Some("shift_jis"));
        assert_eq!(document["screen"]["mode"].as_str(), Some("windowed"));
        assert!(
            Settings::Server {
                endpoint: "tcp://missing-port".into(),
                overridden: false
            }
            .save(configuration)
            .is_err()
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), saved);
        fs::remove_file(path).unwrap();
    }
}
