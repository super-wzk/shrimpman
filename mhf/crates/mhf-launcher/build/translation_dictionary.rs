#[allow(dead_code)]
#[path = "../src/localization/native/layout.rs"]
mod native_layout;

use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    fmt::{self, Write as _},
    fs,
    path::{Path, PathBuf},
};

const TRANSLATIONS_DIRECTORY: &str = "translations";
const RESOURCE_LAYOUT_FILE: &str = "resources.json";
const GENERATED_RUST_FILE: &str = "translations.rs";
const GENERATED_BINARY_FILE: &str = "translations.bin";
const TRANSLATION_ENTRY_SIZE: usize = 20;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceLayoutFile {
    version: u32,
    record_layouts: BTreeMap<String, RecordLayout>,
    resources: Vec<ResourceDefinition>,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum ResourceDefinition {
    Records {
        id: String,
        identity: Option<ResourceIdentity>,
        code_page: Option<u32>,
        runtime: ResourceRuntimeDefinition,
        tables: Vec<RecordTableDefinition>,
        table_directories: Vec<TableDirectory>,
    },
    Quest {
        id: String,
        identity: Option<ResourceIdentity>,
        code_page: Option<u32>,
        runtime: ResourceRuntimeDefinition,
        layout: QuestTableDefinition,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceIdentity {
    magic: u32,
    format_version: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ResourceRuntimeDefinition {
    post_relocation: CodeHookDefinition,
    buffer_rva: String,
    size_rva: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CodeHookDefinition {
    rva: String,
    signature: String,
}

#[derive(Clone, Copy, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(deny_unknown_fields)]
struct RecordLayout {
    stride: u16,
    text_offset: u16,
    parts: u16,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecordTableDefinition {
    id: String,
    root_field: FieldPath,
    #[serde(default)]
    first_record: u32,
    records: RecordCount,
    layout: String,
    directory: Option<DirectoryEntry>,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct DirectoryEntry {
    index: u32,
    count: RecordCount,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDirectory {
    first_root_field: u32,
    root_stride: u32,
    layout: String,
    entries: Vec<TableDirectoryEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableDirectoryEntry {
    id: String,
    records: RecordCount,
}

#[derive(Clone, Deserialize)]
#[serde(untagged)]
enum FieldPath {
    Root(u32),
    Indirect(Vec<u32>),
}

impl FieldPath {
    fn offsets(&self) -> &[u32] {
        match self {
            Self::Root(offset) => std::slice::from_ref(offset),
            Self::Indirect(offsets) => offsets,
        }
    }
}

#[derive(Clone, Deserialize)]
#[serde(untagged, deny_unknown_fields)]
enum RecordCount {
    Fixed(u32),
    U16 { u16_at: FieldPath },
    U32 { u32_at: FieldPath },
    Sentinel { until: SentinelCount },
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SentinelCount {
    root_field: FieldPath,
    stride: u16,
    offset: u16,
    width: u8,
    value: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct QuestTableDefinition {
    id: String,
    root_field: u32,
    count_root_field: u32,
    category_stride: u16,
    category_count_field: u16,
    category_records_field: u16,
    record_text_field: u16,
    record_id_field: u16,
    parts: u16,
}

struct ResourceLayout {
    id: String,
    identity: Option<ResourceIdentity>,
    code_page: Option<u32>,
    runtime: ResourceRuntime,
    body: ResourceBody,
}

struct ResourceRuntime {
    post_relocation_rva: u32,
    post_relocation_signature: Vec<(usize, u8)>,
    buffer_rva: u32,
    size_rva: u32,
}

enum ResourceBody {
    Records(Vec<RecordTable>),
    Quest(QuestTable),
}

struct RecordTable {
    id: String,
    root: FieldPath,
    first_record: u32,
    records: RecordCount,
    text_offset: u16,
    parts: u16,
    stride: u16,
    directory: Option<DirectoryEntry>,
}

struct QuestTable {
    id: String,
    root: u32,
    count_root: u32,
    category_stride: u16,
    category_count_field: u16,
    category_records_field: u16,
    record_text_field: u16,
    record_id_field: u16,
    parts: u16,
}

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

type ResourceGroupIds = BTreeMap<String, BTreeMap<String, u32>>;

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

pub(super) fn generate() -> Result<(), String> {
    let manifest_directory = PathBuf::from(
        env::var_os("CARGO_MANIFEST_DIR")
            .ok_or_else(|| "CARGO_MANIFEST_DIR is not set".to_owned())?,
    );
    let translations_directory = manifest_directory.join(TRANSLATIONS_DIRECTORY);
    let layout_path = translations_directory.join(RESOURCE_LAYOUT_FILE);
    println!("cargo:rerun-if-changed={}", layout_path.display());
    println!("cargo:rerun-if-changed=src/localization/native/layout.rs");
    let layouts = read_resource_layouts(&layout_path)?;
    let group_ids = resource_group_ids(&layouts)?;
    let output_directory =
        PathBuf::from(env::var_os("OUT_DIR").ok_or_else(|| "OUT_DIR is not set".to_owned())?);

    let mut resources =
        "// @generated by build.rs from the resource layout. Do not edit.\n\n".to_owned();
    render_resource_layouts(&mut resources, &layouts, &group_ids);
    render_main_resource_hooks(&mut resources, &layouts);
    resources.push_str("const NATIVE_GROUPS: &[(&str, u32)] = &[\n");
    for (name, id) in group_ids.get("native").expect("native groups exist") {
        writeln!(resources, "    ({name:?}, {id}),").expect("writing to String cannot fail");
    }
    resources.push_str("];\n");
    fs::write(output_directory.join("resources.rs"), resources)
        .map_err(|error| format!("failed to write resources.rs: {error}"))?;

    println!(
        "cargo:rerun-if-changed={}",
        translations_directory.display()
    );
    let locales = read_locales(&translations_directory, &layouts)?;
    let (generated_rust, generated_binary) = compile_dictionary(&locales, &group_ids)?;
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
    layouts: &[ResourceLayout],
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

        let translations = read_dictionary(&path, layouts)?;
        locales.push(Locale { id, translations });
    }
    locales.sort_by_cached_key(|locale| locale.id.to_ascii_lowercase());
    Ok(locales)
}

fn read_resource_layouts(path: &Path) -> Result<Vec<ResourceLayout>, String> {
    let contents = read_utf8(path)?;
    let definitions: ResourceLayoutFile = serde_json::from_str(&contents)
        .map_err(|error| format!("failed to parse {}: {error}", path.display()))?;
    if definitions.version != 4 {
        return Err(format!(
            "{} has unsupported resource layout version {}",
            path.display(),
            definitions.version
        ));
    }
    if definitions.record_layouts.is_empty() {
        return Err(format!(
            "{} does not define any reusable record layouts",
            path.display()
        ));
    }
    let mut record_shapes = BTreeMap::new();
    for (id, layout) in &definitions.record_layouts {
        validate_record_layout(path, id, *layout)?;
        if let Some(previous_id) = record_shapes.insert(*layout, id) {
            return Err(format!(
                "{} record layouts {previous_id:?} and {id:?} describe the same shape",
                path.display()
            ));
        }
    }
    if definitions.resources.is_empty() {
        return Err(format!("{} does not define any resources", path.display()));
    }

    let mut resource_ids = BTreeSet::new();
    let mut layouts = Vec::with_capacity(definitions.resources.len());
    for definition in definitions.resources {
        let resource = expand_resource_layout(path, &definitions.record_layouts, definition)?;
        if !resource_ids.insert(resource.id.clone()) {
            return Err(format!(
                "{} defines resource {:?} more than once",
                path.display(),
                resource.id
            ));
        }
        validate_resource_layout(path, &resource)?;
        layouts.push(resource);
    }
    Ok(layouts)
}

fn validate_record_layout(path: &Path, id: &str, layout: RecordLayout) -> Result<(), String> {
    if !valid_identifier(id) {
        return Err(format!(
            "{} has invalid record layout id {id:?}",
            path.display()
        ));
    }
    let text_end = u32::from(layout.text_offset) + u32::from(layout.parts) * 4;
    if layout.parts == 0
        || !layout.text_offset.is_multiple_of(4)
        || layout.stride == 0
        || !layout.stride.is_multiple_of(4)
        || text_end > u32::from(layout.stride)
    {
        return Err(format!(
            "{} has invalid record layout {id:?}",
            path.display()
        ));
    }
    Ok(())
}

fn expand_resource_layout(
    path: &Path,
    record_layouts: &BTreeMap<String, RecordLayout>,
    definition: ResourceDefinition,
) -> Result<ResourceLayout, String> {
    let (id, identity, code_page, runtime, body) = match definition {
        ResourceDefinition::Records {
            id,
            identity,
            code_page,
            runtime,
            tables,
            table_directories,
        } => {
            let tables =
                expand_record_tables(path, &id, record_layouts, tables, table_directories)?;
            (
                id,
                identity,
                code_page,
                runtime,
                ResourceBody::Records(tables),
            )
        }
        ResourceDefinition::Quest {
            id,
            identity,
            code_page,
            runtime,
            layout,
        } => {
            let quest = QuestTable {
                id: layout.id,
                root: layout.root_field,
                count_root: layout.count_root_field,
                category_stride: layout.category_stride,
                category_count_field: layout.category_count_field,
                category_records_field: layout.category_records_field,
                record_text_field: layout.record_text_field,
                record_id_field: layout.record_id_field,
                parts: layout.parts,
            };
            (id, identity, code_page, runtime, ResourceBody::Quest(quest))
        }
    };
    let runtime = expand_resource_runtime(path, &id, runtime)?;
    Ok(ResourceLayout {
        id,
        identity,
        code_page,
        runtime,
        body,
    })
}

fn expand_resource_runtime(
    path: &Path,
    resource_id: &str,
    definition: ResourceRuntimeDefinition,
) -> Result<ResourceRuntime, String> {
    let post_relocation_rva = parse_rva(
        path,
        resource_id,
        "post-relocation hook RVA",
        &definition.post_relocation.rva,
    )?;
    let post_relocation_signature =
        parse_signature(path, resource_id, &definition.post_relocation.signature)?;
    let buffer_rva = parse_rva(path, resource_id, "buffer RVA", &definition.buffer_rva)?;
    let size_rva = parse_rva(path, resource_id, "size RVA", &definition.size_rva)?;
    if post_relocation_rva == 0 || buffer_rva == 0 || size_rva == 0 {
        return Err(format!(
            "{} resource {resource_id:?} has a zero runtime RVA",
            path.display()
        ));
    }
    if !buffer_rva.is_multiple_of(4) || !size_rva.is_multiple_of(4) {
        return Err(format!(
            "{} resource {resource_id:?} has unaligned runtime fields",
            path.display()
        ));
    }
    Ok(ResourceRuntime {
        post_relocation_rva,
        post_relocation_signature,
        buffer_rva,
        size_rva,
    })
}

fn parse_rva(path: &Path, resource_id: &str, name: &str, value: &str) -> Result<u32, String> {
    let digits = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or_else(|| {
            format!(
                "{} resource {resource_id:?} {name} must use a 0x-prefixed hexadecimal value",
                path.display()
            )
        })?;
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(format!(
            "{} resource {resource_id:?} has invalid {name} {value:?}",
            path.display()
        ));
    }
    u32::from_str_radix(digits, 16).map_err(|error| {
        format!(
            "{} resource {resource_id:?} has invalid {name} {value:?}: {error}",
            path.display()
        )
    })
}

fn parse_signature(
    path: &Path,
    resource_id: &str,
    value: &str,
) -> Result<Vec<(usize, u8)>, String> {
    let mut signature = Vec::new();
    let mut length = 0;
    for (offset, token) in value.split_ascii_whitespace().enumerate() {
        length = offset + 1;
        if token == "??" {
            continue;
        }
        if token.len() != 2 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(format!(
                "{} resource {resource_id:?} has invalid post-relocation signature byte {token:?}",
                path.display()
            ));
        }
        let byte = u8::from_str_radix(token, 16).expect("two hexadecimal digits fit in u8");
        signature.push((offset, byte));
    }
    if length == 0
        || signature
            .last()
            .is_none_or(|(offset, _)| offset + 1 != length)
    {
        return Err(format!(
            "{} resource {resource_id:?} has an empty or wildcard-ended post-relocation signature",
            path.display()
        ));
    }
    Ok(signature)
}

fn expand_record_tables(
    path: &Path,
    resource_id: &str,
    record_layouts: &BTreeMap<String, RecordLayout>,
    definitions: Vec<RecordTableDefinition>,
    directories: Vec<TableDirectory>,
) -> Result<Vec<RecordTable>, String> {
    let mut tables = Vec::new();
    for table in definitions {
        let mut expanded = expand_record_table(
            path,
            resource_id,
            record_layouts,
            table.id,
            table.root_field,
            table.records,
            &table.layout,
        )?;
        expanded.directory = table.directory;
        expanded.first_record = table.first_record;
        tables.push(expanded);
    }
    for directory in directories {
        if directory.entries.len() < 2
            || !directory.first_root_field.is_multiple_of(4)
            || directory.root_stride == 0
            || !directory.root_stride.is_multiple_of(4)
        {
            return Err(format!(
                "{} resource {resource_id:?} has an invalid table directory at root field {}",
                path.display(),
                directory.first_root_field
            ));
        }
        for (index, entry) in directory.entries.into_iter().enumerate() {
            let root = u32::try_from(index)
                .ok()
                .and_then(|index| index.checked_mul(directory.root_stride))
                .and_then(|offset| directory.first_root_field.checked_add(offset))
                .ok_or_else(|| {
                    format!(
                        "{} resource {resource_id:?} table directory root fields overflow",
                        path.display()
                    )
                })?;
            tables.push(expand_record_table(
                path,
                resource_id,
                record_layouts,
                entry.id,
                FieldPath::Root(root),
                entry.records,
                &directory.layout,
            )?);
        }
    }
    tables.sort_by(|left, right| {
        (left.root.offsets(), left.first_record).cmp(&(right.root.offsets(), right.first_record))
    });
    Ok(tables)
}

fn expand_record_table(
    path: &Path,
    resource_id: &str,
    record_layouts: &BTreeMap<String, RecordLayout>,
    id: String,
    root: FieldPath,
    records: RecordCount,
    layout_id: &str,
) -> Result<RecordTable, String> {
    let layout = record_layouts.get(layout_id).ok_or_else(|| {
        format!(
            "{} resource {resource_id:?} table {id:?} references unknown record layout {layout_id:?}",
            path.display()
        )
    })?;
    Ok(RecordTable {
        id,
        root,
        first_record: 0,
        records,
        text_offset: layout.text_offset,
        parts: layout.parts,
        stride: layout.stride,
        directory: None,
    })
}

fn validate_resource_layout(path: &Path, resource: &ResourceLayout) -> Result<(), String> {
    if resource
        .identity
        .as_ref()
        .is_some_and(|identity| identity.magic == 0)
    {
        return Err(format!(
            "{} resource {:?} has an invalid identity",
            path.display(),
            resource.id
        ));
    }
    match &resource.body {
        ResourceBody::Records(tables) => validate_record_tables(path, resource, tables),
        ResourceBody::Quest(layout) => validate_quest_layout(path, resource, layout),
    }
}

fn validate_record_tables(
    path: &Path,
    resource: &ResourceLayout,
    tables: &[RecordTable],
) -> Result<(), String> {
    if tables.is_empty() {
        return Err(format!(
            "{} records resource {:?} has no tables",
            path.display(),
            resource.id
        ));
    }
    let mut previous_root = None;
    let mut table_ids = BTreeSet::new();
    for table in tables {
        if !valid_identifier(&table.id) {
            return Err(format!(
                "{} resource {:?} has invalid table id {:?}",
                path.display(),
                resource.id,
                table.id
            ));
        }
        if !table_ids.insert(&table.id) {
            return Err(format!(
                "{} resource {:?} defines table id {:?} more than once",
                path.display(),
                resource.id,
                table.id
            ));
        }
        validate_field_path(path, &table.root, 4)?;
        let location = (table.root.offsets(), table.first_record);
        if previous_root.is_some_and(|previous: (&[u32], u32)| previous >= location) {
            return Err(format!(
                "{} resource {:?} table paths and first records must be unique and strictly ordered",
                path.display(),
                resource.id
            ));
        }
        previous_root = Some(location);
        for count in
            std::iter::once(&table.records).chain(table.directory.iter().map(|entry| &entry.count))
        {
            match count {
                RecordCount::Fixed(0) => {
                    return Err(format!(
                        "{} resource {:?} has an empty fixed table {:?}",
                        path.display(),
                        resource.id,
                        table.id
                    ));
                }
                RecordCount::Fixed(_) => {}
                RecordCount::U16 { u16_at } => validate_field_path(path, u16_at, 2)?,
                RecordCount::U32 { u32_at } => validate_field_path(path, u32_at, 4)?,
                RecordCount::Sentinel { until } => {
                    validate_field_path(path, &until.root_field, 4)?;
                    if !matches!(until.width, 2 | 4)
                        || until.stride == 0
                        || u32::from(until.offset) + u32::from(until.width)
                            > u32::from(until.stride)
                        || (until.width == 2 && until.value > u16::MAX as u32)
                    {
                        return Err(format!(
                            "{} resource {:?} has an invalid sentinel count for {:?}",
                            path.display(),
                            resource.id,
                            table.id
                        ));
                    }
                }
            }
        }
    }
    Ok(())
}

fn validate_field_path(path: &Path, field: &FieldPath, width: u32) -> Result<(), String> {
    let Some((last, parents)) = field.offsets().split_last() else {
        return Err(format!("{} contains an empty field path", path.display()));
    };
    if !last.is_multiple_of(width) || parents.iter().any(|offset| !offset.is_multiple_of(4)) {
        return Err(format!(
            "{} contains an unaligned field path {:?}",
            path.display(),
            field.offsets()
        ));
    }
    Ok(())
}

fn validate_quest_layout(
    path: &Path,
    resource: &ResourceLayout,
    layout: &QuestTable,
) -> Result<(), String> {
    let category_count_fits = layout
        .category_count_field
        .checked_add(2)
        .is_some_and(|end| end <= layout.category_stride);
    let category_records_fit = layout
        .category_records_field
        .checked_add(4)
        .is_some_and(|end| end <= layout.category_stride);
    if !valid_identifier(&layout.id)
        || !layout.root.is_multiple_of(4)
        || !layout.count_root.is_multiple_of(4)
        || layout.root == layout.count_root
        || layout.category_stride == 0
        || !category_count_fits
        || !category_records_fit
        || !layout.category_records_field.is_multiple_of(4)
        || !layout.record_text_field.is_multiple_of(4)
        || !layout.record_id_field.is_multiple_of(2)
        || layout.record_text_field.checked_add(4).is_none()
        || layout.record_id_field.checked_add(2).is_none()
        || layout.parts == 0
    {
        return Err(format!(
            "{} resource {:?} has an invalid quest layout",
            path.display(),
            resource.id
        ));
    }
    Ok(())
}

fn valid_identifier(id: &str) -> bool {
    id.len() <= 64
        && id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
}

fn group_width(layouts: &[ResourceLayout], key: &TranslationGroupKey) -> Result<u16, String> {
    let (resource_id, group_id, record_id) = match key {
        TranslationGroupKey::Resource {
            resource_id,
            group_id,
            record_id,
        } => (resource_id, group_id, *record_id),
        TranslationGroupKey::Stage { .. } => return Ok(1),
    };
    if resource_id == "native" {
        let valid = if group_id == "literal" {
            native_layout::LITERALS
                .iter()
                .any(|literal| literal.rva == record_id as usize)
        } else {
            native_layout::TABLES
                .iter()
                .any(|table| table.id == group_id && record_id < table.records)
        };
        return if valid {
            Ok(1)
        } else {
            Err(format!("key {key} is absent from the native text layout"))
        };
    }
    let resource = layouts
        .iter()
        .find(|layout| layout.id == resource_id.as_str())
        .ok_or_else(|| format!("key {key} names a resource absent from resources.json"))?;
    match &resource.body {
        ResourceBody::Records(tables) => {
            let table = tables
                .iter()
                .find(|table| table.id == group_id.as_str())
                .ok_or_else(|| {
                    format!("key {key} names a group id absent from the resource layout")
                })?;
            if let RecordCount::Fixed(records) = table.records
                && record_id >= records
            {
                return Err(format!(
                    "key {key} is outside the table's {records}-record layout"
                ));
            }
            Ok(table.parts)
        }
        ResourceBody::Quest(layout) => {
            if record_id == 0 {
                return Err("quest ID 0 is reserved".to_owned());
            }
            if u16::try_from(record_id).is_err() {
                return Err("quest ID must fit in u16".to_owned());
            }
            if layout.id != group_id.as_str() {
                return Err(format!(
                    "key {key} names a group id absent from the resource layout"
                ));
            }
            Ok(layout.parts)
        }
    }
}

fn read_dictionary(path: &Path, layouts: &[ResourceLayout]) -> Result<Vec<Translation>, String> {
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
            group_width(layouts, &key).map_err(|error| location_error(path, line_number, error))?;
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
    group_ids: &ResourceGroupIds,
) -> Result<(String, Vec<u8>), String> {
    let (binary, compiled_locales) = compile_locales(locales, group_ids)?;
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

fn resource_group_ids(layouts: &[ResourceLayout]) -> Result<ResourceGroupIds, String> {
    let mut keys = BTreeSet::new();
    for resource in layouts {
        match &resource.body {
            ResourceBody::Records(tables) => {
                for table in tables {
                    keys.insert((resource.id.clone(), table.id.clone()));
                }
            }
            ResourceBody::Quest(layout) => {
                keys.insert((resource.id.clone(), layout.id.clone()));
            }
        }
    }

    keys.insert(("native".to_owned(), "literal".to_owned()));
    for table in native_layout::TABLES {
        keys.insert(("native".to_owned(), table.id.to_owned()));
    }
    let mut ids = ResourceGroupIds::new();
    for (index, (resource_id, group_id)) in keys.into_iter().enumerate() {
        let index = u32::try_from(index)
            .map_err(|_| "resource layout defines too many translation groups".to_owned())?;
        ids.entry(resource_id).or_default().insert(group_id, index);
    }
    Ok(ids)
}

fn resource_group_id(group_ids: &ResourceGroupIds, resource_id: &str, group_id: &str) -> u32 {
    *group_ids
        .get(resource_id)
        .and_then(|groups| groups.get(group_id))
        .expect("every validated resource group has a binary ID")
}

fn compile_locales(
    locales: &[Locale],
    group_ids: &ResourceGroupIds,
) -> Result<(Vec<u8>, Vec<CompiledLocaleLayout>), String> {
    let mut binary = Vec::new();
    let mut compiled_locales = Vec::with_capacity(locales.len());
    for locale in locales {
        let mut translations = locale
            .translations
            .iter()
            .map(|translation| {
                (
                    binary_translation_key(&translation.key, group_ids),
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

fn binary_translation_key(
    key: &TranslationKey,
    group_ids: &ResourceGroupIds,
) -> BinaryTranslationKey {
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
        } => (
            1,
            resource_group_id(group_ids, resource_id, group_id),
            *record_id,
        ),
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

fn render_resource_layouts(
    output: &mut String,
    layouts: &[ResourceLayout],
    group_ids: &ResourceGroupIds,
) {
    for (resource_index, resource) in layouts.iter().enumerate() {
        let ResourceBody::Records(tables) = &resource.body else {
            continue;
        };
        writeln!(
            output,
            "static RESOURCE_TABLES_{resource_index}: [RecordTableLayout; {}] = [",
            tables.len()
        )
        .expect("writing to String cannot fail");
        for table in tables {
            writeln!(
                output,
                "    RecordTableLayout {{ id: {:?}, translation_group: {}, root: &{:?}, first_record: {}, records: {}, text_offset: {}, parts: {}, stride: {}, directory: {} }},",
                table.id,
                resource_group_id(group_ids, &resource.id, &table.id),
                table.root.offsets(),
                table.first_record,
                render_record_count(&table.records),
                table.text_offset,
                table.parts,
                table.stride,
                table.directory.as_ref().map_or_else(|| "None".to_owned(), |entry| format!("Some(({}, {}))", entry.index, render_record_count(&entry.count))),
            )
            .expect("writing to String cannot fail");
        }
        output.push_str("];\n\n");
    }

    writeln!(
        output,
        "static RESOURCE_LAYOUTS: [ResourceLayout; {}] = [",
        layouts.len()
    )
    .expect("writing to String cannot fail");
    for (resource_index, resource) in layouts.iter().enumerate() {
        let body = match &resource.body {
            ResourceBody::Records(_) => {
                format!("ResourceBodyLayout::Records(&RESOURCE_TABLES_{resource_index})")
            }
            ResourceBody::Quest(layout) => format!(
                "ResourceBodyLayout::Quest(QuestTableLayout {{ id: {:?}, translation_group: {}, root: 0x{:08X}, count_root: 0x{:08X}, category_stride: {}, category_count_field: {}, category_records_field: {}, record_text_field: {}, record_id_field: {}, parts: {} }})",
                layout.id,
                resource_group_id(group_ids, &resource.id, &layout.id),
                layout.root,
                layout.count_root,
                layout.category_stride,
                layout.category_count_field,
                layout.category_records_field,
                layout.record_text_field,
                layout.record_id_field,
                layout.parts
            ),
        };
        writeln!(
            output,
            "    ResourceLayout {{ id: {:?}, identity: {:?}, code_page: {:?}, body: {body} }},",
            resource.id,
            resource
                .identity
                .as_ref()
                .map(|identity| (identity.magic, identity.format_version)),
            resource.code_page
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("];\n\n");
}

fn render_record_count(count: &RecordCount) -> String {
    match count {
        RecordCount::Fixed(count) => format!("RecordCount::Fixed({count})"),
        RecordCount::U16 { u16_at } => format!("RecordCount::U16(&{:?})", u16_at.offsets()),
        RecordCount::U32 { u32_at } => format!("RecordCount::U32(&{:?})", u32_at.offsets()),
        RecordCount::Sentinel { until } => format!(
            "RecordCount::Sentinel {{ root: &{:?}, stride: {}, offset: {}, width: {}, value: {} }}",
            until.root_field.offsets(),
            until.stride,
            until.offset,
            until.width,
            until.value,
        ),
    }
}

fn render_main_resource_hooks(output: &mut String, layouts: &[ResourceLayout]) {
    for (resource_index, resource) in layouts.iter().enumerate() {
        writeln!(
            output,
            "static MAIN_RESOURCE_SIGNATURE_{resource_index}: &[(usize, u8)] = &["
        )
        .expect("writing to String cannot fail");
        for (offset, byte) in &resource.runtime.post_relocation_signature {
            writeln!(output, "    ({offset}, 0x{byte:02X}),")
                .expect("writing to String cannot fail");
        }
        output.push_str("];\n");
        writeln!(
            output,
            "static MAIN_RESOURCE_ORIGINAL_{resource_index}: AtomicUsize = AtomicUsize::new(0);"
        )
        .expect("writing to String cannot fail");
        writeln!(
            output,
            "define_main_resource_detour!(main_resource_detour_{resource_index}, {resource_index}, MAIN_RESOURCE_ORIGINAL_{resource_index});\n"
        )
        .expect("writing to String cannot fail");
    }

    writeln!(
        output,
        "static MAIN_RESOURCE_BINDINGS: [MainResourceBinding; {}] = [",
        layouts.len()
    )
    .expect("writing to String cannot fail");
    for resource in layouts {
        writeln!(
            output,
            "    MainResourceBinding {{ id: {:?}, buffer_rva: 0x{:08X}, size_rva: 0x{:08X} }},",
            resource.id, resource.runtime.buffer_rva, resource.runtime.size_rva
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("];\n\n");

    writeln!(
        output,
        "fn main_resource_code_hooks() -> [CodeHook; {}] {{",
        layouts.len()
    )
    .expect("writing to String cannot fail");
    output.push_str("    [\n");
    for (resource_index, resource) in layouts.iter().enumerate() {
        writeln!(
            output,
            "        CodeHook {{ name: {:?}, rva: 0x{:08X}, signature: MAIN_RESOURCE_SIGNATURE_{resource_index}, detour: main_resource_detour_{resource_index} as *const () as *mut c_void, original: &MAIN_RESOURCE_ORIGINAL_{resource_index} }},",
            format!("resource {} post-relocation boundary", resource.id),
            resource.runtime.post_relocation_rva
        )
        .expect("writing to String cannot fail");
    }
    output.push_str("    ]\n}\n\n");
}

fn read_utf8(path: &Path) -> Result<String, String> {
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    if contents.starts_with('\u{feff}') {
        return Err(format!(
            "{} must be UTF-8 without a byte-order mark",
            path.display()
        ));
    }
    Ok(contents)
}

fn location_error(path: &Path, line: usize, error: impl std::fmt::Display) -> String {
    format!("{}:{line}: {error}", path.display())
}
