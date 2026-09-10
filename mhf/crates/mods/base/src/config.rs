//! Game settings owned and validated by Base.

mod registration;
pub use registration::register_config;

#[cfg(test)]
mod tests;

use serde::{Deserialize, Serialize};
use std::net::Ipv4Addr;

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfConfig {
    pub set: MhfSetConfig,
    pub screen: MhfScreenConfig,
    pub video: MhfVideoConfig,
    pub sound: MhfSoundConfig,
    pub localization: MhfLocalizationConfig,
    pub font: MhfFontConfig,
    pub option: MhfOptionConfig,
    pub launch: MhfLaunchConfig,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfSetConfig {
    pub preset_level: u32,
    pub custom: bool,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfScreenConfig {
    pub mode: ScreenMode,
    pub window_resolution: Resolution,
    pub fullscreen_resolution: Resolution,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum ScreenMode {
    Windowed = 0,
    #[default]
    Fullscreen = 1,
}

impl TryFrom<u32> for ScreenMode {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Windowed),
            1 => Ok(Self::Fullscreen),
            _ => Err(format!("unsupported screen mode {value}")),
        }
    }
}

impl From<ScreenMode> for u32 {
    fn from(value: ScreenMode) -> Self {
        value as Self
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfVideoConfig {
    pub display_character_limit: u32,
    pub use_dxt_textures: bool,
    pub now_monitor_wh: bool,
    pub graphics_version: GraphicsVersion,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum GraphicsVersion {
    Standard = 0,
    #[default]
    HighDefinition = 1,
}

impl TryFrom<u32> for GraphicsVersion {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Standard),
            1 => Ok(Self::HighDefinition),
            _ => Err(format!("unsupported graphics version {value}")),
        }
    }
}

impl From<GraphicsVersion> for u32 {
    fn from(value: GraphicsVersion) -> Self {
        value as Self
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfSoundConfig {
    pub disabled: bool,
    pub volume: u32,
    pub inactive_volume: u32,
    pub minimized_volume: u32,
    pub sample_rate: u32,
    pub buffer_size: u32,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfLocalizationConfig {
    pub language: Language,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum Language {
    #[default]
    Japanese = 0,
    English = 1,
    Korean = 6,
    TraditionalChinese = 7,
}

impl TryFrom<u32> for Language {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Japanese),
            1 => Ok(Self::English),
            6 => Ok(Self::Korean),
            7 => Ok(Self::TraditionalChinese),
            _ => Err(format!("unsupported language code {value}")),
        }
    }
}

impl From<Language> for u32 {
    fn from(value: Language) -> Self {
        value as Self
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfFontConfig {
    pub quality: FontQuality,
    pub weight: u16,
    pub name: String,
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[repr(u32)]
pub enum FontQuality {
    Default = 0,
    Draft = 1,
    Proof = 2,
    NonAntialiased = 3,
    #[default]
    Antialiased = 4,
    ClearType = 5,
    ClearTypeNatural = 6,
}

impl TryFrom<u32> for FontQuality {
    type Error = String;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Default),
            1 => Ok(Self::Draft),
            2 => Ok(Self::Proof),
            3 => Ok(Self::NonAntialiased),
            4 => Ok(Self::Antialiased),
            5 => Ok(Self::ClearType),
            6 => Ok(Self::ClearTypeNatural),
            _ => Err(format!("unsupported font quality {value}")),
        }
    }
}

impl From<FontQuality> for u32 {
    fn from(value: FontQuality) -> Self {
        value as Self
    }
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfOptionConfig {
    pub draw_skip: bool,
    pub clog_disabled: bool,
}

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default)]
pub struct MhfLaunchConfig {
    pub use_proxy: bool,
    pub use_ie_proxy: bool,
    pub proxy_configured: bool,
    pub proxy_address: Ipv4Addr,
    pub proxy_port: u16,
    pub server_selection: u32,
}

impl Default for MhfSetConfig {
    fn default() -> Self {
        Self {
            preset_level: 0,
            custom: true,
        }
    }
}

impl Default for Resolution {
    fn default() -> Self {
        Self {
            width: 1920,
            height: 1080,
        }
    }
}

impl Default for MhfVideoConfig {
    fn default() -> Self {
        Self {
            display_character_limit: 100,
            use_dxt_textures: false,
            now_monitor_wh: false,
            graphics_version: GraphicsVersion::default(),
        }
    }
}

impl Default for MhfSoundConfig {
    fn default() -> Self {
        Self {
            disabled: false,
            volume: 0,
            inactive_volume: 0,
            minimized_volume: 0,
            sample_rate: 48_000,
            buffer_size: 2048,
        }
    }
}

impl Default for MhfFontConfig {
    fn default() -> Self {
        Self {
            quality: FontQuality::default(),
            weight: 400,
            name: mhf_font::FAMILY_NAME.to_owned(),
        }
    }
}

impl Default for MhfOptionConfig {
    fn default() -> Self {
        Self {
            draw_skip: true,
            clog_disabled: false,
        }
    }
}

impl Default for MhfLaunchConfig {
    fn default() -> Self {
        Self {
            use_proxy: false,
            use_ie_proxy: false,
            proxy_configured: true,
            proxy_address: Ipv4Addr::LOCALHOST,
            proxy_port: 8888,
            server_selection: 1,
        }
    }
}
