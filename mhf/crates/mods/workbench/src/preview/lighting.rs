use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum LightingPreset {
    #[default]
    Standard,
    Soft,
    Dark,
    Warm,
    Cool,
    Ambient,
}

#[derive(Clone, Copy)]
pub(crate) struct DirectionalLight {
    pub color: [f32; 3],
    pub direction: [f32; 3],
}

pub(crate) struct Lighting {
    pub ambient: [u8; 3],
    pub lights: [DirectionalLight; 3],
}

impl LightingPreset {
    pub const ALL: [Self; 6] = [
        Self::Standard,
        Self::Soft,
        Self::Dark,
        Self::Warm,
        Self::Cool,
        Self::Ambient,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Standard => "标准",
            Self::Soft => "柔和",
            Self::Dark => "暗光",
            Self::Warm => "暖光",
            Self::Cool => "冷光",
            Self::Ambient => "纯环境光",
        }
    }

    pub fn description(self) -> &'static str {
        match self {
            Self::Standard => "原有的中性环境光与单侧主光",
            Self::Soft => "降低主光强度，增加侧面补光",
            Self::Dark => "低亮度中性光，便于观察发光部分",
            Self::Warm => "偏暖主光与少量冷色补光",
            Self::Cool => "偏冷主光与少量暖色补光",
            Self::Ambient => "关闭方向光，仅保留均匀环境光",
        }
    }

    pub fn lighting(self) -> Lighting {
        let (ambient, key, fill) = match self {
            // Keep the existing preview lighting as the default.
            Self::Standard => ([128; 3], [0.8; 3], [0.0; 3]),
            Self::Soft => ([112; 3], [0.35; 3], [0.15; 3]),
            Self::Dark => ([24; 3], [0.2; 3], [0.0; 3]),
            Self::Warm => ([96, 83, 72], [0.65, 0.48, 0.32], [0.06, 0.10, 0.16]),
            Self::Cool => ([64, 80, 104], [0.36, 0.52, 0.72], [0.12, 0.10, 0.08]),
            Self::Ambient => ([200; 3], [0.0; 3], [0.0; 3]),
        };
        Lighting {
            ambient,
            // Always specify all three native light slots so switching away
            // from a preset with fill light also clears that light.
            lights: [
                DirectionalLight {
                    color: key,
                    direction: [-0.4, -0.7, -0.6],
                },
                DirectionalLight {
                    color: fill,
                    direction: [0.6, -0.2, 0.7745967],
                },
                DirectionalLight {
                    color: [0.0; 3],
                    direction: [-0.4, -0.7, -0.6],
                },
            ],
        }
    }
}
