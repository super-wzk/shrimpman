use super::resource_layout::{ResourceCatalog, read_utf8, valid_identifier};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fmt::{self, Write as _},
    fs,
    path::Path,
};

const GENERATED_RUST_FILE: &str = "translations.rs";
const GENERATED_BINARY_FILE: &str = "translations.bin";
const TRANSLATION_ENTRY_SIZE: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TranslationRecord {
    key: String,
    source: Option<LocalizedValue>,
    translation: Option<LocalizedValue>,
    #[serde(rename = "context")]
    _context: Option<String>,
    #[serde(rename = "note")]
    _note: Option<String>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum LocalizedValue {
    Single(String),
    Parts(Vec<Option<String>>),
}

struct Translation {
    key: TranslationKey,
    text: String,
}

struct Locale {
    id: String,
    translations: Vec<Translation>,
}

struct CompiledLocaleLayout {
    id: String,
    entries_offset: usize,
    entries_len: usize,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BinaryTranslationKey {
    kind: u8,
    primary: u32,
    secondary: u32,
    part: u16,
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum TranslationGroupKey {
    Stage {
        stage: u16,
        section: u16,
        record: u16,
    },
    Resource {
        resource_id: String,
        group_id: String,
        record_id: u32,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct TranslationKey {
    group: TranslationGroupKey,
    part: u16,
}

impl fmt::Display for TranslationGroupKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stage {
                stage,
                section,
                record,
            } => write!(formatter, "stage:{stage:03}:{section:04X}:{record:04X}"),
            Self::Resource {
                resource_id,
                group_id,
                record_id,
            } => write!(formatter, "{resource_id}:{group_id}:{record_id}"),
        }
    }
}

impl fmt::Display for TranslationKey {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.part == 0 {
            write!(formatter, "{}", self.group)
        } else {
            write!(formatter, "{}:{:02}", self.group, self.part)
        }
    }
}

pub(super) fn generate(
    translations_directory: &Path,
    output_directory: &Path,
    catalog: &ResourceCatalog,
) -> Result<(), String> {
    println!(
        "cargo:rerun-if-changed={}",
        translations_directory.display()
    );
    let locales = read_locales(translations_directory, catalog)?;
    let (generated_rust, generated_binary) = compile_dictionary(&locales, catalog)?;
    fs::write(
        output_directory.join(GENERATED_BINARY_FILE),
        generated_binary,
    )
    .map_err(|error| format!("failed to write {GENERATED_BINARY_FILE}: {error}"))?;
    fs::write(output_directory.join(GENERATED_RUST_FILE), generated_rust)
        .map_err(|error| format!("failed to write {GENERATED_RUST_FILE}: {error}"))
}

fn read_locales(
    translations_directory: &Path,
    catalog: &ResourceCatalog,
) -> Result<Vec<Locale>, String> {
    let mut paths = fs::read_dir(translations_directory)
        .map_err(|error| {
            format!(
                "failed to read {}: {error}",
                translations_directory.display()
            )
        })?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            format!(
                "failed to enumerate {}: {error}",
                translations_directory.display()
            )
        })?;
    paths.retain(|path| {
        path.extension()
            .is_some_and(|extension| extension == "jsonl")
    });
    paths.sort();
    if paths.is_empty() {
        return Err(format!(
            "{} does not contain a .jsonl dictionary",
            translations_directory.display()
        ));
    }

    let mut locale_paths = BTreeMap::new();
    let mut locales = Vec::new();
    for path in paths {
        let id = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .ok_or_else(|| format!("{} has a non-Unicode file name", path.display()))?
            .to_owned();
        let normalized_id = id.to_ascii_lowercase();
        if let Some(previous_path) = locale_paths.insert(normalized_id, path.clone()) {
            return Err(format!(
                "translation locale {id:?} is defined by both {} and {}",
                previous_path.display(),
                path.display()
            ));
        }

        let translations = read_dictionary(&path, catalog)?;
        locales.push(Locale { id, translations });
    }
    locales.sort_by_cached_key(|locale| locale.id.to_ascii_lowercase());
    Ok(locales)
}

fn group_width(catalog: &ResourceCatalog, key: &TranslationGroupKey) -> Result<u16, String> {
    match key {
        TranslationGroupKey::Resource {
            resource_id,
            group_id,
            record_id,
        } => catalog.group_width(resource_id, group_id, *record_id),
        TranslationGroupKey::Stage { .. } => Ok(1),
    }
}

fn read_dictionary(path: &Path, catalog: &ResourceCatalog) -> Result<Vec<Translation>, String> {
    let contents = read_utf8(path)?;
    let mut keys = BTreeMap::new();
    let mut translations = Vec::new();
    for (index, line) in contents.lines().enumerate() {
        let line_number = index + 1;
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let record: TranslationRecord =
            serde_json::from_str(line).map_err(|error| location_error(path, line_number, error))?;
        let key = parse_group_key(&record.key)
            .map_err(|error| location_error(path, line_number, error))?;
        let width =
            group_width(catalog, &key).map_err(|error| location_error(path, line_number, error))?;
        if let Some(previous_line) = keys.insert(key.clone(), line_number) {
            return Err(location_error(
                path,
                line_number,
                format!(
                    "duplicate key {}; first defined at line {previous_line}",
                    record.key
                ),
            ));
        }

        if let Some(source) = record.source {
            normalize_value(path, line_number, "source", source, &key, width)?;
        }
        if let Some(translation) = record.translation {
            let translation =
                normalize_value(path, line_number, "translation", translation, &key, width)?;
            for (part, text) in translation.into_iter().enumerate() {
                let Some(text) = text else {
                    continue;
                };
                let key = TranslationKey {
                    group: key.clone(),
                    part: u16::try_from(part).expect("localized value width fits in u16"),
                };
                translations.push(Translation { key, text });
            }
        }
    }
    Ok(translations)
}

fn normalize_value(
    path: &Path,
    line: usize,
    field: &str,
    value: LocalizedValue,
    key: &TranslationGroupKey,
    width: u16,
) -> Result<Vec<Option<String>>, String> {
    match value {
        LocalizedValue::Single(text) => {
            if width != 1 {
                return Err(location_error(
                    path,
                    line,
                    format!("{field} for multipart key {key} must be an array"),
                ));
            }
            Ok(vec![Some(text)])
        }
        LocalizedValue::Parts(values) => {
            if values.len() != usize::from(width) {
                return Err(location_error(
                    path,
                    line,
                    format!(
                        "{field} for key {} must contain exactly {} part slots",
                        key, width
                    ),
                ));
            }
            Ok(values)
        }
    }
}

fn parse_group_key(value: &str) -> Result<TranslationGroupKey, String> {
    let mut components = value.split(':');
    let kind = components.next().unwrap_or_default();
    let key = match kind {
        "stage" => {
            let stage = parse_stage(components.next())?;
            let section = parse_hex_u16(components.next(), "section")?;
            let record = parse_hex_u16(components.next(), "record")?;
            TranslationGroupKey::Stage {
                stage,
                section,
                record,
            }
        }
        resource_id if valid_identifier(resource_id) => TranslationGroupKey::Resource {
            resource_id: resource_id.to_owned(),
            group_id: parse_group_id(components.next())?,
            record_id: parse_decimal_u32(components.next(), "record ID")?,
        },
        _ => {
            return Err("key must start with stage: or a valid resource ID".to_owned());
        }
    };
    if components.next().is_some() {
        return Err(match kind {
            "stage" => "key must have the form stage:NUMBER:SECTION:RECORD",
            _ => "resource key must have the form KIND:GROUP_ID:RECORD_ID",
        }
        .to_owned());
    }
    Ok(key)
}

fn parse_group_id(value: Option<&str>) -> Result<String, String> {
    let value = value.ok_or_else(|| "key is missing its group id".to_owned())?;
    if !valid_identifier(value) {
        return Err(
            "group id must contain 1 to 64 lowercase ASCII letters, digits, '.', '-', or '_'"
                .to_owned(),
        );
    }
    Ok(value.to_owned())
}

fn parse_decimal_u32(value: Option<&str>, name: &str) -> Result<u32, String> {
    let value = value.ok_or_else(|| format!("key is missing its {name}"))?;
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(format!("{name} must be an unsigned decimal number"));
    }
    value
        .parse()
        .map_err(|error| format!("invalid {name}: {error}"))
}

fn parse_hex_u16(value: Option<&str>, name: &str) -> Result<u16, String> {
    let value = value.ok_or_else(|| format!("key is missing its {name}"))?;
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!("key {name} must be an unsigned hexadecimal number"));
    }
    u16::from_str_radix(value, 16).map_err(|error| format!("key {name} must fit in u16: {error}"))
}

fn parse_stage(value: Option<&str>) -> Result<u16, String> {
    let value = parse_decimal_u32(value, "stage number")?;
    if value > 999 {
        return Err("stage number must be between 0 and 999".to_owned());
    }
    Ok(value as u16)
}

fn compile_dictionary(
    locales: &[Locale],
    catalog: &ResourceCatalog,
) -> Result<(String, Vec<u8>), String> {
    let (binary, compiled_locales) = compile_locales(locales, catalog)?;
    let mut output = "// @generated by build.rs from the resource layout and translations/*.jsonl. Do not edit.\n\n"
        .to_owned();

    writeln!(
        output,
        "const TRANSLATION_ENTRY_SIZE: usize = {TRANSLATION_ENTRY_SIZE};"
    )
    .expect("writing to String cannot fail");
    output.push('\n');
    writeln!(
        output,
        "static LOCALES: [CompiledLocale; {}] = [",
        compiled_locales.len()
    )
    .expect("writing to String cannot fail");
    for locale in compiled_locales {
        writeln!(
            output,
            "    CompiledLocale::new({:?}, {}, {}),",
            locale.id, locale.entries_offset, locale.entries_len
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("];\n\n");
    writeln!(
        output,
        "static TRANSLATION_DICTIONARY: CompiledDictionary = CompiledDictionary::new(include_bytes!(concat!(env!(\"OUT_DIR\"), \"/{GENERATED_BINARY_FILE}\")), &LOCALES);"
    )
    .expect("writing to String cannot fail");
    Ok((output, binary))
}

fn compile_locales(
    locales: &[Locale],
    catalog: &ResourceCatalog,
) -> Result<(Vec<u8>, Vec<CompiledLocaleLayout>), String> {
    let mut binary = Vec::new();
    let mut compiled_locales = Vec::with_capacity(locales.len());
    for locale in locales {
        let mut translations = locale
            .translations
            .iter()
            .map(|translation| {
                (
                    binary_translation_key(&translation.key, catalog),
                    translation,
                )
            })
            .collect::<Vec<_>>();
        translations.sort_by_key(|(key, _)| *key);

        let entries_offset = binary.len();
        let entries_size = translations
            .len()
            .checked_mul(TRANSLATION_ENTRY_SIZE)
            .ok_or_else(|| format!("locale {:?} has too many translations", locale.id))?;
        binary.resize(
            entries_offset
                .checked_add(entries_size)
                .ok_or_else(|| "compiled translation dictionary is too large".to_owned())?,
            0,
        );

        for (index, (key, translation)) in translations.into_iter().enumerate() {
            let record_offset = u32::try_from(binary.len())
                .map_err(|_| "compiled translation dictionary exceeds 4 GiB".to_owned())?;
            let record = translation.text.as_bytes();
            let record_len = u32::try_from(record.len().saturating_add(1))
                .map_err(|_| "compiled translation record exceeds 4 GiB".to_owned())?;
            binary.extend_from_slice(record);
            binary.push(0);
            let entry_offset = entries_offset + index * TRANSLATION_ENTRY_SIZE;
            write_translation_entry(
                &mut binary[entry_offset..entry_offset + TRANSLATION_ENTRY_SIZE],
                key,
                record_offset,
                record_len,
            );
        }
        if binary.len() > u32::MAX as usize {
            return Err("compiled translation dictionary exceeds 4 GiB".to_owned());
        }

        compiled_locales.push(CompiledLocaleLayout {
            id: locale.id.clone(),
            entries_offset,
            entries_len: locale.translations.len(),
        });
    }
    Ok((binary, compiled_locales))
}

fn binary_translation_key(key: &TranslationKey, catalog: &ResourceCatalog) -> BinaryTranslationKey {
    let (kind, primary, secondary) = match &key.group {
        TranslationGroupKey::Stage {
            stage,
            section,
            record,
        } => (
            0,
            u32::from(*stage),
            u32::from(*section) << 16 | u32::from(*record),
        ),
        TranslationGroupKey::Resource {
            resource_id,
            group_id,
            record_id,
        } => (1, catalog.group_id(resource_id, group_id), *record_id),
    };
    BinaryTranslationKey {
        kind,
        primary,
        secondary,
        part: key.part,
    }
}

fn write_translation_entry(
    entry: &mut [u8],
    key: BinaryTranslationKey,
    record_offset: u32,
    record_len: u32,
) {
    entry[0] = key.kind;
    entry[2..4].copy_from_slice(&key.part.to_le_bytes());
    entry[4..8].copy_from_slice(&key.primary.to_le_bytes());
    entry[8..12].copy_from_slice(&key.secondary.to_le_bytes());
    entry[12..16].copy_from_slice(&record_offset.to_le_bytes());
    entry[16..20].copy_from_slice(&record_len.to_le_bytes());
}

fn location_error(path: &Path, line: usize, error: impl std::fmt::Display) -> String {
    format!("{}:{line}: {error}", path.display())
}
