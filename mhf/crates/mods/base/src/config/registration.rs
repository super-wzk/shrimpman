use super::MhfConfig;
use mhf_config::{Config, IniField, IniKind, IniSection, Registration};
use toml::{Table, Value};

const U32: IniKind = IniKind::Integer {
    min: 0,
    max: u32::MAX as i64,
};
const U16: IniKind = IniKind::Integer {
    min: 0,
    max: u16::MAX as i64,
};

/// Register Base's sections, then validate the values the service exposes to it.
pub fn register_config(config: Config<'_>) -> Result<MhfConfig, String> {
    let mut values = Table::new();
    for (section, definition) in registrations()? {
        config
            .register(section, &definition)
            .map_err(|error| error.to_string())?;
        let source = config.read(section).map_err(|error| error.to_string())?;
        let table = toml::from_str(&source)
            .map_err(|error| format!("invalid Base [{section}]: {error}"))?;
        values.insert(section.into(), Value::Table(table));
    }
    values
        .try_into()
        .map_err(|error| format!("invalid Base configuration: {error}"))
}

fn registrations() -> Result<Vec<(&'static str, Registration)>, String> {
    let mut defaults = Table::try_from(MhfConfig::default()).map_err(|error| error.to_string())?;
    let sections = [
        (
            "set",
            "SET",
            vec![
                field("PRESET_LEVEL", &["preset_level"], U32),
                field("CUSTOM", &["custom"], IniKind::Boolean),
            ],
        ),
        (
            "screen",
            "SCREEN",
            vec![
                field(
                    "FULLSCREEN_MODE",
                    &["mode"],
                    enumeration(&[("windowed", 0), ("fullscreen", 1)]),
                ),
                field("WINDOW_RESOLUTION_W", &["window_resolution", "width"], U32),
                field("WINDOW_RESOLUTION_H", &["window_resolution", "height"], U32),
                field(
                    "FULLSCREEN_RESOLUTION_W",
                    &["fullscreen_resolution", "width"],
                    U32,
                ),
                field(
                    "FULLSCREEN_RESOLUTION_H",
                    &["fullscreen_resolution", "height"],
                    U32,
                ),
            ],
        ),
        (
            "video",
            "VIDEO",
            vec![
                field("DISP_MAX_CHAR", &["display_character_limit"], U32),
                field("TEXTURE_DXT_USE", &["use_dxt_textures"], IniKind::Boolean),
                field("NOW_MONITOR_WH", &["now_monitor_wh"], IniKind::Boolean),
                field(
                    "GRAPHICS_VER",
                    &["graphics_version"],
                    enumeration(&[("standard", 0), ("high_definition", 1)]),
                ),
            ],
        ),
        (
            "sound",
            "SOUND",
            vec![
                field("SOUND_NOTUSE", &["disabled"], IniKind::Boolean),
                field("SOUND_VOLUME", &["volume"], U32),
                field("SOUND_VOLUME_INACTIVITY", &["inactive_volume"], U32),
                field("SOUND_VOLUME_MINIMIZE", &["minimized_volume"], U32),
                field("SOUND_FREQUENCY", &["sample_rate"], U32),
                field("SOUND_BUFFERNUM", &["buffer_size"], U32),
            ],
        ),
        (
            "localization",
            "LOCALIZATION",
            vec![field(
                "LANGUAGE",
                &["language"],
                enumeration(&[
                    ("japanese", 0),
                    ("english", 1),
                    ("korean", 6),
                    ("traditional_chinese", 7),
                ]),
            )],
        ),
        (
            "font",
            "FONT",
            vec![
                field(
                    "QUALITY",
                    &["quality"],
                    enumeration(&[
                        ("default", 0),
                        ("draft", 1),
                        ("proof", 2),
                        ("non_antialiased", 3),
                        ("antialiased", 4),
                        ("clear_type", 5),
                        ("clear_type_natural", 6),
                    ]),
                ),
                field("WEIGHT", &["weight"], U16),
                field("NAME", &["name"], IniKind::String),
            ],
        ),
        (
            "option",
            "OPTION",
            vec![
                field("DRAWSKIP", &["draw_skip"], IniKind::Boolean),
                field("CLOGDIS", &["clog_disabled"], IniKind::Boolean),
            ],
        ),
        (
            "launch",
            "LAUNCH",
            vec![
                field("PROXY_USE", &["use_proxy"], IniKind::Boolean),
                field("PROXY_IE", &["use_ie_proxy"], IniKind::Boolean),
                field("PROXY_SET", &["proxy_configured"], IniKind::Boolean),
                field("PROXY_ADDR", &["proxy_address"], IniKind::String),
                field("PROXY_PORT", &["proxy_port"], U16),
                field("SERVER_SEL", &["server_selection"], U32),
            ],
        ),
    ];
    sections
        .into_iter()
        .map(|(section, name, fields)| {
            let Some(Value::Table(defaults)) = defaults.remove(section) else {
                return Err(format!("Base defaults for [{section}] must be a table"));
            };
            let fixed = if section == "video" {
                Table::from_iter([(
                    "graphics_version".into(),
                    Value::String("high_definition".into()),
                )])
            } else {
                Table::new()
            };
            Ok((
                section,
                Registration {
                    defaults,
                    fixed,
                    ini: Some(IniSection {
                        name: name.into(),
                        fields,
                    }),
                },
            ))
        })
        .collect()
}

fn field(key: &str, path: &[&str], kind: IniKind) -> IniField {
    IniField {
        key: key.into(),
        path: path.iter().map(|part| (*part).into()).collect(),
        kind,
    }
}

fn enumeration(values: &[(&str, i64)]) -> IniKind {
    IniKind::Enum {
        values: values
            .iter()
            .map(|(name, value)| ((*name).into(), *value))
            .collect(),
    }
}
