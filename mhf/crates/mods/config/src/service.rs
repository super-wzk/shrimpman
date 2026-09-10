use crate::{IniField, IniKind, Registration};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};
use toml::{Table, Value};

type Result<T> = std::result::Result<T, String>;

/// One TOML document and the definitions registered by its consumers.
/// Defaults are read in memory; writes merge into the latest file on disk.
pub struct Store {
    path: PathBuf,
    document: Table,
    registrations: BTreeMap<String, Registration>,
}

impl Store {
    pub fn load(path: PathBuf) -> Result<Self> {
        Ok(Self {
            document: read_document(&path)?,
            path,
            registrations: BTreeMap::new(),
        })
    }

    pub fn document(&self) -> &Table {
        &self.document
    }

    pub fn register(&mut self, section: &str, registration: Registration) -> Result<()> {
        valid_name(section, "configuration section")?;
        if let Some(previous) = self.registrations.get(section) {
            return if previous == &registration {
                Ok(())
            } else {
                Err(format!(
                    "configuration section [{section}] has a different registration"
                ))
            };
        }
        validate_registration(&registration)?;
        let aliases = |name: &str, definition: &Registration| {
            let mut names = vec![name.to_owned()];
            if let Some(ini) = &definition.ini {
                names.push(ini.name.clone());
            }
            names
        };
        let names = aliases(section, &registration);
        for (other, definition) in &self.registrations {
            if names.iter().any(|name| {
                aliases(other, definition)
                    .iter()
                    .any(|other| name.eq_ignore_ascii_case(other))
            }) {
                return Err(format!(
                    "configuration section [{section}] conflicts with [{other}]"
                ));
            }
        }
        validate_section_aliases(&self.document, section, &registration)?;
        effective_section(&self.document, section, Some(&registration))?;
        self.registrations.insert(section.to_owned(), registration);
        Ok(())
    }

    pub fn read(&self, section: &str) -> Result<Table> {
        effective_section(&self.document, section, self.registrations.get(section))
    }

    pub fn write(&mut self, section: &str, patch: Table) -> Result<()> {
        valid_name(section, "configuration section")?;
        self.update(|document| {
            merge(table_mut(document, section)?, &patch);
            Ok(())
        })
    }

    pub fn value(&self, section: &str, key: &str) -> Option<String> {
        match self.ini_registration(section).ok()? {
            Some((name, registration)) => {
                let values = self.read(name).ok()?;
                if let Some(field) = ini_field(registration, key) {
                    return to_ini(&field.kind, path_value(&values, &field.path)?).ok();
                }
                if reserved_key(registration, key) {
                    return None;
                }
                string_value(&values, key)
            }
            None => string_value(legacy_section(&self.document, section)?, key),
        }
    }

    pub fn section_names(&self) -> Vec<String> {
        let mut names: Vec<_> = self
            .registrations
            .iter()
            .filter_map(|(section, registration)| {
                registration
                    .ini
                    .as_ref()
                    .filter(|_| self.read(section).is_ok())
                    .map(|ini| ini.name.clone())
            })
            .collect();
        names.extend(self.document.iter().filter_map(|(name, value)| {
            if self.reserved_section(name) {
                return None;
            }
            value
                .as_table()
                .filter(|table| string_table(table))
                .map(|_| name.clone())
        }));
        names.sort();
        names
    }

    pub(crate) fn key_names(&self, section: &str) -> Vec<String> {
        match self.ini_registration(section) {
            Ok(Some((name, registration))) => {
                let Ok(values) = self.read(name) else {
                    return Vec::new();
                };
                let mut names = registration
                    .ini
                    .as_ref()
                    .into_iter()
                    .flat_map(|ini| &ini.fields)
                    .filter(|field| path_value(&values, &field.path).is_some())
                    .map(|field| field.key.clone())
                    .collect::<Vec<_>>();
                names.extend(
                    values
                        .iter()
                        .filter(|(name, value)| value.is_str() && !reserved_key(registration, name))
                        .map(|(name, _)| name.clone()),
                );
                names
            }
            Ok(None) => legacy_section(&self.document, section)
                .map(|table| table.keys().cloned().collect())
                .unwrap_or_default(),
            Err(_) => Vec::new(),
        }
    }

    pub(crate) fn set_value(&mut self, section: String, key: String, value: String) -> Result<()> {
        valid_name(&key, "INI key")?;
        let (section, registration) = self.ini_target(&section)?;
        let field = registration
            .as_ref()
            .and_then(|registration| ini_field(registration, &key))
            .cloned();
        if field.is_none()
            && registration
                .as_ref()
                .is_some_and(|registration| reserved_key(registration, &key))
        {
            return Err(format!("INI key {key} has no registered mapping"));
        }
        let value = match &field {
            Some(field) => parse_ini(&field.kind, &value)?,
            None => Value::String(value),
        };
        self.update(|document| {
            let section = if registration.is_some() {
                section
            } else {
                matching_key(document, &section).unwrap_or(section)
            };
            let values = ini_table_mut(document, &section, registration.is_some())?;
            if let Some(field) = field {
                set_path(values, &field.path, value)
            } else {
                let key = matching_key(values, &key).unwrap_or(key);
                values.insert(key, value);
                Ok(())
            }
        })
    }

    pub(crate) fn remove_key(&mut self, section: &str, key: &str) -> Result<()> {
        let (section, registration) = self.ini_target(section)?;
        let field = registration
            .as_ref()
            .and_then(|registration| ini_field(registration, key))
            .cloned();
        if field.is_none()
            && registration
                .as_ref()
                .is_some_and(|registration| reserved_key(registration, key))
        {
            return Err(format!("INI key {key} has no registered mapping"));
        }
        self.update(|document| {
            let section = if registration.is_some() {
                section
            } else {
                matching_key(document, &section).unwrap_or(section)
            };
            if !document.contains_key(&section) {
                return Ok(());
            }
            let values = ini_table_mut(document, &section, registration.is_some())?;
            if let Some(field) = field {
                remove_path(values, &field.path)
            } else {
                if let Some(key) = matching_key(values, key) {
                    values.remove(&key);
                }
                Ok(())
            }
        })
    }

    pub(crate) fn remove_section(&mut self, section: &str) -> Result<()> {
        let (section, registration) = self.ini_target(section)?;
        self.update(|document| {
            let section = if registration.is_some() {
                section
            } else {
                matching_key(document, &section).unwrap_or(section)
            };
            if document.contains_key(&section) {
                ini_table_mut(document, &section, registration.is_some())?;
                document.remove(&section);
            }
            Ok(())
        })
    }

    fn ini_registration(&self, name: &str) -> Result<Option<(&str, &Registration)>> {
        if let Some((section, registration)) =
            self.registrations.iter().find(|(_, registration)| {
                registration
                    .ini
                    .as_ref()
                    .is_some_and(|ini| ini.name.eq_ignore_ascii_case(name))
            })
        {
            return Ok(Some((section, registration)));
        }
        if self
            .registrations
            .keys()
            .any(|section| section.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "configuration section [{name}] is not exposed under this INI name"
            ));
        }
        Ok(None)
    }

    fn ini_target(&self, name: &str) -> Result<(String, Option<Registration>)> {
        valid_name(name, "INI section")?;
        match self.ini_registration(name)? {
            Some((section, registration)) => Ok((section.to_owned(), Some(registration.clone()))),
            None => Ok((name.to_owned(), None)),
        }
    }

    fn reserved_section(&self, name: &str) -> bool {
        self.registrations.iter().any(|(section, registration)| {
            section.eq_ignore_ascii_case(name)
                || registration
                    .ini
                    .as_ref()
                    .is_some_and(|ini| ini.name.eq_ignore_ascii_case(name))
        })
    }

    fn update(&mut self, update: impl FnOnce(&mut Table) -> Result<()>) -> Result<()> {
        let mut document = read_document(&self.path)?;
        update(&mut document)?;
        for (section, registration) in &self.registrations {
            validate_section_aliases(&document, section, registration)?;
            if !registration.fixed.is_empty() {
                merge(table_mut(&mut document, section)?, &registration.fixed);
            }
            effective_section(&document, section, Some(registration))?;
        }
        let source = toml::to_string_pretty(&document)
            .map_err(|error| format!("failed to serialize {}: {error}", self.path.display()))?;
        fs::write(&self.path, source)
            .map_err(|error| format!("failed to write {}: {error}", self.path.display()))?;
        self.document = document;
        Ok(())
    }
}

fn read_document(path: &Path) -> Result<Table> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    toml::from_str(&source).map_err(|error| format!("failed to parse {}: {error}", path.display()))
}

fn effective_section(
    document: &Table,
    section: &str,
    registration: Option<&Registration>,
) -> Result<Table> {
    let mut values = registration
        .map(|registration| registration.defaults.clone())
        .unwrap_or_default();
    if let Some(existing) = document.get(section) {
        merge(
            &mut values,
            existing
                .as_table()
                .ok_or_else(|| format!("configuration section [{section}] must be a TOML table"))?,
        );
    }
    if let Some(registration) = registration {
        merge(&mut values, &registration.fixed);
        validate_types(&values, &registration.defaults, section)?;
        if let Some(ini) = &registration.ini {
            for field in &ini.fields {
                if let Some(value) = path_value(&values, &field.path) {
                    to_ini(&field.kind, value).map_err(|error| {
                        format!("[{section}] {}: {error}", field.path.join("."))
                    })?;
                }
            }
        }
    }
    Ok(values)
}

fn validate_section_aliases(
    document: &Table,
    section: &str,
    registration: &Registration,
) -> Result<()> {
    for name in document.keys() {
        if name != section
            && (name.eq_ignore_ascii_case(section)
                || registration
                    .ini
                    .as_ref()
                    .is_some_and(|ini| ini.name.eq_ignore_ascii_case(name)))
        {
            return Err(format!(
                "TOML section [{name}] conflicts with registered [{section}]"
            ));
        }
    }
    Ok(())
}

fn validate_types(values: &Table, defaults: &Table, section: &str) -> Result<()> {
    for (key, default) in defaults {
        let Some(value) = values.get(key) else {
            continue;
        };
        let path = format!("{section}.{key}");
        if let (Value::Table(values), Value::Table(defaults)) = (value, default) {
            validate_types(values, defaults, &path)?;
        } else if std::mem::discriminant(value) != std::mem::discriminant(default) {
            return Err(format!(
                "{path} must be {}, got {}",
                default.type_str(),
                value.type_str()
            ));
        }
    }
    Ok(())
}

fn validate_registration(registration: &Registration) -> Result<()> {
    let Some(ini) = &registration.ini else {
        let mut values = registration.defaults.clone();
        merge(&mut values, &registration.fixed);
        return validate_types(&values, &registration.defaults, "defaults");
    };
    valid_name(&ini.name, "INI section")?;
    let mut keys = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for field in &ini.fields {
        valid_name(&field.key, "INI key")?;
        if !keys.insert(field.key.to_ascii_lowercase()) {
            return Err(format!("duplicate INI key {}", field.key));
        }
        if field.path.is_empty() || !paths.insert(field.path.clone()) {
            return Err(format!("invalid or duplicate INI path for {}", field.key));
        }
        for part in &field.path {
            valid_name(part, "INI field path")?;
        }
        if let IniKind::Integer { min, max } = &field.kind
            && min > max
        {
            return Err(format!("INI integer {} has an invalid range", field.key));
        }
        if let IniKind::Enum { values } = &field.kind {
            let unique: BTreeSet<_> = values.values().collect();
            if values.is_empty() || unique.len() != values.len() {
                return Err(format!(
                    "INI enum {} must have unique numeric values",
                    field.key
                ));
            }
        }
    }
    effective_section(&Table::new(), "defaults", Some(registration)).map(|_| ())
}

fn valid_name(name: &str, kind: &str) -> Result<()> {
    if name.is_empty() || name.contains('\0') {
        Err(format!("{kind} must be nonempty and contain no NUL"))
    } else {
        Ok(())
    }
}

fn merge(values: &mut Table, patch: &Table) {
    for (key, value) in patch {
        if let (Some(Value::Table(existing)), Value::Table(patch)) = (values.get_mut(key), value) {
            merge(existing, patch);
        } else {
            values.insert(key.clone(), value.clone());
        }
    }
}

fn table_mut<'a>(document: &'a mut Table, section: &str) -> Result<&'a mut Table> {
    document
        .entry(section)
        .or_insert_with(|| Value::Table(Table::new()))
        .as_table_mut()
        .ok_or_else(|| format!("configuration section [{section}] must be a TOML table"))
}

fn ini_table_mut<'a>(
    document: &'a mut Table,
    section: &str,
    registered: bool,
) -> Result<&'a mut Table> {
    let table = table_mut(document, section)?;
    if !registered && !string_table(table) {
        return Err(format!(
            "unregistered INI section [{section}] must contain only strings"
        ));
    }
    Ok(table)
}

fn matching_key(table: &Table, key: &str) -> Option<String> {
    table
        .keys()
        .find(|name| name.eq_ignore_ascii_case(key))
        .cloned()
}

fn string_table(table: &Table) -> bool {
    table.values().all(Value::is_str)
}

fn legacy_section<'a>(document: &'a Table, section: &str) -> Option<&'a Table> {
    document
        .get(&matching_key(document, section)?)?
        .as_table()
        .filter(|table| string_table(table))
}

fn string_value(values: &Table, key: &str) -> Option<String> {
    values
        .get(&matching_key(values, key)?)?
        .as_str()
        .map(str::to_owned)
}

fn ini_field<'a>(registration: &'a Registration, key: &str) -> Option<&'a IniField> {
    registration
        .ini
        .as_ref()?
        .fields
        .iter()
        .find(|field| field.key.eq_ignore_ascii_case(key))
}

fn reserved_key(registration: &Registration, key: &str) -> bool {
    registration
        .defaults
        .keys()
        .chain(registration.fixed.keys())
        .any(|name| name.eq_ignore_ascii_case(key))
        || registration
            .ini
            .as_ref()
            .into_iter()
            .flat_map(|ini| &ini.fields)
            .any(|field| {
                field.key.eq_ignore_ascii_case(key)
                    || field
                        .path
                        .first()
                        .is_some_and(|name| name.eq_ignore_ascii_case(key))
            })
}

fn path_value<'a>(table: &'a Table, path: &[String]) -> Option<&'a Value> {
    let (first, rest) = path.split_first()?;
    let mut value = table.get(first)?;
    for part in rest {
        value = value.as_table()?.get(part)?;
    }
    Some(value)
}

fn set_path(table: &mut Table, path: &[String], value: Value) -> Result<()> {
    let (first, rest) = path.split_first().ok_or("INI field path is empty")?;
    if rest.is_empty() {
        table.insert(first.clone(), value);
        Ok(())
    } else {
        set_path(table_mut(table, first)?, rest, value)
    }
}

fn remove_path(table: &mut Table, path: &[String]) -> Result<()> {
    let (first, rest) = path.split_first().ok_or("INI field path is empty")?;
    if rest.is_empty() {
        table.remove(first);
    } else if table.contains_key(first) {
        let values = table_mut(table, first)?;
        remove_path(values, rest)?;
        if values.is_empty() {
            table.remove(first);
        }
    }
    Ok(())
}

fn to_ini(kind: &IniKind, value: &Value) -> Result<String> {
    match kind {
        IniKind::Boolean => value
            .as_bool()
            .map(|value| if value { "1" } else { "0" }.to_owned()),
        IniKind::Integer { min, max } => value
            .as_integer()
            .filter(|value| min <= value && value <= max)
            .map(|value| value.to_string()),
        IniKind::String => value.as_str().map(str::to_owned),
        IniKind::Enum { values } => value
            .as_str()
            .and_then(|value| values.get(value))
            .map(ToString::to_string),
    }
    .ok_or_else(|| format!("value {value} does not match its registered INI kind"))
}

fn parse_ini(kind: &IniKind, value: &str) -> Result<Value> {
    match kind {
        IniKind::Boolean => match value.trim() {
            "0" => Ok(Value::Boolean(false)),
            "1" => Ok(Value::Boolean(true)),
            _ => Err(format!("expected INI boolean 0 or 1, got {value:?}")),
        },
        IniKind::Integer { min, max } => {
            let value = integer(value)?;
            if (*min..=*max).contains(&value) {
                Ok(Value::Integer(value))
            } else {
                Err(format!(
                    "INI integer {value} must be between {min} and {max}"
                ))
            }
        }
        IniKind::String => Ok(Value::String(value.to_owned())),
        IniKind::Enum { values } => {
            let number = integer(value)?;
            values
                .iter()
                .find(|(_, value)| **value == number)
                .map(|(name, _)| Value::String(name.clone()))
                .ok_or_else(|| format!("unsupported INI enum value {number}"))
        }
    }
}

fn integer(value: &str) -> Result<i64> {
    let value = value.trim();
    value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .map_or_else(|| value.parse::<i64>(), |hex| i64::from_str_radix(hex, 16))
        .map_err(|error| format!("invalid INI integer {value:?}: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::IniSection;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct File(PathBuf);
    impl File {
        fn new(source: &str) -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "mhf-config-store-{}-{}.toml",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, source).unwrap();
            Self(path)
        }
    }
    impl Drop for File {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }

    fn definition() -> Registration {
        Registration {
            defaults: toml::from_str(
                "enabled = true\nmode = 'normal'\n[window]\nwidth = 800\nheight = 600",
            )
            .unwrap(),
            fixed: toml::from_str("mode = 'normal'").unwrap(),
            ini: Some(IniSection {
                name: "DISPLAY".into(),
                fields: vec![
                    IniField {
                        key: "ENABLED".into(),
                        path: vec!["enabled".into()],
                        kind: IniKind::Boolean,
                    },
                    IniField {
                        key: "MODE".into(),
                        path: vec!["mode".into()],
                        kind: IniKind::Enum {
                            values: BTreeMap::from([
                                ("normal".into(), 1),
                                ("alternative".into(), 2),
                            ]),
                        },
                    },
                    IniField {
                        key: "WIDTH".into(),
                        path: vec!["window".into(), "width".into()],
                        kind: IniKind::Integer {
                            min: 0,
                            max: i64::from(u16::MAX),
                        },
                    },
                ],
            }),
        }
    }

    #[test]
    fn registration_merges_defaults_and_fixed_without_rewriting_the_file() {
        let source = "[display]\nmode = 'old'\n[display.window]\nwidth = 1200\n[hidden]\nsecret = 'private'\n[legacy]\nvalue = '42'\n[structured]\nvalue = 42";
        let file = File::new(source);
        let mut store = Store::load(file.0.clone()).unwrap();
        store.register("display", definition()).unwrap();
        store.register("display", definition()).unwrap();
        store.register("hidden", Registration::default()).unwrap();
        let values = store.read("display").unwrap();
        assert_eq!(values["window"]["width"].as_integer(), Some(1200));
        assert_eq!(values["window"]["height"].as_integer(), Some(600));
        assert_eq!(values["mode"].as_str(), Some("normal"));
        assert_eq!(fs::read_to_string(&file.0).unwrap(), source);
        assert_eq!(store.value("display", "WIDTH").as_deref(), Some("1200"));
        assert_eq!(store.value("DISPLAY", "MODE").as_deref(), Some("1"));
        assert_eq!(store.value("legacy", "VALUE").as_deref(), Some("42"));
        assert!(store.value("HIDDEN", "secret").is_none());
        assert!(store.value("structured", "value").is_none());
        assert_eq!(store.section_names(), ["DISPLAY", "legacy"]);
        assert!(
            store
                .set_value("hidden".into(), "secret".into(), "changed".into())
                .is_err()
        );
        assert!(store.remove_section("structured").is_err());
        assert!(store.register("display", Registration::default()).is_err());
        assert_eq!(fs::read_to_string(&file.0).unwrap(), source);
    }

    #[test]
    fn writes_merge_fresh_fields_and_normalize_fixed_for_toml_and_ini() {
        let file = File::new("[display.window]\nwidth = 800");
        let mut store = Store::load(file.0.clone()).unwrap();
        store.register("display", definition()).unwrap();
        fs::write(&file.0, "[display.window]\nwidth = 900\nheight = 700\n[mods.extra]\nenabled = true\n[custom]\nvalue = 'keep'").unwrap();
        store
            .write(
                "display",
                toml::from_str("mode = 'alternative'\n[window]\nwidth = 1400").unwrap(),
            )
            .unwrap();
        assert_eq!(
            store.read("display").unwrap()["window"]["height"].as_integer(),
            Some(700)
        );
        assert_eq!(
            store.document()["mods"]["extra"]["enabled"].as_bool(),
            Some(true)
        );
        assert_eq!(store.document()["custom"]["value"].as_str(), Some("keep"));
        assert_eq!(store.document()["display"]["mode"].as_str(), Some("normal"));
        store
            .set_value("DISPLAY".into(), "WIDTH".into(), "0x640".into())
            .unwrap();
        store
            .set_value("DISPLAY".into(), "MODE".into(), "2".into())
            .unwrap();
        assert_eq!(store.value("DISPLAY", "WIDTH").as_deref(), Some("1600"));
        assert_eq!(store.value("DISPLAY", "MODE").as_deref(), Some("1"));
        store.remove_key("DISPLAY", "WIDTH").unwrap();
        assert_eq!(store.value("DISPLAY", "WIDTH").as_deref(), Some("800"));
        assert_eq!(
            store.read("display").unwrap()["window"]["height"].as_integer(),
            Some(700)
        );
        let saved: Table = toml::from_str(&fs::read_to_string(&file.0).unwrap()).unwrap();
        assert_eq!(&saved, store.document());
    }

    #[test]
    fn invalid_patches_or_fresh_documents_never_replace_saved_or_cached_data() {
        let file = File::new("[display.window]\nwidth = 800");
        let mut store = Store::load(file.0.clone()).unwrap();
        store.register("display", definition()).unwrap();
        let initial = fs::read_to_string(&file.0).unwrap();
        let cached = store.document().clone();
        assert!(
            store
                .write(
                    "display",
                    toml::from_str("[window]\nwidth = 'wrong'").unwrap()
                )
                .is_err()
        );
        assert!(
            store
                .set_value("DISPLAY".into(), "MODE".into(), "3".into())
                .is_err()
        );
        for invalid in ["-1", "65536"] {
            assert!(
                store
                    .set_value("DISPLAY".into(), "WIDTH".into(), invalid.into())
                    .is_err()
            );
        }
        assert!(
            store
                .write(
                    "display",
                    toml::from_str("[window]\nwidth = 65536").unwrap()
                )
                .is_err()
        );
        assert_eq!(fs::read_to_string(&file.0).unwrap(), initial);
        for invalid in [
            "invalid = [",
            "display = 'not a table'",
            "[display]\nwindow = 'not a table'",
            "[DISPLAY.window]\nwidth = 900",
        ] {
            fs::write(&file.0, invalid).unwrap();
            assert!(
                store
                    .set_value("DISPLAY".into(), "WIDTH".into(), "1920".into())
                    .is_err()
            );
            assert_eq!(fs::read_to_string(&file.0).unwrap(), invalid);
            assert_eq!(store.document(), &cached);
        }
        fs::remove_file(&file.0).unwrap();
        assert!(store.write("display", Table::new()).is_err());
        assert!(!file.0.exists());
        assert_eq!(store.document(), &cached);
    }
}
