use serde::{Deserialize, Serialize, de::DeserializeOwned};
use shrimpman_mhf_launcher::{FontQuality, GraphicsVersion, Language, MhfConfig, ScreenMode};
use std::{
    fs,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

const SIGN_SECTION: &str = "sign";

const INI_SECTIONS: &[(&str, &str)] = &[
    ("SET", "set"),
    ("SCREEN", "screen"),
    ("VIDEO", "video"),
    ("SOUND", "sound"),
    ("LOCALIZATION", "localization"),
    ("FONT", "font"),
    ("OPTION", "option"),
    ("LAUNCH", "launch"),
];

#[derive(Clone, Copy)]
enum TomlPath {
    Direct(&'static str),
    Nested(&'static str, &'static str),
}

impl TomlPath {
    const fn root(self) -> &'static str {
        match self {
            Self::Direct(key) | Self::Nested(key, _) => key,
        }
    }

    fn get(self, table: &Table) -> Option<&Value> {
        match self {
            Self::Direct(key) => table.get(key),
            Self::Nested(group, key) => table.get(group)?.as_table()?.get(key),
        }
    }

    fn insert(self, table: &mut Table, value: Value) {
        match self {
            Self::Direct(key) => {
                table.insert(key.to_owned(), value);
            }
            Self::Nested(group, key) => {
                let nested = table
                    .entry(group.to_owned())
                    .or_insert_with(|| Value::Table(Table::new()))
                    .as_table_mut()
                    .expect("validated domain config group must be a table");
                nested.insert(key.to_owned(), value);
            }
        }
    }

    fn remove(self, table: &mut Table) {
        match self {
            Self::Direct(key) => {
                table.remove(key);
            }
            Self::Nested(group, key) => {
                let Some(nested) = table.get_mut(group).and_then(Value::as_table_mut) else {
                    return;
                };
                nested.remove(key);
                if nested.is_empty() {
                    table.remove(group);
                }
            }
        }
    }
}

#[derive(Clone, Copy)]
enum IniValueKind {
    Boolean,
    Integer,
    String,
    ScreenMode,
    GraphicsVersion,
    Language,
    FontQuality,
}

impl IniValueKind {
    fn to_ini(self, value: &Value) -> Option<String> {
        match self {
            Self::Boolean => value
                .as_bool()
                .map(|value| if value { "1" } else { "0" }.to_owned()),
            Self::Integer => value.as_integer().map(|value| value.to_string()),
            Self::String => value.as_str().map(ToOwned::to_owned),
            Self::ScreenMode => enum_to_ini::<ScreenMode>(value),
            Self::GraphicsVersion => enum_to_ini::<GraphicsVersion>(value),
            Self::Language => enum_to_ini::<Language>(value),
            Self::FontQuality => enum_to_ini::<FontQuality>(value),
        }
    }

    fn parse_ini(self, value: &str) -> Result<Value, String> {
        match self {
            Self::Boolean => match value.trim() {
                "0" => Ok(Value::Boolean(false)),
                "1" => Ok(Value::Boolean(true)),
                value => Err(format!("expected 0 or 1, got {value:?}")),
            },
            Self::Integer => parse_ini_integer(value).map(Value::Integer),
            Self::String => Ok(Value::String(value.to_owned())),
            Self::ScreenMode => enum_from_ini::<ScreenMode>(value),
            Self::GraphicsVersion => enum_from_ini::<GraphicsVersion>(value),
            Self::Language => enum_from_ini::<Language>(value),
            Self::FontQuality => enum_from_ini::<FontQuality>(value),
        }
    }
}

fn enum_to_ini<T>(value: &Value) -> Option<String>
where
    T: DeserializeOwned + Into<u32>,
{
    let value: T = value.clone().try_into().ok()?;
    Some(value.into().to_string())
}

fn enum_from_ini<T>(value: &str) -> Result<Value, String>
where
    T: Serialize + TryFrom<u32, Error = String>,
{
    let value = T::try_from(parse_ini_u32(value)?)?;
    Value::try_from(value).map_err(|error| error.to_string())
}

struct IniField {
    ini_section: &'static str,
    ini_key: &'static str,
    toml_path: TomlPath,
    kind: IniValueKind,
}

impl IniField {
    const fn direct(
        ini_section: &'static str,
        ini_key: &'static str,
        toml_key: &'static str,
        kind: IniValueKind,
    ) -> Self {
        Self {
            ini_section,
            ini_key,
            toml_path: TomlPath::Direct(toml_key),
            kind,
        }
    }

    const fn nested(
        ini_section: &'static str,
        ini_key: &'static str,
        toml_group: &'static str,
        toml_key: &'static str,
        kind: IniValueKind,
    ) -> Self {
        Self {
            ini_section,
            ini_key,
            toml_path: TomlPath::Nested(toml_group, toml_key),
            kind,
        }
    }
}

const INI_FIELDS: &[IniField] = &[
    IniField::direct("SET", "PRESET_LEVEL", "preset_level", IniValueKind::Integer),
    IniField::direct("SET", "CUSTOM", "custom", IniValueKind::Boolean),
    IniField::direct(
        "SCREEN",
        "FULLSCREEN_MODE",
        "mode",
        IniValueKind::ScreenMode,
    ),
    IniField::nested(
        "SCREEN",
        "WINDOW_RESOLUTION_W",
        "window_resolution",
        "width",
        IniValueKind::Integer,
    ),
    IniField::nested(
        "SCREEN",
        "WINDOW_RESOLUTION_H",
        "window_resolution",
        "height",
        IniValueKind::Integer,
    ),
    IniField::nested(
        "SCREEN",
        "FULLSCREEN_RESOLUTION_W",
        "fullscreen_resolution",
        "width",
        IniValueKind::Integer,
    ),
    IniField::nested(
        "SCREEN",
        "FULLSCREEN_RESOLUTION_H",
        "fullscreen_resolution",
        "height",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "VIDEO",
        "DISP_MAX_CHAR",
        "display_character_limit",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "VIDEO",
        "TEXTURE_DXT_USE",
        "use_dxt_textures",
        IniValueKind::Boolean,
    ),
    IniField::direct(
        "VIDEO",
        "NOW_MONITOR_WH",
        "now_monitor_wh",
        IniValueKind::Boolean,
    ),
    IniField::direct(
        "VIDEO",
        "GRAPHICS_VER",
        "graphics_version",
        IniValueKind::GraphicsVersion,
    ),
    IniField::direct("SOUND", "SOUND_NOTUSE", "disabled", IniValueKind::Boolean),
    IniField::direct("SOUND", "SOUND_VOLUME", "volume", IniValueKind::Integer),
    IniField::direct(
        "SOUND",
        "SOUND_VOLUME_INACTIVITY",
        "inactive_volume",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "SOUND",
        "SOUND_VOLUME_MINIMIZE",
        "minimized_volume",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "SOUND",
        "SOUND_FREQUENCY",
        "sample_rate",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "SOUND",
        "SOUND_BUFFERNUM",
        "buffer_size",
        IniValueKind::Integer,
    ),
    IniField::direct(
        "LOCALIZATION",
        "LANGUAGE",
        "language",
        IniValueKind::Language,
    ),
    IniField::direct("FONT", "QUALITY", "quality", IniValueKind::FontQuality),
    IniField::direct("FONT", "WEIGHT", "weight", IniValueKind::Integer),
    IniField::direct("FONT", "NAME", "name", IniValueKind::String),
    IniField::direct("OPTION", "DRAWSKIP", "draw_skip", IniValueKind::Boolean),
    IniField::direct("OPTION", "CLOGDIS", "clog_disabled", IniValueKind::Boolean),
    IniField::direct("LAUNCH", "PROXY_USE", "use_proxy", IniValueKind::Boolean),
    IniField::direct("LAUNCH", "PROXY_IE", "use_ie_proxy", IniValueKind::Boolean),
    IniField::direct(
        "LAUNCH",
        "PROXY_SET",
        "proxy_configured",
        IniValueKind::Boolean,
    ),
    IniField::direct(
        "LAUNCH",
        "PROXY_ADDR",
        "proxy_address",
        IniValueKind::String,
    ),
    IniField::direct("LAUNCH", "PROXY_PORT", "proxy_port", IniValueKind::Integer),
    IniField::direct(
        "LAUNCH",
        "SERVER_SEL",
        "server_selection",
        IniValueKind::Integer,
    ),
];

fn ini_field(section: &str, key: &str) -> Option<&'static IniField> {
    section_fields(section).find(|field| field.ini_key.eq_ignore_ascii_case(key))
}

fn section_fields(section: &str) -> impl Iterator<Item = &'static IniField> + '_ {
    INI_FIELDS
        .iter()
        .filter(move |field| field.ini_section.eq_ignore_ascii_case(section))
}

fn toml_section_name(ini_section: &str) -> Option<&'static str> {
    INI_SECTIONS
        .iter()
        .find(|(ini, _)| ini.eq_ignore_ascii_case(ini_section))
        .map(|(_, toml)| *toml)
}

fn ini_section_name(toml_section: &str) -> Option<&'static str> {
    INI_SECTIONS
        .iter()
        .find(|(_, toml)| toml.eq_ignore_ascii_case(toml_section))
        .map(|(ini, _)| *ini)
}

fn parse_ini_integer(value: &str) -> Result<i64, String> {
    let value = value.trim();
    let parsed = match value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
    {
        Some(hex) => i64::from_str_radix(hex, 16),
        None => value.parse(),
    };
    parsed.map_err(|error| format!("invalid integer {value:?}: {error}"))
}

fn parse_ini_u32(value: &str) -> Result<u32, String> {
    let value = parse_ini_integer(value)?;
    u32::try_from(value).map_err(|_| format!("integer {value} is outside the u32 range"))
}

pub(crate) struct Store {
    path: PathBuf,
    document: Table,
}

#[derive(Debug, Deserialize)]
pub(crate) struct Settings {
    pub(crate) sign: sign::Settings,
    #[serde(flatten)]
    pub(crate) mhf: MhfConfig,
}

pub(crate) mod sign {
    use serde::Deserialize;

    #[derive(Debug, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(crate) struct Settings {
        pub(crate) http: http::Settings,
    }

    pub(crate) mod http {
        use serde::Deserialize;

        #[derive(Debug, Deserialize)]
        #[serde(deny_unknown_fields)]
        pub(crate) struct Settings {
            pub(crate) base_url: String,
        }
    }
}

pub(crate) fn load(path: PathBuf) -> Result<(Settings, Store), String> {
    let source = fs::read_to_string(&path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let document: Table = toml::from_str(&source)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    let config = decode(&document)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    Ok((config, Store { path, document }))
}

impl Store {
    pub(crate) fn value(&self, section: &str, key: &str) -> Option<String> {
        let values = profile_section(&self.document, section)?;
        if let Some(field) = ini_field(section, key) {
            return field
                .toml_path
                .get(values)
                .and_then(|value| field.kind.to_ini(value));
        }
        values
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .and_then(|(_, value)| value.as_str())
            .map(ToOwned::to_owned)
    }

    pub(crate) fn section_names(&self) -> Vec<String> {
        self.document
            .iter()
            .filter(|(name, value)| !name.eq_ignore_ascii_case(SIGN_SECTION) && value.is_table())
            .map(|(name, _)| ini_section_name(name).unwrap_or(name).to_owned())
            .collect()
    }

    pub(crate) fn key_names(&self, section: &str) -> Vec<String> {
        let Some(values) = profile_section(&self.document, section) else {
            return Vec::new();
        };
        let mut names: Vec<String> = section_fields(section)
            .filter(|field| field.toml_path.get(values).is_some())
            .map(|field| field.ini_key.to_owned())
            .collect();
        names.extend(
            values
                .keys()
                .filter(|key| !section_fields(section).any(|field| field.toml_path.root() == *key))
                .cloned(),
        );
        names
    }

    pub(crate) fn set_value(
        &mut self,
        section: String,
        key: String,
        value: String,
    ) -> Result<(), String> {
        if section.eq_ignore_ascii_case(SIGN_SECTION) {
            return Err("[sign] is launcher configuration, not an MHF INI section".to_owned());
        }
        let field = ini_field(&section, &key);
        let value = match field {
            Some(field) => field.kind.parse_ini(&value).map_err(|error| {
                format!(
                    "invalid INI value [{}] {}: {error}",
                    field.ini_section, field.ini_key
                )
            })?,
            None => Value::String(value),
        };
        let new_section = toml_section_name(&section).unwrap_or(&section).to_owned();
        self.update(|document| {
            let section = section_name(document, &section).unwrap_or(new_section);
            let values = document
                .entry(section)
                .or_insert_with(|| Value::Table(Table::new()))
                .as_table_mut()
                .expect("validated INI section must be a table");
            if let Some(field) = field {
                field.toml_path.insert(values, value);
            } else {
                let key = values
                    .keys()
                    .find(|name| name.eq_ignore_ascii_case(&key))
                    .cloned()
                    .unwrap_or(key);
                values.insert(key, value);
            }
        })
    }

    pub(crate) fn remove_key(&mut self, section: &str, key: &str) -> Result<(), String> {
        if section.eq_ignore_ascii_case(SIGN_SECTION) {
            return Err("[sign] is launcher configuration, not an MHF INI section".to_owned());
        }
        let field = ini_field(section, key);
        self.update(|document| {
            let Some(section) = section_name(document, section) else {
                return;
            };
            let values = document
                .get_mut(&section)
                .and_then(Value::as_table_mut)
                .expect("resolved INI section must be a table");
            if let Some(field) = field {
                field.toml_path.remove(values);
            } else if let Some(key) = values
                .keys()
                .find(|name| name.eq_ignore_ascii_case(key))
                .cloned()
            {
                values.remove(&key);
            }
        })
    }

    pub(crate) fn remove_section(&mut self, section: &str) -> Result<(), String> {
        if section.eq_ignore_ascii_case(SIGN_SECTION) {
            return Err("[sign] is launcher configuration, not an MHF INI section".to_owned());
        }
        self.update(|document| {
            if let Some(section) = section_name(document, section) {
                document.remove(&section);
            }
        })
    }

    fn update(&mut self, update: impl FnOnce(&mut Table)) -> Result<(), String> {
        let mut document = self.document.clone();
        update(&mut document);
        decode(&document)?;
        write_document(&self.path, &document)?;
        self.document = document;
        Ok(())
    }
}

fn write_document(path: &Path, document: &Table) -> Result<(), String> {
    let source = toml::to_string_pretty(document)
        .map_err(|error| format!("failed to serialize {}: {error}", path.display()))?;
    fs::write(path, source).map_err(|error| format!("failed to write {}: {error}", path.display()))
}

fn decode(document: &Table) -> Result<Settings, String> {
    for (section, value) in document {
        if section.eq_ignore_ascii_case(SIGN_SECTION) {
            if section != SIGN_SECTION {
                return Err(format!(
                    "launcher config section [{section}] must be named [{SIGN_SECTION}]"
                ));
            }
            continue;
        }
        let known_section = toml_section_name(section);
        if let Some(canonical) = known_section
            && section != canonical
        {
            return Err(format!(
                "MHF config section [{section}] must be named [{canonical}]"
            ));
        }
        let table = value
            .as_table()
            .ok_or_else(|| format!("INI section {section:?} must be a TOML table"))?;
        for (key, value) in table {
            if let Some(field) = section_fields(section).find(|field| {
                field.toml_path.root().eq_ignore_ascii_case(key)
                    || field.ini_key.eq_ignore_ascii_case(key)
            }) {
                if key != field.toml_path.root() {
                    return Err(format!(
                        "MHF config field [{section}] {key} must be named {}",
                        field.toml_path.root()
                    ));
                }
                continue;
            }
            if !value.is_str() {
                return Err(format!("INI value [{section}] {key} must be a TOML string"));
            }
        }
    }
    Value::Table(document.clone())
        .try_into()
        .map_err(|error| error.to_string())
}

fn profile_section<'a>(document: &'a Table, name: &str) -> Option<&'a Table> {
    if name.eq_ignore_ascii_case(SIGN_SECTION) {
        return None;
    }
    let name = toml_section_name(name).unwrap_or(name);
    document
        .iter()
        .find(|(section, _)| section.eq_ignore_ascii_case(name))
        .and_then(|(_, value)| value.as_table())
}

fn section_name(document: &Table, name: &str) -> Option<String> {
    let name = toml_section_name(name).unwrap_or(name);
    document
        .keys()
        .find(|section| section.eq_ignore_ascii_case(name))
        .cloned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    const SOURCE: &str = r#"
[sign.http]
base_url = "http://127.0.0.1:53313"

[screen]
mode = "windowed"
window_resolution = { width = 1280, height = 720 }

[video]
use_dxt_textures = false
graphics_version = "standard"

[localization]
language = "japanese"

[font]
name = "ＭＳ ゴシック"
weight = 0x190

[launch]
proxy_address = "10.0.0.1"

[extra]
value = "preserved"
"#;

    fn document() -> Table {
        toml::from_str(SOURCE).expect("test config should parse")
    }

    #[test]
    fn example_config_uses_the_domain_types() {
        let document = toml::from_str(include_str!("../../../../../mhf.toml"))
            .expect("example config should parse");
        let config = decode(&document).expect("example config should be valid");

        assert_eq!(config.sign.http.base_url, "http://127.0.0.1:53313");
        assert_eq!(config.mhf.screen.mode, ScreenMode::Windowed);
        assert_eq!(
            config.mhf.video.graphics_version,
            GraphicsVersion::HighDefinition
        );
        assert_eq!(config.mhf.localization.language, Language::Japanese);
        assert!(!document.contains_key("server"));
    }

    #[test]
    fn persistence_decodes_to_the_strong_domain_config() {
        let config = decode(&document()).expect("test config should be valid");

        assert_eq!(config.mhf.video.graphics_version, GraphicsVersion::Standard);
        assert_eq!(config.mhf.localization.language, Language::Japanese);
        assert_eq!(config.mhf.font.name, "ＭＳ ゴシック");
        assert_eq!(config.mhf.font.weight, 0x190);
        assert_eq!(config.mhf.launch.proxy_address, Ipv4Addr::new(10, 0, 0, 1));
        assert_eq!(config.mhf.screen.window_resolution.width, 1280);
    }

    #[test]
    fn erased_values_are_limited_to_the_persistence_boundary() {
        let mut document = document();
        assert_eq!(
            profile_section(&document, "extra").unwrap()["value"].as_str(),
            Some("preserved")
        );
        assert!(profile_section(&document, "sign").is_none());

        let video = document
            .remove("video")
            .expect("video section should exist");
        document.insert("VIDEO".to_owned(), video);
        let error = decode(&document).expect_err("legacy section names must fail");
        assert!(error.contains("[VIDEO] must be named [video]"));

        let video = document
            .remove("VIDEO")
            .expect("legacy video section should exist");
        document.insert("video".to_owned(), video);
        let video = document["video"]
            .as_table_mut()
            .expect("video section should be a table");
        let version = video
            .remove("graphics_version")
            .expect("graphics version should exist");
        video.insert("GRAPHICS_VER".to_owned(), version);
        let error = decode(&document).expect_err("legacy field names must fail");
        assert!(error.contains("GRAPHICS_VER must be named graphics_version"));
    }

    #[test]
    fn store_validates_before_replacing_the_toml_file() {
        let path =
            std::env::temp_dir().join(format!("shrimpman-mhf-config-{}.toml", std::process::id()));
        fs::write(&path, SOURCE).expect("test config should be written");
        let (_, mut store) = load(path.clone()).expect("test config should load");

        assert!(store.section_names().contains(&"SCREEN".to_owned()));
        assert!(
            store
                .key_names("SCREEN")
                .contains(&"WINDOW_RESOLUTION_W".to_owned())
        );
        assert_eq!(
            store.value("SCREEN", "WINDOW_RESOLUTION_W").as_deref(),
            Some("1280")
        );
        store
            .set_value("EXTRA".to_owned(), "VALUE".to_owned(), "updated".to_owned())
            .expect("unknown INI values should pass through");
        assert_eq!(store.value("EXTRA", "VALUE").as_deref(), Some("updated"));
        store
            .set_value(
                "VIDEO".to_owned(),
                "TEXTURE_DXT_USE".to_owned(),
                "1".to_owned(),
            )
            .expect("known INI booleans should map to TOML booleans");
        store
            .set_value(
                "VIDEO".to_owned(),
                "GRAPHICS_VER".to_owned(),
                "1".to_owned(),
            )
            .expect("known INI enums should map to their domain values");
        assert_eq!(store.value("VIDEO", "GRAPHICS_VER").as_deref(), Some("1"));
        assert!(
            store
                .set_value(
                    "VIDEO".to_owned(),
                    "GRAPHICS_VER".to_owned(),
                    "invalid".to_owned(),
                )
                .is_err()
        );
        assert_eq!(store.value("VIDEO", "GRAPHICS_VER").as_deref(), Some("1"));
        assert_eq!(
            store.value("LOCALIZATION", "LANGUAGE").as_deref(),
            Some("0")
        );
        store
            .set_value(
                "LOCALIZATION".to_owned(),
                "LANGUAGE".to_owned(),
                "6".to_owned(),
            )
            .expect("known INI languages should map to their domain values");
        assert_eq!(
            store.value("LOCALIZATION", "LANGUAGE").as_deref(),
            Some("6")
        );
        assert!(
            store
                .set_value(
                    "LOCALIZATION".to_owned(),
                    "LANGUAGE".to_owned(),
                    "2".to_owned(),
                )
                .is_err()
        );
        assert_eq!(
            store.value("LOCALIZATION", "LANGUAGE").as_deref(),
            Some("6")
        );

        let source = fs::read_to_string(&path).expect("updated config should be readable");
        let document: Table = toml::from_str(&source).expect("updated config should remain TOML");
        assert!(!source.contains("[server]"));
        assert!(source.contains("[sign.http]"));
        assert!(source.contains("value = \"updated\""));
        assert!(source.contains("graphics_version = \"high_definition\""));
        assert!(source.contains("language = \"korean\""));
        assert_eq!(document["video"]["use_dxt_textures"].as_bool(), Some(true));
        fs::remove_file(path).expect("test config should be removed");
    }
}
