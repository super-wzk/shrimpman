use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::Path,
};

const RESOURCE_LAYOUT_FILE: &str = "resources/layout.json";

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
    Quest(QuestTableDefinition),
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

/// Generate DAT and INF layouts for offline resource inspection.
pub(super) fn generate_inspection(
    manifest_directory: &Path,
    output_directory: &Path,
) -> Result<(), String> {
    let layout_path = manifest_directory.join(RESOURCE_LAYOUT_FILE);
    println!("cargo:rerun-if-changed={}", layout_path.display());
    let layouts = read_resource_layouts(&layout_path)?;
    let resource = layouts
        .iter()
        .find(|layout| layout.id == "mhfdat")
        .ok_or("missing mhfdat resource layout")?;
    let ResourceBody::Records(tables) = &resource.body else {
        return Err("mhfdat must contain record tables".into());
    };
    let mut output = String::from("// Generated from shared/resource/resources/layout.json.\n");
    output.push_str("static TEXT_TABLES: &[TableLayout] = &[\n");
    for table in tables {
        writeln!(
            output,
            "TableLayout {{ id: {id:?}, label: {id:?}, root: &{root:?}, start_offset: 0, first_record: {first}, records: {count}, stride: {stride}, format: RecordFormat::Text {{ offset: {offset}, parts: {parts} }}, directory: {directory}, names: None }},",
            id = table.id,
            root = table.root.offsets(),
            first = table.first_record,
            count = render_record_count(&table.records),
            stride = table.stride,
            offset = table.text_offset,
            parts = table.parts,
            directory = table.directory.as_ref().map_or_else(
                || "None".to_owned(),
                |entry| format!("Some(({}, {}))", entry.index, render_record_count(&entry.count)),
            ),
        ).expect("writing to String cannot fail");
    }
    output.push_str("];\n");
    fs::write(output_directory.join("dat_text.rs"), output)
        .map_err(|error| format!("failed to write DAT inspection layout: {error}"))?;

    let resource = layouts
        .iter()
        .find(|layout| layout.id == "mhfinf")
        .ok_or("missing mhfinf resource layout")?;
    let ResourceBody::Quest(layout) = &resource.body else {
        return Err("mhfinf must contain a quest directory".into());
    };
    let output = format!(
        "// Generated from shared/resource/resources/layout.json.\n\
         const QUEST_LAYOUT: mhf_resource::inf::QuestLayout = mhf_resource::inf::QuestLayout {{\n\
             root_field: {}, count_root_field: {}, category_stride: {},\n\
             category_count_field: {}, category_records_field: {},\n\
             record_text_field: {}, record_id_field: {}, parts: {},\n\
         }};\n",
        layout.root_field,
        layout.count_root_field,
        layout.category_stride,
        layout.category_count_field,
        layout.category_records_field,
        layout.record_text_field,
        layout.record_id_field,
        layout.parts,
    );
    fs::write(output_directory.join("inf_layout.rs"), output)
        .map_err(|error| format!("failed to write INF inspection layout: {error}"))
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
        } => (
            id,
            identity,
            code_page,
            runtime,
            ResourceBody::Quest(layout),
        ),
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
    layout: &QuestTableDefinition,
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
        || !layout.root_field.is_multiple_of(4)
        || !layout.count_root_field.is_multiple_of(4)
        || layout.root_field == layout.count_root_field
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

pub(super) fn valid_identifier(id: &str) -> bool {
    id.len() <= 64
        && id
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        && id.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'_' | b'-' | b'.')
        })
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

pub(super) fn read_utf8(path: &Path) -> Result<String, String> {
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
