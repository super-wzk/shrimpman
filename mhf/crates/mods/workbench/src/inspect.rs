//! Owned inspection documents for the I/O worker and UI. Archive entries refer
//! to ranges in shared buffers; only decoded envelopes allocate another buffer.

use std::{fmt, ops::Range, path::Path, sync::Arc};

use crate::metadata::{self, Metadata};

pub use crate::field::Field;
use crate::field::{
    Binding, Endian, FieldType, FieldValue, IntoFieldValue, ScalarType, TextEncoding, formatted,
    typed,
};

#[cfg(test)]
mod archive_tests;
mod dat;
mod legacy_stage;
mod mha;
#[cfg(test)]
mod motion_tests;
mod stage;
mod stage_camera;
mod stage_objects;

use mhf_resource::{
    Decoded,
    binary::{BinaryValue, Reader},
    container::{MhaArchive, SimpleArchive, StageArchive},
    crypto::{Ecd, Exf},
    effect_archive::{
        EffectArchive, EffectBank, EffectResource, MotionEvent, MotionEvents, MotionLookup,
    },
    event_camera::EventCamera,
    fmod::{self, Block, Component, Fmod, MaterialEntry, ObjectEntry, Section, TextureEntry},
    fskl::{Fskl, NodeEntry},
    jkr::Jkr,
    material::GroupedMaterials,
    motion::{Motion, MotionArchive, ObservedMotionDirectory},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Unknown,
    Empty,
    Ecd,
    Exf,
    Jkr,
    Archive,
    Momo,
    Mha,
    Dat,
    DatTable(usize),
    DatRecord(usize),
    Stage,
    StageLighting,
    LegacyStageLighting,
    StageRenderTables,
    LegacyStageRenderTables,
    StageAreaCamera,
    StagePlacements,
    StageObjectPackage,
    StageObjectTables,
    StageObjectWords,
    StageResourceReference,
    Hits,
    KeyEffects,
    Txb,
    Fmod,
    Fskl,
    MotionArchive,
    GroupedMaterials,
    EffectArchive,
    EffectBank,
    EffectMotionEvents,
    Motion,
    EventCamera,
    Text,
    Track,
    Channel,
    Block,
    Object,
    Material,
    Texture,
    Bone,
    Png,
    Dds,
    Ogg,
}

impl Kind {
    pub const fn is_transparent(self) -> bool {
        matches!(
            self,
            Self::Ecd | Self::Exf | Self::Jkr | Self::StageResourceReference
        )
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Unknown => "未识别资源",
            Self::Empty => "空项",
            Self::Ecd => "ECD 加密层",
            Self::Exf => "EXF 加密层",
            Self::Jkr => "JKR 压缩层",
            Self::Archive => "偏移目录",
            Self::Momo => "MOMO 容器",
            Self::Mha => "MHA 命名容器",
            Self::Dat => "DAT 游戏数据",
            Self::DatTable(_) => "DAT 数据表",
            Self::DatRecord(_) => "DAT 记录",
            Self::Stage => "场景专用目录",
            Self::StageLighting => "场景光照与后处理",
            Self::LegacyStageLighting => "旧版场景环境参数",
            Self::StageRenderTables => "场景渲染参数表",
            Self::LegacyStageRenderTables => "旧版场景渲染参数表",
            Self::StageAreaCamera => "场景区域相机",
            Self::StagePlacements => "场景物件实例表",
            Self::StageObjectPackage => "场景对象包",
            Self::StageObjectTables => "场景对象控制表",
            Self::StageObjectWords => "场景附加字表",
            Self::StageResourceReference => "场景资源引用",
            Self::Hits => "HITS 碰撞网格",
            Self::KeyEffects => "KEFFECT 关键帧特效",
            Self::Txb => "TXB 贴图目录",
            Self::Fmod => "FMOD 模型",
            Self::Fskl => "FSKL 骨架",
            Self::MotionArchive => "MOT 动画目录",
            Self::GroupedMaterials => "材质参数组",
            Self::EffectArchive => "特效资源目录",
            Self::EffectBank => "特效库",
            Self::EffectMotionEvents => "动作特效事件",
            Self::Motion => "动画",
            Self::EventCamera => "事件相机动画",
            Self::Text => "文本",
            Self::Track => "动画轨道",
            Self::Channel => "动画通道",
            Self::Block => "数据块",
            Self::Object => "模型对象",
            Self::Material => "材质",
            Self::Texture => "贴图引用",
            Self::Bone => "骨架节点",
            Self::Png => "PNG 图像",
            Self::Dds => "DDS 图像",
            Self::Ogg => "Ogg 音频",
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label())
    }
}

#[derive(Clone, Debug)]
pub struct Node {
    pub name: String,
    pub kind: Kind,
    pub buffer: usize,
    pub range: Range<usize>,
    pub fields: Vec<Field>,
    pub metadata: Metadata,
    /// Ordinary children are appended after their parent. A validated stage
    /// reference instead points to its original target member, which may have
    /// an earlier index or appear elsewhere in this acyclic resource graph.
    pub children: Vec<usize>,
    /// A validated resource's field details can be expanded on request.
    pub deferred: bool,
    /// Invalid data and allocation failures stay visible.
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Document {
    pub buffers: Vec<Arc<[u8]>>,
    pub nodes: Vec<Node>,
    pub root: usize,
}

impl Document {
    pub fn bytes(&self, node_index: usize) -> Option<&[u8]> {
        let node = self.nodes.get(node_index)?;
        self.buffers.get(node.buffer)?.get(node.range.clone())
    }

    /// Follow validated resource references and decoded wrappers without
    /// changing source identity. Broken or cyclic chains have no payload.
    pub fn payload(&self, mut index: usize) -> Option<usize> {
        // A valid chain cannot visit more nodes than the document contains.
        for _ in 0..self.nodes.len() {
            let node = self.nodes.get(index)?;
            if !node.kind.is_transparent() {
                return Some(index);
            }
            if node.children.len() != 1 {
                return None;
            }
            index = node.children[0];
        }
        None
    }
}

pub fn inspect(name: &str, source: Arc<[u8]>) -> Document {
    let mut builder = Builder {
        document: Document {
            nodes: vec![Node {
                name: name.into(),
                kind: Kind::Unknown,
                buffer: 0,
                range: 0..source.len(),
                fields: Vec::new(),
                metadata: Metadata::default(),
                children: Vec::new(),
                deferred: false,
                error: None,
            }],
            buffers: vec![source],
            root: 0,
        },
        parents: vec![None],
        work: Vec::new(),
    };
    if let Some(value) = metadata::from_filename(name) {
        builder.document.nodes[0].metadata.insert(value);
    }
    builder.inspect_node(0, Hint::from_path(name));
    builder.finish();
    builder.document
}

/// Expand one validated resource without re-reading the file or replacing its
/// existing node indices. Containers are already complete; only the selected
/// resource's field details are added here.
pub fn expand(document: &Document, node: usize) -> Result<Document, String> {
    let current = document.nodes.get(node).ok_or("资源节点不存在")?;
    if !current.deferred {
        return Ok(document.clone());
    }
    let buffer = document.buffers[current.buffer].clone();
    let range = current.range.clone();
    let bytes = &buffer[range.clone()];
    let kind = current.kind;
    let mut parents = vec![None; document.nodes.len()];
    for (parent, node) in document.nodes.iter().enumerate() {
        if node.kind != Kind::StageResourceReference {
            for &child in &node.children {
                parents[child] = Some(parent);
            }
        }
    }
    let mut builder = Builder {
        document: document.clone(),
        parents,
        work: Vec::new(),
    };
    builder.document.nodes[node].deferred = false;
    match kind {
        Kind::Mha => {
            let archive =
                MhaArchive::parse(bytes, bytes.len()).map_err(|error| error.to_string())?;
            builder.mha_id_details(node, &archive, range.start)?;
        }
        Kind::DatTable(index) => builder.dat_table_records(node, index)?,
        Kind::DatRecord(index) => builder.dat_record_fields(node, index)?,
        Kind::Motion => {
            let motion = Motion::parse(bytes).map_err(|error| error.to_string())?;
            builder.motion_tracks(node, &motion, range.start);
        }
        Kind::EventCamera => {
            let camera = EventCamera::parse(bytes).map_err(|error| error.to_string())?;
            builder.event_camera_frames(node, &camera, range.start);
        }
        Kind::StageLighting => {
            let file =
                mhf_resource::stage::Lighting::parse(bytes).map_err(|error| error.to_string())?;
            builder.stage_lighting_details(node, &file, range.start);
        }
        Kind::LegacyStageLighting => {
            let file = mhf_resource::stage::LegacyLighting::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.legacy_stage_lighting_details(node, &file, range.start);
        }
        Kind::LegacyStageRenderTables => {
            let file = mhf_resource::stage::LegacyRenderTables::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.legacy_stage_render_details(node, &file, range.start);
        }
        Kind::StageAreaCamera => {
            let file =
                mhf_resource::stage::AreaCamera::parse(bytes).map_err(|error| error.to_string())?;
            builder.stage_area_camera_details(node, &file, range.start);
        }
        Kind::StageRenderTables => {
            let file = mhf_resource::stage::RenderTables::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.stage_render_details(node, &file, range.start);
        }
        Kind::StagePlacements => {
            let file = mhf_resource::stage::PlacementTable::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.stage_placements(node, &file, range.start);
        }
        Kind::StageObjectTables => {
            let file = mhf_resource::stage::ObjectTables::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.stage_object_table_details(node, &file, range.start);
        }
        Kind::StageObjectWords => {
            let file = mhf_resource::stage::ObjectWordTable::parse(bytes)
                .map_err(|error| error.to_string())?;
            builder.stage_object_word_details(node, &file, range.start);
        }
        Kind::Hits => {
            let file =
                mhf_resource::stage::Hits::parse(bytes).map_err(|error| error.to_string())?;
            builder.hits_details(node, &file, range.start);
        }
        Kind::KeyEffects => {
            let file =
                mhf_resource::stage::KEffect::parse(bytes).map_err(|error| error.to_string())?;
            builder.key_effects(node, &file, range.start);
        }
        Kind::EffectBank => {
            let bank = EffectBank::parse(bytes).map_err(|error| error.to_string())?;
            builder.effect_bank_details(node, &bank, range.start);
        }
        Kind::EffectMotionEvents => {
            let events = MotionEvents::parse(bytes).map_err(|error| error.to_string())?;
            if let Some(lookup) = &events.lookup {
                builder.effect_lookup(node, lookup, range.start);
            }
            builder.effect_events(node, &events.events, range.start);
        }
        _ => return Err("此资源没有可展开的明细解析器".into()),
    }
    // Details retain the already resolved resource graph and declarations.
    // Only newly scheduled resource inspection needs the full completion pass.
    if !builder.work.is_empty() {
        builder.finish();
    }
    Ok(builder.document)
}

#[derive(Clone, Copy, Debug, Default)]
struct Hint {
    directory: bool,
    txb: bool,
    fmod: bool,
    fskl: bool,
    motion: bool,
    material: bool,
    effect_archive: bool,
    stage_render_tables: bool,
    legacy_render_tables: bool,
    object_tables: bool,
    object_words: bool,
}

impl Hint {
    fn from_path(name: &str) -> Self {
        let path = name.replace('\\', "/").to_ascii_lowercase();
        let file = Path::new(&path);
        let extension = file
            .extension()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        Self {
            directory: matches!(
                extension,
                "bin" | "pac" | "txb" | "gab" | "snp" | "snd" | "abn"
            ),
            txb: extension == "txb",
            fmod: extension == "fmod",
            fskl: extension == "fskl",
            motion: extension == "mot",
            ..Self::default()
        }
    }
}

struct Builder {
    document: Document,
    parents: Vec<Option<usize>>,
    work: Vec<InspectionTask>,
}

enum InspectionTask {
    Inspect { node: usize, hint: Hint },
    CompleteArchive { node: usize, count: usize },
    CheckObjectMember { node: usize, kind: u8 },
}

impl Builder {
    fn finish(&mut self) {
        self.run();
        self.resolve_stage_references();
        self.inspect_legacy_stage_contexts();
        self.run();
        for (scope, value) in metadata::model_resources(&self.document) {
            if let Some(node) = self.document.nodes.get_mut(scope) {
                node.metadata.insert(value);
            }
        }
    }

    fn run(&mut self) {
        while let Some(task) = self.work.pop() {
            let previous = self.work.len();
            match task {
                InspectionTask::Inspect { node, hint } => {
                    if self.has_ancestor_content(node) {
                        self.fail(node, "资源内容重新指向其祖先，不能形成解包循环".into());
                        continue;
                    }
                    let hint = self.context_hint(node, hint);
                    self.inspect_node_contents(node, hint);
                }
                InspectionTask::CompleteArchive { node, count } => {
                    let children = &self.document.nodes[node].children;
                    if self.document.nodes[node].kind == Kind::Archive
                        && children.len() == count
                        && children.iter().any(|&index| {
                            matches!(
                                self.document.nodes[self.payload(index)].kind,
                                Kind::Png | Kind::Dds
                            )
                        })
                        && children.iter().all(|&index| {
                            let child = &self.document.nodes[self.payload(index)];
                            child.range.is_empty() || matches!(child.kind, Kind::Png | Kind::Dds)
                        })
                    {
                        self.document.nodes[node].kind = Kind::Txb;
                    }
                }
                InspectionTask::CheckObjectMember { node, kind } => {
                    self.check_stage_member(node, kind)
                }
            }
            // Process siblings in source order, including each complete child
            // subtree, without growing the Rust call stack.
            self.work[previous..].reverse();
        }
    }

    fn has_ancestor_content(&self, node: usize) -> bool {
        let Some(bytes) = self.document.bytes(node) else {
            return false;
        };
        let mut ancestor = self.parents[node];
        while let Some(index) = ancestor {
            let current = &self.document.nodes[index];
            if current.range.len() == bytes.len() && self.document.bytes(index) == Some(bytes) {
                return true;
            }
            ancestor = self.parents[index];
        }
        false
    }

    fn context_hint(&self, node: usize, mut hint: Hint) -> Hint {
        let Some(parent) = self.parents[node] else {
            return hint;
        };
        let owner = &self.document.nodes[parent];
        if !matches!(owner.kind, Kind::Archive | Kind::Momo | Kind::Txb) {
            return hint;
        }
        let Some(index) = owner.children.iter().position(|&child| child == node) else {
            return hint;
        };
        hint.stage_render_tables |= index == 2 && self.has_stage_lighting_prefix(parent);
        hint.txb |= index == 3 && self.has_stage_lighting_prefix(parent);
        hint.motion |= index == 2 && self.has_model_package_prefix(parent);
        hint.material |= index == 6 && self.has_model_package_prefix(parent);
        hint.effect_archive |= index == 5 && self.has_model_package_prefix(parent);
        hint
    }

    fn fail(&mut self, node: usize, error: String) {
        let slot = &mut self.document.nodes[node].error;
        if let Some(previous) = slot {
            if !previous.contains(&error) {
                previous.push('；');
                previous.push_str(&error);
            }
        } else {
            *slot = Some(error);
        }
    }

    fn field(
        &mut self,
        node: usize,
        name: impl Into<String>,
        value: impl IntoFieldValue,
        offset: usize,
        size: usize,
    ) {
        let value = value.into_field_value(size);
        let current = &mut self.document.nodes[node];
        current.fields.push(Field {
            name: name.into(),
            value: value.display,
            writable: size != 0 && value.edit != FieldType::ReadOnly,
            binding: Binding {
                buffer: current.buffer,
                range: offset..offset + size,
                endian: Endian::Little,
                format: if size == 0 {
                    FieldType::ReadOnly
                } else {
                    value.edit
                },
            },
        });
    }

    /// Parse a value directly from its backing buffer. The shared codec owns
    /// width, byte order and field range; callers supply only the source offset.
    fn read<T: BinaryValue + fmt::Debug>(
        &mut self,
        node: usize,
        name: impl Into<String>,
        offset: usize,
    ) -> Result<usize, String> {
        self.read_endian::<T>(node, name, offset, Endian::Little)
    }

    fn read_scalar(
        &mut self,
        node: usize,
        name: impl Into<String>,
        offset: usize,
        scalar: ScalarType,
    ) -> Result<usize, String> {
        match scalar {
            ScalarType::U8 => self.read::<u8>(node, name, offset),
            ScalarType::U16 => self.read::<u16>(node, name, offset),
            ScalarType::U32 => self.read::<u32>(node, name, offset),
            ScalarType::U64 => self.read::<u64>(node, name, offset),
            ScalarType::I8 => self.read::<i8>(node, name, offset),
            ScalarType::I16 => self.read::<i16>(node, name, offset),
            ScalarType::I32 => self.read::<i32>(node, name, offset),
            ScalarType::I64 => self.read::<i64>(node, name, offset),
            ScalarType::F32 => self.read::<f32>(node, name, offset),
            ScalarType::F64 => self.read::<f64>(node, name, offset),
        }
    }

    fn read_endian<T: BinaryValue + fmt::Debug>(
        &mut self,
        node: usize,
        name: impl Into<String>,
        offset: usize,
        endian: Endian,
    ) -> Result<usize, String> {
        let buffer = self.document.nodes[node].buffer;
        let source = Reader::new(&self.document.buffers[buffer])
            .with_endian(endian)
            .read_at::<T>(offset)
            .map_err(|error| error.to_string())?;
        let field = Field::from_binary(name, buffer, source);
        let fields = &mut self.document.nodes[node].fields;
        let index = fields.len();
        fields.push(field);
        Ok(index)
    }

    fn read_as<T: BinaryValue + fmt::Debug>(
        &mut self,
        node: usize,
        name: impl Into<String>,
        offset: usize,
        format: FieldType,
    ) -> Result<(), String> {
        let index = self.read::<T>(node, name, offset)?;
        let field = &mut self.document.nodes[node].fields[index];
        field.binding.format = format;
        field.value = field.read(&self.document.buffers)?;
        Ok(())
    }

    fn child(
        &mut self,
        parent: usize,
        name: impl Into<String>,
        kind: Kind,
        buffer: usize,
        range: Range<usize>,
    ) -> Option<usize> {
        if self
            .document
            .buffers
            .get(buffer)
            .and_then(|bytes| bytes.get(range.clone()))
            .is_none()
        {
            self.fail(parent, "子资源范围超出原始数据层".into());
            return None;
        }
        if self.document.nodes.try_reserve(1).is_err()
            || self.document.nodes[parent].children.try_reserve(1).is_err()
            || self.parents.try_reserve(1).is_err()
        {
            self.fail(parent, "无法分配资源目录节点".into());
            return None;
        }
        let node = self.document.nodes.len();
        self.document.nodes.push(Node {
            name: name.into(),
            kind,
            buffer,
            range,
            fields: Vec::new(),
            metadata: Metadata::default(),
            children: Vec::new(),
            deferred: false,
            error: None,
        });
        self.parents.push(Some(parent));
        self.document.nodes[parent].children.push(node);
        Some(node)
    }

    fn payload(&self, node: usize) -> usize {
        self.document.payload(node).unwrap_or(node)
    }

    fn has_model_package_prefix(&self, node: usize) -> bool {
        let children = &self.document.nodes[node].children;
        if children.len() < 2 {
            return false;
        }
        let geometry = &self.document.nodes[self.payload(children[0])];
        let texture = &self.document.nodes[self.payload(children[1])];
        if !matches!(geometry.kind, Kind::Archive | Kind::Momo | Kind::Mha)
            || geometry.children.len() != 2
        {
            return false;
        }
        self.document.nodes[self.payload(geometry.children[0])].kind == Kind::Fmod
            && self.document.nodes[self.payload(geometry.children[1])].kind == Kind::Fskl
            && (texture.kind == Kind::Txb
                || matches!(texture.kind, Kind::Archive | Kind::Momo)
                    && !texture.children.is_empty()
                    && texture.children.iter().all(|&index| {
                        matches!(
                            self.document.nodes[self.payload(index)].kind,
                            Kind::Png | Kind::Dds | Kind::Empty
                        )
                    }))
    }

    fn inspect_node(&mut self, node: usize, hint: Hint) {
        self.work.push(InspectionTask::Inspect { node, hint });
    }

    fn inspect_node_contents(&mut self, node: usize, hint: Hint) {
        let current = &self.document.nodes[node];
        let buffer_index = current.buffer;
        let range = current.range.clone();
        let buffer = self.document.buffers[buffer_index].clone();
        let bytes = &buffer[range.clone()];
        let base = range.start;
        if bytes.is_empty() {
            self.document.nodes[node].kind = Kind::Empty;
            return;
        }
        // The reference signature begins with the UTF-16LE BOM bytes, but its
        // complete 16-byte signature identifies a native resource link.
        if mhf_resource::stage::ResourceReference::has_magic(bytes) {
            self.stage_reference(node, bytes, base);
            return;
        }
        if !hint.object_words
            && bytes.len().is_multiple_of(2)
            && let Some(little_endian) = match bytes.get(..2) {
                Some([0xff, 0xfe]) => Some(true),
                Some([0xfe, 0xff]) => Some(false),
                _ => None,
            }
        {
            let format = FieldType::Text {
                encoding: if little_endian {
                    TextEncoding::Utf16Le
                } else {
                    TextEncoding::Utf16Be
                },
                terminated: false,
            };
            if let Ok(text) = format.decode(&bytes[2..]) {
                self.document.nodes[node].kind = Kind::Text;
                self.field(
                    node,
                    "编码",
                    if little_endian {
                        "UTF-16LE"
                    } else {
                        "UTF-16BE"
                    },
                    base,
                    2,
                );
                self.field(
                    node,
                    "文本内容",
                    typed(format!("{text:?}"), format),
                    base + 2,
                    bytes.len() - 2,
                );
                return;
            }
        }
        // Only the start of a bounded resource is inspected. No magic scanning.
        if bytes.starts_with(b"ecd\x1a") {
            self.document.nodes[node].kind = Kind::Ecd;
            let result = Ecd::parse(bytes).and_then(|file| {
                self.field(node, "key_index", file.header.key_index, base + 4, 2);
                self.field(
                    node,
                    "filename_checksum",
                    formatted(
                        file.header.filename_checksum,
                        format!("{:04X}", file.header.filename_checksum),
                    ),
                    base + 6,
                    2,
                );
                self.field(node, "payload_size", file.header.payload_size, base + 8, 4);
                self.field(
                    node,
                    "CRC32",
                    formatted(file.header.crc32, format!("{:08X}", file.header.crc32)),
                    base + 12,
                    4,
                );
                let size = file.header.payload_size as usize;
                file.decode(size)
            });
            self.decoded(node, result, hint);
            return;
        }
        if bytes.starts_with(b"exf\x1a") {
            self.document.nodes[node].kind = Kind::Exf;
            let result = Exf::parse(bytes).and_then(|file| {
                self.field(node, "key_index", file.header.key_index, base + 4, 2);
                self.field(
                    node,
                    "filename_checksum",
                    formatted(
                        file.header.filename_checksum,
                        format!("{:04X}", file.header.filename_checksum),
                    ),
                    base + 6,
                    2,
                );
                self.field(
                    node,
                    "unknown_08",
                    hex(&file.header.unknown_08),
                    base + 8,
                    4,
                );
                self.field(
                    node,
                    "seed",
                    formatted(file.header.seed, format!("{:08X}", file.header.seed)),
                    base + 12,
                    4,
                );
                file.decode(bytes.len())
            });
            self.decoded(node, result, hint);
            return;
        }
        if bytes.starts_with(b"JKR\x1a") {
            self.document.nodes[node].kind = Kind::Jkr;
            let result = Jkr::parse(bytes).and_then(|file| {
                self.field(
                    node,
                    "version",
                    formatted(file.header.version, format!("{:#06X}", file.header.version)),
                    base + 4,
                    2,
                );
                self.field(
                    node,
                    "encoding",
                    format!("{} · {:?}", file.header.encoding, file.encoding().ok()),
                    base + 6,
                    2,
                );
                self.field(
                    node,
                    "data_offset",
                    formatted(
                        file.header.data_offset,
                        format!("{:#X}", file.header.data_offset),
                    ),
                    base + 8,
                    4,
                );
                self.field(node, "decoded_size", file.header.decoded_size, base + 12, 4);
                let size = file.header.decoded_size as usize;
                file.decode(size)
            });
            self.decoded(node, result, hint);
            return;
        }
        if bytes.starts_with(mhf_resource::dat::MAGIC) {
            self.document.nodes[node].kind = Kind::Dat;
            self.inspect_dat(node, bytes, base);
            return;
        }
        if bytes.starts_with(b"mha\x01") {
            self.inspect_mha(node, bytes, base);
            return;
        }
        if hint.legacy_render_tables {
            self.inspect_legacy_stage_render_tables(node, bytes, base);
            return;
        }
        if hint.object_tables {
            self.document.nodes[node].kind = Kind::StageObjectTables;
            match mhf_resource::stage::ObjectTables::parse(bytes) {
                Ok(file) => self.stage_object_tables(node, &file, base),
                Err(error) => self.fail(node, error.to_string()),
            }
            return;
        }
        if hint.object_words {
            self.document.nodes[node].kind = Kind::StageObjectWords;
            match mhf_resource::stage::ObjectWordTable::parse(bytes) {
                Ok(file) => self.stage_object_words(node, &file, base),
                Err(error) => self.fail(node, error.to_string()),
            }
            return;
        }
        if let Ok(package) = mhf_resource::stage::ObjectPackage::probe(bytes, bytes.len()) {
            self.document.nodes[node].kind = Kind::StageObjectPackage;
            self.stage_objects(node, &package, base);
            return;
        }
        // A model/texture prefix also occurs in collections such as effect.bin.
        // Positional hints are candidates; a failed candidate must not prevent
        // validation of the member's actual directory, model, or texture data.
        // Keep its diagnostic if no other format can account for the bytes.
        let mut hinted_error = None;
        if self.inspect_stage(node, bytes, base, hint.stage_render_tables) {
            return;
        }
        if hint.effect_archive {
            match EffectArchive::parse(bytes, bytes.len()) {
                Ok(file) => {
                    self.document.nodes[node].kind = Kind::EffectArchive;
                    self.effects(node, &file, base);
                    return;
                }
                Err(error) => hinted_error = Some((Kind::EffectArchive, error.to_string())),
            }
        }
        if hint.material {
            match GroupedMaterials::parse(bytes) {
                Ok(file) => {
                    self.document.nodes[node].kind = Kind::GroupedMaterials;
                    self.grouped_materials(node, &file, base);
                    return;
                }
                Err(error) => hinted_error = Some((Kind::GroupedMaterials, error.to_string())),
            }
        }
        if hint.fskl || bytes.starts_with(&0xc000_0000u32.to_le_bytes()) {
            self.document.nodes[node].kind = Kind::Fskl;
            match Fskl::parse(bytes) {
                Ok(file) => self.skeleton(node, &file, base),
                Err(error) => self.fail(node, error.to_string()),
            }
            return;
        }
        if hint.fmod || bytes.starts_with(&1u32.to_le_bytes()) {
            match Fmod::parse(bytes) {
                Ok(file) => {
                    self.document.nodes[node].kind = Kind::Fmod;
                    self.model(node, &file, base);
                    return;
                }
                Err(error) if hint.fmod => {
                    self.document.nodes[node].kind = Kind::Fmod;
                    self.fail(node, error.to_string());
                    return;
                }
                Err(_) => {} // A one-entry offset directory has the same first word.
            }
        }
        if let Ok(camera) = EventCamera::probe(bytes) {
            self.document.nodes[node].kind = Kind::EventCamera;
            self.event_camera(node, &camera, base);
            return;
        }
        let motion = if hint.motion {
            Motion::parse(bytes)
        } else {
            Motion::probe(bytes)
        };
        if let Ok(motion) = motion {
            self.document.nodes[node].kind = Kind::Motion;
            self.motion_summary(node, &motion, base);
            self.motion_tracks(node, &motion, base);
            return;
        }
        if hint.motion {
            match ObservedMotionDirectory::probe_with_budget(bytes, usize::MAX) {
                Ok(observed) => {
                    self.document.nodes[node].kind = Kind::MotionArchive;
                    self.field(
                        node,
                        "目录记录数（结构识别）",
                        observed.record_count(),
                        base,
                        observed.record_count() * 8,
                    );
                    self.field(
                        node,
                        "原生消费组数",
                        "未存储在该文件中，由调用方指定",
                        base,
                        0,
                    );
                    self.motion_archive(node, &observed.directory, base);
                    return;
                }
                Err(error) => {
                    hinted_error = Some((
                        Kind::Unknown,
                        format!("MOT 目录记录无法完整验证：{error}；文件没有原生消费组数字段"),
                    ));
                }
            }
        }
        if bytes.starts_with(b"MOMO") || hint.directory {
            match SimpleArchive::parse(bytes, bytes.len()) {
                Ok(archive) if archive.count != 0 || bytes.starts_with(b"MOMO") || hint.txb => {
                    // 113D5910 and 113D5A90 use the same complete descriptor
                    // directory. It can occur outside monster-package slot 5.
                    // Exact descriptor coverage and a known kind distinguish
                    // it from an ordinary offset directory; individual payload
                    // errors and unknown descriptor kinds remain visible.
                    if let Ok(effects) = EffectArchive::parse(bytes, bytes.len())
                        && effects.index.trailing_bytes.is_empty()
                        && effects
                            .members
                            .iter()
                            .any(|member| matches!(member.reference.kind, 1 | 2))
                    {
                        self.document.nodes[node].kind = Kind::EffectArchive;
                        self.effects(node, &effects, base);
                        return;
                    }
                    self.document.nodes[node].kind = if bytes.starts_with(b"MOMO") {
                        Kind::Momo
                    } else if hint.txb {
                        Kind::Txb
                    } else {
                        Kind::Archive
                    };
                    self.field(
                        node,
                        "count",
                        archive.count,
                        base + archive.table_offset - 4,
                        4,
                    );
                    for entry in &archive.entries {
                        let at = if entry.size == 0 {
                            base
                        } else {
                            base + entry.offset as usize
                        };
                        let Some(child) = self.child(
                            node,
                            format!("{:04} · {:#X}", entry.index, entry.offset),
                            Kind::Unknown,
                            buffer_index,
                            at..at + entry.size as usize,
                        ) else {
                            break;
                        };
                        let meta = base + archive.table_offset + entry.index * 8;
                        self.field(
                            child,
                            "offset",
                            formatted(entry.offset, format!("{:#X}", entry.offset)),
                            meta,
                            4,
                        );
                        self.field(child, "size", entry.size, meta + 4, 4);
                        self.inspect_node(
                            child,
                            Hint {
                                directory: true,
                                ..Hint::default()
                            },
                        );
                    }
                    self.work.push(InspectionTask::CompleteArchive {
                        node,
                        count: archive.entries.len(),
                    });
                    return;
                }
                Err(error) if bytes.starts_with(b"MOMO") => {
                    self.document.nodes[node].kind = Kind::Momo;
                    self.fail(node, error.to_string());
                    return;
                }
                _ => {}
            }
        }
        if let Ok(archive) = StageArchive::probe(bytes, bytes.len()) {
            self.document.nodes[node].kind = Kind::Stage;
            self.field(
                node,
                "additional_count",
                archive.additional_count,
                base + 24,
                4,
            );
            for item in &archive.entries {
                let entry = item.entry;
                let at = if entry.size == 0 {
                    base
                } else {
                    base + entry.offset as usize
                };
                let Some(child) = self.child(
                    node,
                    format!("{:04} · {:#X}", entry.index, entry.offset),
                    Kind::Unknown,
                    buffer_index,
                    at..at + entry.size as usize,
                ) else {
                    break;
                };
                let meta = base
                    + if entry.index < 3 {
                        entry.index * 8
                    } else {
                        28 + (entry.index - 3) * 12
                    };
                let location = if let Some(resource_id) = item.resource_id {
                    self.field(child, "resource_id", resource_id, meta, 4);
                    meta + 4
                } else {
                    meta
                };
                self.field(
                    child,
                    "offset",
                    formatted(entry.offset, format!("{:#X}", entry.offset)),
                    location,
                    4,
                );
                self.field(child, "size", entry.size, location + 4, 4);
                self.inspect_node(
                    child,
                    Hint {
                        directory: true,
                        legacy_render_tables: entry.index == 2,
                        ..Hint::default()
                    },
                );
            }
            return;
        }
        if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            self.document.nodes[node].kind = Kind::Png;
            match mhf_resource::png::Png::parse(bytes) {
                Ok(file) => {
                    let first_field = self.document.nodes[node].fields.len();
                    for (name, offset) in [("width", 16), ("height", 20)] {
                        if let Err(error) =
                            self.read_endian::<u32>(node, name, base + offset, Endian::Big)
                        {
                            self.fail(node, error);
                        }
                    }
                    self.field(node, "bit_depth", file.header.bit_depth, base + 24, 1);
                    self.field(node, "color_type", file.header.color_type, base + 25, 1);
                    self.field(
                        node,
                        "compression_method",
                        file.header.compression_method,
                        base + 26,
                        1,
                    );
                    self.field(
                        node,
                        "filter_method",
                        file.header.filter_method,
                        base + 27,
                        1,
                    );
                    self.field(
                        node,
                        "interlace_method",
                        file.header.interlace_method,
                        base + 28,
                        1,
                    );
                    // Image properties cannot be changed independently of
                    // chunk checksums and encoded pixels. Replace the image.
                    for field in &mut self.document.nodes[node].fields[first_field..] {
                        field.writable = false;
                    }
                    if let Err(error) = file.validate() {
                        self.fail(node, error.to_string());
                    }
                    for chunk in &file.chunks {
                        let at = base + chunk.offset;
                        let Some(child) = self.child(
                            node,
                            String::from_utf8_lossy(&chunk.kind).into_owned(),
                            Kind::Block,
                            buffer_index,
                            at..at + chunk.source.len(),
                        ) else {
                            break;
                        };
                        if let Err(error) =
                            self.read_endian::<u32>(child, "length", at, Endian::Big)
                        {
                            self.fail(child, error);
                        }
                        self.field(child, "kind", hex(&chunk.kind), at + 4, 4);
                        if let Err(error) = self.read_endian::<u32>(
                            child,
                            "CRC32",
                            at + 8 + chunk.data.len(),
                            Endian::Big,
                        ) {
                            self.fail(child, error);
                        }
                        for field in &mut self.document.nodes[child].fields {
                            field.writable = false;
                        }
                    }
                }
                Err(error) => self.fail(node, error.to_string()),
            }
        } else if bytes.starts_with(b"DDS ") {
            self.document.nodes[node].kind = Kind::Dds;
            match mhf_resource::dds::Dds::parse(bytes) {
                Ok(file) => {
                    let header = file.header;
                    for (name, value, offset) in [
                        ("size", header.size, 4),
                        ("flags", header.flags, 8),
                        ("height", header.height, 12),
                        ("width", header.width, 16),
                        ("pitch_or_linear_size", header.pitch_or_linear_size, 20),
                        ("depth", header.depth, 24),
                        ("mip_map_count", header.mip_map_count, 28),
                        ("pixel_format.size", header.pixel_format.size, 76),
                        ("pixel_format.flags", header.pixel_format.flags, 80),
                        ("rgb_bit_count", header.pixel_format.rgb_bit_count, 88),
                        ("r_bit_mask", header.pixel_format.r_bit_mask, 92),
                        ("g_bit_mask", header.pixel_format.g_bit_mask, 96),
                        ("b_bit_mask", header.pixel_format.b_bit_mask, 100),
                        ("a_bit_mask", header.pixel_format.a_bit_mask, 104),
                        ("caps", header.caps, 108),
                        ("caps_2", header.caps_2, 112),
                        ("caps_3", header.caps_3, 116),
                        ("caps_4", header.caps_4, 120),
                        ("reserved_2", header.reserved_2, 124),
                    ] {
                        self.field(
                            node,
                            name,
                            formatted(value, format!("{value} ({value:#X})")),
                            base + offset,
                            4,
                        );
                    }
                    self.field(
                        node,
                        "reserved_1",
                        typed(
                            summary(&header.reserved_1),
                            FieldType::Array(ScalarType::U32),
                        ),
                        base + 32,
                        44,
                    );
                    self.field(
                        node,
                        "four_cc",
                        hex(&header.pixel_format.four_cc),
                        base + 84,
                        4,
                    );
                    if let Some(dx10) = file.dx10 {
                        for (name, value, offset) in [
                            ("dxgi_format", dx10.dxgi_format, 128),
                            ("resource_dimension", dx10.resource_dimension, 132),
                            ("misc_flag", dx10.misc_flag, 136),
                            ("array_size", dx10.array_size, 140),
                            ("misc_flags_2", dx10.misc_flags_2, 144),
                        ] {
                            self.field(node, name, value, base + offset, 4);
                        }
                    }
                    match file.surfaces(bytes.len()) {
                        Ok(surfaces) => {
                            for surface in surfaces {
                                let at = base + surface.offset;
                                let Some(child) = self.child(
                                    node,
                                    format!(
                                        "数组 {} · {:?} · Mip {} · {}×{}×{}",
                                        surface.array_index,
                                        surface.cube_face,
                                        surface.mip_level,
                                        surface.width,
                                        surface.height,
                                        surface.depth
                                    ),
                                    Kind::Block,
                                    buffer_index,
                                    at..at + surface.bytes.len(),
                                ) else {
                                    break;
                                };
                                self.field(
                                    child,
                                    "存储布局",
                                    format!(
                                        "{:?}；行距 {}，切片距 {}",
                                        file.encoding(),
                                        surface.row_pitch,
                                        surface.slice_pitch
                                    ),
                                    at,
                                    surface.bytes.len(),
                                );
                            }
                        }
                        Err(error) => self.fail(node, error.to_string()),
                    }
                }
                Err(error) => self.fail(node, error.to_string()),
            }
        } else if bytes.starts_with(b"OggS") {
            self.document.nodes[node].kind = Kind::Ogg;
        }
        if self.document.nodes[node].kind == Kind::Unknown
            && let Some((kind, error)) = hinted_error
        {
            self.document.nodes[node].kind = kind;
            self.fail(node, error);
        }
    }

    fn decoded<H>(
        &mut self,
        parent: usize,
        result: mhf_resource::Result<Decoded<H, Box<[u8]>>>,
        hint: Hint,
    ) {
        match result {
            Ok(decoded) => {
                let bytes: Arc<[u8]> = decoded.into_inner().into();
                let size = bytes.len();
                let buffer = self.document.buffers.len();
                if self.document.buffers.try_reserve(1).is_err() {
                    self.fail(parent, "无法保留解码数据层".into());
                    return;
                }
                self.document.buffers.push(bytes);
                if let Some(child) = self.child(parent, "解码内容", Kind::Unknown, buffer, 0..size)
                {
                    self.inspect_node(child, hint);
                }
            }
            Err(error) => self.fail(parent, error.to_string()),
        }
    }

    fn block_fields(&mut self, node: usize, block: Block<'_>, base: usize) {
        let at = base + block.offset();
        self.field(
            node,
            "kind",
            formatted(block.header.kind, format!("{:#010X}", block.header.kind)),
            at,
            4,
        );
        self.field(node, "count", block.header.count, at + 4, 4);
        self.field(node, "size", block.header.size, at + 8, 4);
    }

    fn block_child(
        &mut self,
        parent: usize,
        name: impl Into<String>,
        kind: Kind,
        block: Block<'_>,
        base: usize,
    ) -> Option<usize> {
        let at = base + block.offset();
        let child = self.child(
            parent,
            name,
            kind,
            self.document.nodes[parent].buffer,
            at..at + block.as_bytes().len(),
        )?;
        self.block_fields(child, block, base);
        Some(child)
    }

    fn model(&mut self, node: usize, file: &Fmod<'_>, base: usize) {
        self.block_fields(node, file.root, base);
        for section in &file.sections {
            match section {
                Section::Init(value) => {
                    self.block_child(node, "初始化索引", Kind::Block, value.block, base);
                }
                Section::Unknown(block) => {
                    self.block_child(
                        node,
                        format!("未知块 {:#X}", block.header.kind),
                        Kind::Block,
                        *block,
                        base,
                    );
                }
                Section::Meshes(meshes) => {
                    let Some(parent) =
                        self.block_child(node, "模型对象", Kind::Block, meshes.block, base)
                    else {
                        break;
                    };
                    for (index, entry) in meshes.entries.iter().enumerate() {
                        match entry {
                            ObjectEntry::Object(object) => {
                                let Some(child) = self.block_child(
                                    parent,
                                    format!("对象 {index}"),
                                    Kind::Object,
                                    object.block,
                                    base,
                                ) else {
                                    break;
                                };
                                if let Err(error) = object.validate_geometry() {
                                    self.fail(child, error.to_string());
                                }
                                for component in &object.components {
                                    self.component(child, component, base);
                                }
                            }
                            ObjectEntry::Unknown(block) => {
                                self.block_child(
                                    parent,
                                    format!("未知对象 {index}"),
                                    Kind::Block,
                                    *block,
                                    base,
                                );
                            }
                        }
                    }
                }
                Section::Materials(table) => {
                    let Some(parent) =
                        self.block_child(node, "材质", Kind::Block, table.block, base)
                    else {
                        break;
                    };
                    for (index, entry) in table.records.iter().enumerate() {
                        match entry {
                            MaterialEntry::Material(material) => {
                                let Some(child) = self.block_child(
                                    parent,
                                    format!("材质 {index}"),
                                    Kind::Material,
                                    material.block,
                                    base,
                                ) else {
                                    break;
                                };
                                let at = base + material.block.offset() + 12;
                                self.field(
                                    child,
                                    "color_00",
                                    typed(
                                        format!("{:?}", material.color_00),
                                        FieldType::Array(ScalarType::F32),
                                    ),
                                    at,
                                    16,
                                );
                                self.field(
                                    child,
                                    "color_10",
                                    typed(
                                        format!("{:?}", material.color_10),
                                        FieldType::Array(ScalarType::F32),
                                    ),
                                    at + 16,
                                    16,
                                );
                                self.field(
                                    child,
                                    "color_20",
                                    typed(
                                        format!("{:?}", material.color_20),
                                        FieldType::Array(ScalarType::F32),
                                    ),
                                    at + 32,
                                    16,
                                );
                                self.field(
                                    child,
                                    "parameter_30",
                                    formatted(
                                        material.parameter_30,
                                        format!(
                                            "{} ({:#010X})",
                                            material.parameter_30,
                                            material.parameter_30.to_bits()
                                        ),
                                    ),
                                    at + 48,
                                    4,
                                );
                                self.field(
                                    child,
                                    "贴图引用数",
                                    material.texture_indices.len(),
                                    at + 52,
                                    4,
                                );
                                self.field(
                                    child,
                                    "texture_indices",
                                    typed(
                                        summary(&material.texture_indices),
                                        FieldType::Array(ScalarType::U32),
                                    ),
                                    at + 256,
                                    material.texture_indices.len() * 4,
                                );
                                self.field(
                                    child,
                                    "unknown_38",
                                    hex(material.unknown_38),
                                    at + 56,
                                    material.unknown_38.len(),
                                );
                            }
                            MaterialEntry::Unknown(block) => {
                                self.block_child(
                                    parent,
                                    format!("未知材质 {index}"),
                                    Kind::Block,
                                    *block,
                                    base,
                                );
                            }
                        }
                    }
                }
                Section::Textures(table) => {
                    let Some(parent) =
                        self.block_child(node, "贴图引用", Kind::Block, table.block, base)
                    else {
                        break;
                    };
                    for (index, entry) in table.records.iter().enumerate() {
                        match entry {
                            TextureEntry::Texture(texture) => {
                                let Some(child) = self.block_child(
                                    parent,
                                    format!("贴图 {index}"),
                                    Kind::Texture,
                                    texture.block,
                                    base,
                                ) else {
                                    break;
                                };
                                let at = base + texture.block.offset() + 12;
                                self.field(child, "image_id", texture.image_id, at, 4);
                                self.field(child, "width", texture.width, at + 4, 4);
                                self.field(child, "height", texture.height, at + 8, 4);
                                self.field(
                                    child,
                                    "unknown_0c",
                                    hex(texture.unknown_0c),
                                    at + 12,
                                    texture.unknown_0c.len(),
                                );
                            }
                            TextureEntry::Unknown(block) => {
                                self.block_child(
                                    parent,
                                    format!("未知贴图 {index}"),
                                    Kind::Block,
                                    *block,
                                    base,
                                );
                            }
                        }
                    }
                }
            }
        }
    }

    fn component(&mut self, parent: usize, component: &Component<'_>, base: usize) {
        let name = match component {
            Component::Faces(_) => "面",
            Component::MaterialList(_) => "材质索引",
            Component::MaterialMap(_) => "材质映射",
            Component::Positions(_) => "顶点位置",
            Component::Normals(_) => "法线",
            Component::Uvs(_) => "UV",
            Component::Colors(_) => "顶点颜色",
            Component::Weights(_) => "蒙皮权重",
            Component::BoneMap(_) => "骨骼映射",
            Component::Attribute12(_) => "属性 0x12",
            Component::Rendering(_) => "渲染参数",
            Component::WordGroups(_) => "分组字数据（0x0E）",
            Component::Unknown(_) => "未知对象数据",
        };
        let block = component.block();
        let Some(node) = self.block_child(parent, name, Kind::Block, block, base) else {
            return;
        };
        let at = base + block.offset() + 12;
        let length = block.payload().len();
        match component {
            Component::Positions(value) | Component::Normals(value) => self.field(
                node,
                "f32[3]",
                typed(summary(&value.values), FieldType::Array(ScalarType::F32)),
                at,
                value.values.len() * 12,
            ),
            Component::Uvs(value) => self.field(
                node,
                "f32[2]",
                typed(summary(&value.values), FieldType::Array(ScalarType::F32)),
                at,
                value.values.len() * 8,
            ),
            Component::Colors(value) | Component::Attribute12(value) => self.field(
                node,
                "f32[4]",
                typed(summary(&value.values), FieldType::Array(ScalarType::F32)),
                at,
                value.values.len() * 16,
            ),
            Component::MaterialList(value)
            | Component::MaterialMap(value)
            | Component::BoneMap(value) => self.field(
                node,
                "u32 索引",
                typed(summary(&value.values), FieldType::Array(ScalarType::U32)),
                at,
                value.values.len() * 4,
            ),
            Component::Weights(value) => self.field(
                node,
                "原始权重",
                format!(
                    "{} 个顶点，{} 个骨骼影响；保留文件权重范围",
                    value.vertices.len(),
                    value
                        .vertices
                        .iter()
                        .map(|v| v.influences.len())
                        .sum::<usize>()
                ),
                at,
                length,
            ),
            Component::Faces(value) => {
                for group in &value.groups {
                    match group {
                        fmod::FaceGroup::Strips(strips) => {
                            if let Some(child) =
                                self.block_child(node, "三角带", Kind::Block, strips.block, base)
                            {
                                self.field(
                                    child,
                                    "三角带索引",
                                    format!(
                                        "{} 条三角带；{} 个索引",
                                        strips.strips.len(),
                                        strips
                                            .strips
                                            .iter()
                                            .map(|v| v.indices.len())
                                            .sum::<usize>()
                                    ),
                                    base + strips.block.offset() + 12,
                                    strips.block.payload().len(),
                                );
                            }
                        }
                        fmod::FaceGroup::Unknown(block) => {
                            self.block_child(node, "未知面数据", Kind::Block, *block, base);
                        }
                    }
                }
            }
            Component::WordGroups(value) => {
                for (index, group) in value.groups.iter().enumerate() {
                    let at = base + group.offset;
                    let size = 4 + group.words.len() * 4;
                    let Some(child) = self.child(
                        node,
                        format!("字组 {index}"),
                        Kind::Block,
                        self.document.nodes[node].buffer,
                        at..at + size,
                    ) else {
                        break;
                    };
                    self.field(child, "count", group.words.len(), at, 4);
                    self.field(
                        child,
                        "u32 words",
                        formatted(&group.words, format!("{:08X?}", group.words)),
                        at + 4,
                        group.words.len() * 4,
                    );
                }
                if !value.trailing.is_empty() {
                    self.field(
                        node,
                        "trailing",
                        hex(value.trailing),
                        at + length - value.trailing.len(),
                        value.trailing.len(),
                    );
                }
            }
            Component::Rendering(value) => {
                for (index, word) in value.words.iter().enumerate() {
                    self.field(
                        node,
                        format!("word_{:02X}", index * 4),
                        formatted(word, format!("{word:#010X}")),
                        at + index * 4,
                        4,
                    );
                }
                if !value.trailing.is_empty() {
                    self.field(
                        node,
                        "trailing",
                        hex(value.trailing),
                        at + 72,
                        value.trailing.len(),
                    );
                }
            }
            Component::Unknown(_) => self.field(node, "原始数据", hex(block.payload()), at, length),
        }
    }

    fn skeleton(&mut self, node: usize, file: &Fskl<'_>, base: usize) {
        self.block_fields(node, file.root, base);
        if let Err(error) = file.validate_hierarchy() {
            self.fail(node, error.to_string());
        }
        // File order is preserved. Hierarchy links remain properties, so cycles
        // or a corrupt link cannot make the UI's tree recursive.
        let mut root_tables = file.root_tables.iter().peekable();
        let mut bones = file
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, item)| match item {
                NodeEntry::Bone(bone) => Some((index, bone)),
                NodeEntry::Unknown(_) => None,
            })
            .peekable();
        for block in &file.blocks {
            if let Some(table) = root_tables.next_if(|table| table.block.offset() == block.offset())
            {
                if let Some(child) = self.block_child(node, "根节点索引", Kind::Block, *block, base)
                {
                    self.field(
                        child,
                        "indices",
                        typed(summary(&table.values), FieldType::Array(ScalarType::U32)),
                        base + block.offset() + 12,
                        table.values.len() * 4,
                    );
                }
                continue;
            }
            if let Some((index, bone)) =
                bones.next_if(|(_, bone)| bone.block.offset() == block.offset())
            {
                let Some(child) = self.block_child(
                    node,
                    format!("节点 {index} · ID {}", bone.node_id),
                    Kind::Bone,
                    *block,
                    base,
                ) else {
                    break;
                };
                let at = base + block.offset() + 12;
                for (name, value, offset) in [
                    ("node_id", bone.node_id, 0),
                    ("parent_index", bone.parent_index, 4),
                    ("first_child_index", bone.first_child_index, 8),
                    ("next_sibling_index", bone.next_sibling_index, 12),
                ] {
                    self.field(child, name, value, at + offset, 4);
                }
                self.field(
                    child,
                    "scale",
                    typed(
                        format!("{:?}", bone.transform.scale),
                        FieldType::Array(ScalarType::F32),
                    ),
                    at + 16,
                    16,
                );
                self.field(
                    child,
                    "rotation",
                    typed(
                        format!("{:?}", bone.transform.rotation),
                        FieldType::Array(ScalarType::F32),
                    ),
                    at + 32,
                    16,
                );
                self.field(
                    child,
                    "translation",
                    typed(
                        format!("{:?}", bone.transform.translation),
                        FieldType::Array(ScalarType::F32),
                    ),
                    at + 48,
                    16,
                );
                self.field(
                    child,
                    "unknown_40",
                    formatted(
                        bone.unknown_40,
                        format!("{}（{:#010X}）", bone.unknown_40 as i16, bone.unknown_40),
                    ),
                    at + 64,
                    4,
                );
                self.field(
                    child,
                    "motion_tag（动画分组）",
                    formatted(
                        bone.motion_tag,
                        format!("{}（{:#010X}）", bone.motion_tag as u16, bone.motion_tag),
                    ),
                    at + 68,
                    4,
                );
                self.field(
                    child,
                    "unknown_48",
                    hex(bone.unknown_48),
                    at + 72,
                    bone.unknown_48.len(),
                );
            } else {
                self.block_child(
                    node,
                    format!("未知骨架块 {:#X}", block.header.kind),
                    Kind::Block,
                    *block,
                    base,
                );
            }
        }
    }

    fn effects(&mut self, node: usize, file: &EffectArchive<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        self.field(
            node,
            "count",
            file.directory.count,
            base + file.directory.table_offset - 4,
            4,
        );
        let at = base + file.index.offset as usize;
        if let Some(index) = self.child(
            node,
            "特效索引",
            Kind::Block,
            buffer,
            at..at + file.index.as_bytes().len(),
        ) {
            self.field(index, "unknown_00", file.index.unknown_00, at, 2);
            self.field(index, "count", file.index.count, at + 2, 2);
            for (i, reference) in file.index.entries.iter().enumerate() {
                self.field(
                    index,
                    format!("[{i}].kind"),
                    reference.kind,
                    at + 4 + i * 4,
                    2,
                );
                self.field(
                    index,
                    format!("[{i}].resource_id"),
                    reference.resource_id,
                    at + 6 + i * 4,
                    2,
                );
            }
            if !file.index.trailing_bytes.is_empty() {
                self.field(
                    index,
                    "trailing",
                    hex(file.index.trailing_bytes),
                    at + file.index.as_bytes().len() - file.index.trailing_bytes.len(),
                    file.index.trailing_bytes.len(),
                );
            }
        }
        for member in &file.members {
            let at = base + member.offset as usize;
            let kind = match member.reference.kind {
                1 => Kind::EffectBank,
                2 => Kind::EffectMotionEvents,
                _ => Kind::Unknown,
            };
            let Some(child) = self.child(
                node,
                format!(
                    "{:04} · 资源 {}",
                    member.index, member.reference.resource_id
                ),
                kind,
                buffer,
                at..at + member.size as usize,
            ) else {
                break;
            };
            let meta = base + file.directory.table_offset + member.index * 8;
            self.field(
                child,
                "offset",
                formatted(member.offset, format!("{:#X}", member.offset)),
                meta,
                4,
            );
            self.field(child, "size", member.size, meta + 4, 4);
            let descriptor = base + file.index.offset as usize + 4 + (member.index - 1) * 4;
            self.field(child, "kind", member.reference.kind, descriptor, 2);
            self.field(
                child,
                "resource_id",
                member.reference.resource_id,
                descriptor + 2,
                2,
            );
            match member.resource() {
                Ok(EffectResource::Bank(bank)) => {
                    self.field(child, "version", bank.version, at, 2);
                    for (i, count) in bank.counts.iter().enumerate() {
                        self.field(child, format!("count_{i}"), count, at + 2 + i * 2, 2);
                    }
                    self.field(child, "unknown_14", hex(&bank.unknown_14), at + 20, 8);
                    if !bank.trailing_bytes.is_empty() {
                        self.field(
                            child,
                            "trailing",
                            hex(bank.trailing_bytes),
                            at + bank.as_bytes().len() - bank.trailing_bytes.len(),
                            bank.trailing_bytes.len(),
                        );
                    }
                    self.document.nodes[child].deferred =
                        bank.counts[..8].iter().any(|&count| count != 0);
                }
                Ok(EffectResource::MotionEvents(events)) => {
                    for (name, value, offset) in [
                        ("unknown_00", events.unknown_00, 0),
                        ("lookup_count", events.lookup_count, 2),
                        ("event_count", events.event_count, 4),
                        ("unknown_06", events.unknown_06, 6),
                    ] {
                        self.field(child, name, value, at + offset, 2);
                    }
                    if !events.trailing_bytes.is_empty() {
                        self.field(
                            child,
                            "trailing",
                            hex(events.trailing_bytes),
                            at + events.as_bytes().len() - events.trailing_bytes.len(),
                            events.trailing_bytes.len(),
                        );
                    }
                    self.document.nodes[child].deferred =
                        events.lookup.is_some() || !events.events.is_empty();
                }
                Ok(EffectResource::Unknown(bytes)) => {
                    self.field(child, "原始数据", hex(bytes), at, bytes.len())
                }
                Err(error) => self.fail(child, error.to_string()),
            }
        }
    }

    fn effect_bank_details(&mut self, node: usize, bank: &EffectBank<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        let at = base + bank.table_offsets[0];
        if !bank.emitters.is_empty()
            && let Some(parent) = self.child(
                node,
                "发射器",
                Kind::Block,
                buffer,
                at..at + bank.emitters.len() * 112,
            )
        {
            for (index, emitter) in bank.emitters.iter().enumerate() {
                let at = at + index * 112;
                let Some(child) = self.child(
                    parent,
                    format!("发射器 {index} · {}", emitter.emitter_id),
                    Kind::Block,
                    buffer,
                    at..at + 112,
                ) else {
                    break;
                };
                for (i, (name, bits)) in [
                    ("position", emitter.position_bits),
                    ("position_random", emitter.position_random_bits),
                    ("rotation", emitter.rotation_bits),
                    ("rotation_random", emitter.rotation_random_bits),
                    ("scale", emitter.scale_bits),
                    ("scale_random", emitter.scale_random_bits),
                ]
                .into_iter()
                .enumerate()
                {
                    self.field(
                        child,
                        name,
                        typed(
                            format!("{:?} · {:08X?}", bits.map(f32::from_bits), bits),
                            FieldType::Array(ScalarType::F32),
                        ),
                        at + i * 12,
                        12,
                    );
                }
                self.field(
                    child,
                    "unknown_48",
                    formatted(emitter.unknown_48, format!("{:#010X}", emitter.unknown_48)),
                    at + 72,
                    4,
                );
                for (name, value, offset) in [
                    ("definition_id", emitter.definition_id, 76),
                    ("trigger_frame", emitter.trigger_frame, 78),
                    ("emitter_id", emitter.emitter_id, 80),
                    ("unknown_52", emitter.unknown_52, 82),
                    ("flags", emitter.flags, 84),
                    ("unknown_56", emitter.unknown_56, 86),
                    ("unknown_5a", emitter.unknown_5a, 90),
                ] {
                    self.field(child, name, value, at + offset, 2);
                }
                self.field(child, "spawn_count", emitter.spawn_count, at + 88, 2);
                self.field(
                    child,
                    "unknown_5c",
                    formatted(emitter.unknown_5c, format!("{:#010X}", emitter.unknown_5c)),
                    at + 92,
                    4,
                );
                self.field(child, "unknown_60", hex(&emitter.unknown_60), at + 96, 16);
            }
        }
        for (index, name, stride, value) in [
            (1, "三分量曲线", 24, summary(&bank.vector_keys)),
            (2, "颜色曲线", 16, summary(&bank.color_keys)),
            (3, "整数曲线", 16, summary(&bank.integer_keys)),
            (4, "定义 · 56 字节", 56, summary(&bank.definitions_56)),
            (5, "定义 · 140 字节", 140, summary(&bank.definitions_140)),
        ] {
            let count = bank.counts[index] as usize;
            if count == 0 {
                continue;
            }
            let at = base + bank.table_offsets[index];
            if let Some(child) =
                self.child(node, name, Kind::Block, buffer, at..at + count * stride)
            {
                self.field(child, "count", count, base + 2 + index * 2, 2);
                self.field(child, "records", value, at, count * stride);
            }
        }
        if let Some(lookup) = &bank.motion_lookup {
            self.effect_lookup(node, lookup, base);
        }
        self.effect_events(node, &bank.motion_events, base);
    }

    fn effect_lookup(&mut self, node: usize, lookup: &MotionLookup, base: usize) {
        let at = base + lookup.offset;
        let count = lookup.event_indices.len();
        if let Some(child) = self.child(
            node,
            "动作事件索引",
            Kind::Block,
            self.document.nodes[node].buffer,
            at..at + 4 + count * 4,
        ) {
            self.field(child, "start", lookup.start, at, 2);
            self.field(child, "end（不含）", lookup.end, at + 2, 2);
            self.field(
                child,
                "event_indices",
                format!(
                    "{count} 槽位 · {} 有效索引 · {}",
                    lookup
                        .event_indices
                        .iter()
                        .filter(|value| value.is_some())
                        .count(),
                    summary(&lookup.event_indices)
                ),
                at + 4,
                count * 4,
            );
        }
    }

    fn effect_events(&mut self, node: usize, events: &[MotionEvent], base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (index, event) in events.iter().enumerate() {
            let at = base + event.offset;
            let Some(child) = self.child(
                node,
                format!(
                    "事件 {index} · 动作 {} / 帧 {}",
                    event.motion_id, event.frame
                ),
                Kind::Block,
                buffer,
                at..at + MotionEvent::SIZE,
            ) else {
                break;
            };
            self.field(
                child,
                "position",
                typed(
                    format!(
                        "{:?} · {:08X?}",
                        event.position_bits.map(f32::from_bits),
                        event.position_bits
                    ),
                    FieldType::Array(ScalarType::F32),
                ),
                at,
                12,
            );
            for (name, value, offset) in [
                ("motion_id", event.motion_id, 12),
                ("frame", event.frame, 14),
                ("node_index", event.node_index, 16),
                ("emitter_id", event.emitter_id, 18),
                ("resource_id", event.resource_id, 20),
            ] {
                self.field(child, name, value, at + offset, 2);
            }
            self.field(
                child,
                "flags",
                typed(event.flags, FieldType::Flags(ScalarType::U16)),
                at + 22,
                2,
            );
            self.field(child, "unknown_18", hex(&event.unknown_18), at + 24, 8);
        }
    }

    fn grouped_materials(&mut self, node: usize, file: &GroupedMaterials<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        if let Some(marker) = file.version_marker {
            self.field(
                node,
                "version_marker",
                formatted(marker, format!("{marker:#04X}")),
                base,
                1,
            );
        }
        self.field(
            node,
            "count",
            file.header.count,
            base + file.header.offset,
            1,
        );
        self.field(
            node,
            "header.unknown",
            hex(&file.header.unknown),
            base + file.header.offset + 1,
            15,
        );
        for (index, group) in file.groups.iter().enumerate() {
            let at = base + group.header.offset;
            let size = 16
                + group
                    .records
                    .iter()
                    .map(|record| record.as_bytes().len())
                    .sum::<usize>();
            let Some(parent) = self.child(
                node,
                format!("材质组 {index}"),
                Kind::Block,
                buffer,
                at..at + size,
            ) else {
                break;
            };
            self.field(parent, "count", group.header.count, at, 1);
            self.field(
                parent,
                "header.unknown",
                hex(&group.header.unknown),
                at + 1,
                15,
            );
            for (index, record) in group.records.iter().enumerate() {
                let at = base + record.offset;
                let size = record.as_bytes().len();
                let Some(child) = self.child(
                    parent,
                    format!("参数记录 {index}"),
                    Kind::Material,
                    buffer,
                    at..at + size,
                ) else {
                    break;
                };
                for (name, color, offset) in [
                    ("color_00", record.color_00, 0),
                    ("color_10", record.color_10, 16),
                    ("color_20", record.color_20, 32),
                ] {
                    self.field(
                        child,
                        name,
                        typed(
                            format!("{:?} · {:08X?}", color.map(f32::from_bits), color),
                            FieldType::Array(ScalarType::F32),
                        ),
                        at + offset,
                        16,
                    );
                }
                for (index, word) in record.parameter_words.iter().enumerate() {
                    self.field(
                        child,
                        format!("word_{:02X}", 48 + index * 4),
                        formatted(word, format!("{word:#010X}")),
                        at + 48 + index * 4,
                        4,
                    );
                }
                self.field(
                    child,
                    "unknown_tail",
                    hex(record.unknown_tail),
                    at + size - record.unknown_tail.len(),
                    record.unknown_tail.len(),
                );
            }
        }
        if !file.trailing.is_empty() {
            self.field(
                node,
                "trailing",
                hex(file.trailing),
                base + file.as_bytes().len() - file.trailing.len(),
                file.trailing.len(),
            );
        }
    }

    fn motion_archive(&mut self, node: usize, file: &MotionArchive<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (group_index, group) in file.groups.iter().enumerate() {
            let table = base + (group.offsets_offset / 4 * 4) as usize;
            let Some(parent) = self.child(
                node,
                format!("目录记录 {group_index}"),
                Kind::Block,
                buffer,
                table..table + group.motion_offsets.len() * 4,
            ) else {
                break;
            };
            self.field(
                parent,
                "count",
                group.motion_offsets.len(),
                base + group_index * 8,
                4,
            );
            self.field(
                parent,
                "offsets_offset",
                formatted(group.offsets_offset, format!("{:#X}", group.offsets_offset)),
                base + group_index * 8 + 4,
                4,
            );
            for (slot, offset) in group.motion_offsets.iter().enumerate() {
                let name = format!("动画 · 记录 {group_index} / 槽位 {slot}");
                let motion = offset
                    .map(|offset| Motion::parse_at(file.as_bytes(), offset as usize))
                    .transpose();
                match motion {
                    Ok(Some(motion)) => {
                        let at = base + motion.offset;
                        let Some(child) = self.child(
                            parent,
                            name,
                            Kind::Motion,
                            buffer,
                            at..at + motion.as_bytes().len(),
                        ) else {
                            break;
                        };
                        self.field(
                            child,
                            "motion_offset",
                            formatted(motion.offset, format!("{:#X}", motion.offset)),
                            table + slot * 4,
                            4,
                        );
                        self.motion_summary(child, &motion, base);
                        self.document.nodes[child].deferred = !motion.tracks.is_empty();
                    }
                    Ok(None) => {
                        let Some(child) = self.child(
                            parent,
                            format!("{name} · 空"),
                            Kind::Empty,
                            buffer,
                            table + slot * 4..table + slot * 4 + 4,
                        ) else {
                            break;
                        };
                        self.field(
                            child,
                            "motion_offset",
                            typed("0xFFFFFFFF", FieldType::Scalar(ScalarType::U32)),
                            table + slot * 4,
                            4,
                        );
                    }
                    Err(error) => {
                        let Some(child) = self.child(
                            parent,
                            name,
                            Kind::Unknown,
                            buffer,
                            table + slot * 4..table + slot * 4 + 4,
                        ) else {
                            break;
                        };
                        self.fail(child, error.to_string());
                    }
                }
            }
        }
    }

    fn event_camera(&mut self, node: usize, camera: &EventCamera<'_>, base: usize) {
        self.field(node, "unknown_00", camera.unknown_00, base, 4);
        self.field(node, "unknown_04", camera.unknown_04, base + 4, 4);
        self.field(
            node,
            "unknown_08",
            typed(
                format!(
                    "{} ({:#010X})",
                    f32::from_bits(camera.unknown_08_bits),
                    camera.unknown_08_bits
                ),
                FieldType::Scalar(ScalarType::F32),
            ),
            base + 8,
            4,
        );
        self.field(node, "帧数", camera.frame_count, base + 12, 4);
        for (index, name) in ["视野角", "位置", "滚转角", "目标"].into_iter().enumerate()
        {
            self.field(
                node,
                format!("{name}数组偏移"),
                formatted(
                    camera.array_offsets[index],
                    format!("{:#X}", camera.array_offsets[index]),
                ),
                base + 16 + 4 * index,
                4,
            );
        }
        self.document.nodes[node].deferred = camera.frame_count != 0;
    }

    fn event_camera_frames(&mut self, node: usize, camera: &EventCamera<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (index, name) in ["视野角", "位置 XYZ", "滚转角", "目标 XYZ"]
            .into_iter()
            .enumerate()
        {
            let at = base + camera.array_offsets[index] as usize;
            let bytes = camera.arrays[index];
            let Some(child) = self.child(node, name, Kind::Block, buffer, at..at + bytes.len())
            else {
                break;
            };
            let stride = EventCamera::STRIDES[index];
            for (frame, value) in bytes.chunks_exact(stride).enumerate() {
                let values = value
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|word| {
                        let bits = u32::from_le_bytes(*word);
                        format!("{} ({bits:#010X})", f32::from_bits(bits))
                    })
                    .collect::<Vec<_>>();
                self.field(
                    child,
                    format!("帧 {frame}"),
                    typed(values.join(", "), FieldType::Array(ScalarType::F32)),
                    at + frame * stride,
                    stride,
                );
            }
        }
    }

    fn motion_summary(&mut self, node: usize, motion: &Motion<'_>, base: usize) {
        let at = base + motion.offset;
        self.motion_header(node, motion.header, at);
        self.field(
            node,
            "metadata_present",
            motion.metadata_present,
            at + 12,
            4,
        );
        self.field(node, "metadata", motion.metadata, at + 16, 4);
    }

    fn motion_tracks(&mut self, node: usize, motion: &Motion<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (index, track) in motion.tracks.iter().enumerate() {
            let at = base + track.offset;
            let Some(child) = self.child(
                node,
                format!("轨道 {index}"),
                Kind::Track,
                buffer,
                at..at + track.as_bytes().len(),
            ) else {
                break;
            };
            self.motion_header(child, track.header, at);
            for (index, channel) in track.channels.iter().enumerate() {
                let at = base + channel.offset;
                let Some(leaf) = self.child(
                    child,
                    format!("通道 {index} · {:?}", channel.target_slot()),
                    Kind::Channel,
                    buffer,
                    at..at + channel.as_bytes().len(),
                ) else {
                    break;
                };
                self.motion_header(leaf, channel.header, at);
                self.field(leaf, "encoding", format!("{:?}", channel.encoding()), at, 4);
                self.field(
                    leaf,
                    "native_key_count",
                    channel.native_key_count(),
                    at + 4,
                    2,
                );
                self.field(
                    leaf,
                    "编码关键帧",
                    format!(
                        "{} 个关键帧，步长 {:?}",
                        channel.native_key_count(),
                        channel.encoding().stride()
                    ),
                    at + 12,
                    channel.payload().len(),
                );
            }
        }
    }

    fn motion_header(&mut self, node: usize, header: mhf_resource::motion::BlockHeader, at: usize) {
        self.field(
            node,
            "kind",
            formatted(header.kind, format!("{:#010X}", header.kind)),
            at,
            4,
        );
        self.field(node, "count", header.count, at + 4, 4);
        self.field(node, "byte_size", header.byte_size, at + 8, 4);
    }
}

fn hex(bytes: &[u8]) -> FieldValue {
    let mut value = FieldType::Bytes
        .decode(&bytes[..bytes.len().min(24)])
        .expect("a byte preview is bounded to 24 bytes");
    if bytes.len() > 24 {
        value.push_str(&format!(" … 共 {} 字节", bytes.len()));
    }
    typed(value, FieldType::Bytes)
}

fn archive_name(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn summary<T: fmt::Debug>(values: &[T]) -> String {
    if values.len() <= 8 {
        format!("{values:?}")
    } else {
        format!("{:?} … 共 {} 项", &values[..8], values.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(values: &[u32]) -> Vec<u8> {
        values
            .iter()
            .flat_map(|value| value.to_le_bytes())
            .collect()
    }

    fn jkr(payload: &[u8]) -> Vec<u8> {
        let mut bytes = b"JKR\x1a\x08\x01\0\0".to_vec();
        bytes.extend(words(&[16, payload.len() as u32]));
        bytes.extend_from_slice(payload);
        bytes
    }

    fn archive(members: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = words(&[members.len() as u32]);
        let mut offset = 4 + members.len() * 8;
        for member in members {
            bytes.extend(words(&[offset as u32, member.len() as u32]));
            offset += member.len();
        }
        for member in members {
            bytes.extend_from_slice(member);
        }
        bytes
    }

    fn model_collection() -> Vec<Vec<u8>> {
        let geometry = archive(&[jkr(&words(&[1, 0, 12])), jkr(&words(&[0xc000_0000, 0, 12]))]);
        let textures = archive(&[Vec::new()]);
        (0..4)
            .flat_map(|_| [geometry.clone(), textures.clone()])
            .collect()
    }

    fn all_ranges_are_in_owned_buffers(document: &Document) {
        assert!(document.root < document.nodes.len());
        for (index, node) in document.nodes.iter().enumerate() {
            assert_eq!(document.bytes(index).unwrap().len(), node.range.len());
            let buffer = &document.buffers[node.buffer];
            for field in &node.fields {
                assert!(
                    buffer
                        .get(
                            field.binding.range.start
                                ..field.binding.range.start + field.binding.range.len()
                        )
                        .is_some(),
                    "{} {}: {}+{} / {}",
                    node.name,
                    field.name,
                    field.binding.range.start,
                    field.binding.range.len(),
                    buffer.len()
                );
                if field.binding.format != FieldType::ReadOnly {
                    assert!(!field.binding.range.is_empty());
                    field
                        .read(&document.buffers)
                        .unwrap_or_else(|error| panic!("{} {}: {error}", node.name, field.name));
                }
            }
            for &child in &node.children {
                assert!(child > index && child < document.nodes.len());
            }
        }
    }

    #[test]
    fn container_children_share_source_while_decoded_layers_have_owned_buffers() {
        let inner = jkr(b"raw payload");
        let mut source = words(&[2, 20, inner.len() as u32, 0xffff_ffff, 0]);
        source.extend_from_slice(&inner);
        let source: Arc<[u8]> = source.into();
        let document = inspect("test.bin", source.clone());
        assert!(Arc::ptr_eq(&source, &document.buffers[0]));
        assert_eq!(document.buffers.len(), 2);
        assert_eq!(document.nodes[0].kind, Kind::Archive);
        let compressed = document.nodes[0].children[0];
        assert_eq!(document.nodes[compressed].kind, Kind::Jkr);
        assert_eq!(document.bytes(compressed).unwrap(), inner);
        let decoded = document.nodes[compressed].children[0];
        assert_eq!(document.bytes(decoded).unwrap(), b"raw payload");
        assert_eq!(document.nodes[compressed].buffer, 0);
        assert_eq!(document.nodes[decoded].buffer, 1);
        let empty = document.nodes[0].children[1];
        assert!(document.bytes(empty).unwrap().is_empty());
        all_ranges_are_in_owned_buffers(&document);
    }

    #[test]
    fn unknown_data_does_not_become_a_directory_without_resource_context() {
        let mut raw = words(&[1, 12, 3]);
        raw.extend_from_slice(b"abc");
        let document = inspect("mystery.dat", raw.clone().into());
        assert_eq!(document.nodes[0].kind, Kind::Unknown);
        assert!(document.nodes[0].children.is_empty());
        assert_eq!(document.bytes(0).unwrap(), raw);
        // An embedded signature is never scanned or exposed as a resource.
        let document = inspect("mystery.dat", Arc::from(&b"prefixJKR\x1a"[..]));
        assert_eq!(document.nodes[0].kind, Kind::Unknown);
    }

    #[test]
    fn deep_containers_and_wrappers_keep_every_layer_without_a_depth_limit() {
        let mut source = b"abc".to_vec();
        for _ in 0..64 {
            source = archive(&[jkr(&source)]);
        }
        let document = inspect("file.bin", source.clone().into());
        assert!(document.nodes.iter().all(|node| node.error.is_none()));
        assert_eq!(document.nodes.len(), 129);
        assert_eq!(document.buffers.len(), 65);
        assert_eq!(document.bytes(0).unwrap(), source);
        assert_eq!(document.bytes(document.nodes.len() - 1), Some(&b"abc"[..]));
        all_ranges_are_in_owned_buffers(&document);
        let document = inspect("broken.bin", Arc::from(&b"JKR\x1a\0"[..]));
        assert_eq!(document.nodes[0].kind, Kind::Jkr);
        assert!(document.nodes[0].error.is_some());
    }

    #[test]
    fn typed_blocks_keep_unknown_bytes_and_absolute_hex_offsets() {
        let mut model = words(&[1, 1, 28, 0x12345678, 0, 16]);
        model.extend_from_slice(b"keep");
        let mut source = words(&[1, 12, model.len() as u32]);
        source.extend_from_slice(&model);
        let document = inspect("file.bin", source.into());
        let node = document.nodes[0].children[0];
        assert_eq!(document.nodes[node].kind, Kind::Fmod);
        let child = document.nodes[node].children[0];
        assert_eq!(document.nodes[child].range.start, 24);
        assert_eq!(document.bytes(child).unwrap(), &model[12..]);
        assert_eq!(document.nodes[child].fields[0].binding.range.start, 24);
        all_ranges_are_in_owned_buffers(&document);
    }

    #[test]
    fn skeleton_blocks_keep_native_ordinals_across_root_tables_and_unknown_metadata() {
        use mhf_resource::fskl::{BONE, BONE_HD, BONE_RECORD_SIZE, ROOT_INDICES, SKELETON};

        let bone = |kind, id| {
            let mut bytes = words(&[kind, 1, (12 + BONE_RECORD_SIZE) as u32, id]);
            bytes.extend(words(&[u32::MAX; 3]));
            bytes.resize(12 + BONE_RECORD_SIZE, 0);
            bytes
        };
        let blocks = [
            words(&[ROOT_INDICES, 1, 16, 1]),
            words(&[0x1234_0001, 0, 12]),
            bone(BONE_HD, 17),
            words(&[0x1234_0000, 0, 12]),
            words(&[ROOT_INDICES, 1, 16, 2]),
            bone(BONE, 29),
        ];
        let size = 12 + blocks.iter().map(Vec::len).sum::<usize>();
        let mut skeleton = words(&[SKELETON, blocks.len() as u32, size as u32]);
        skeleton.extend(blocks.iter().flatten());
        let document = inspect("nested.bin", archive(&[skeleton]).into());
        let root = document.nodes[document.root].children[0];
        assert_eq!(document.nodes[root].kind, Kind::Fskl);
        // Unknown node layouts remain inspectable even when validation fails.
        assert!(document.nodes[root].error.is_some());
        let children = &document.nodes[root].children;
        assert_eq!(children.len(), blocks.len());
        for (&child, expected) in children.iter().zip(&blocks) {
            assert_eq!(document.bytes(child).unwrap(), expected);
        }
        assert_eq!(document.nodes[children[0]].name, "根节点索引");
        assert_eq!(document.nodes[children[4]].name, "根节点索引");
        assert_eq!(document.nodes[children[2]].name, "节点 1 · ID 17");
        assert_eq!(document.nodes[children[5]].name, "节点 2 · ID 29");
        all_ranges_are_in_owned_buffers(&document);
    }

    #[test]
    fn repeated_model_texture_pairs_do_not_force_positional_formats() {
        let members = model_collection();
        let source: Arc<[u8]> = archive(&members).into();
        let document = inspect("renamed.bin", source.clone());
        assert!(Arc::ptr_eq(&source, &document.buffers[0]));
        assert_eq!(document.nodes[0].children.len(), members.len());
        for (&index, member) in document.nodes[0].children.iter().zip(&members) {
            assert_eq!(document.nodes[index].kind, Kind::Archive);
            assert_eq!(document.bytes(index).unwrap(), member);
        }
        for kind in [Kind::Fmod, Kind::Fskl] {
            assert_eq!(
                document
                    .nodes
                    .iter()
                    .filter(|node| node.kind == kind)
                    .count(),
                4
            );
        }
        assert!(document.nodes.iter().all(|node| node.error.is_none()));
        all_ranges_are_in_owned_buffers(&document);

        // An invalid hinted member still reports its failure after all other
        // formats have been checked, and keeps its complete original range.
        for index in [2, 5, 6] {
            let mut malformed = members.clone();
            malformed[index] = b"broken resource".to_vec();
            let document = inspect("renamed.bin", archive(&malformed).into());
            let child = document.nodes[0].children[index];
            assert!(document.nodes[child].error.is_some(), "member {index}");
            assert_eq!(document.bytes(child).unwrap(), malformed[index]);
            all_ranges_are_in_owned_buffers(&document);
        }
    }

    #[test]
    fn effect_descriptors_identify_any_directory_position_and_keep_unknown_members() {
        let mut bank = vec![0; 28];
        bank[..2].copy_from_slice(&4_u16.to_le_bytes());
        bank.extend_from_slice(&[0xaa, 0xbb]);
        let descriptor: Vec<_> = [1_u16, 2, 1, 3000, 0x77, 5000]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        let unknown = vec![0xde, 0xad, 0xbe, 0xef];
        for malformed_bank in [false, true] {
            let mut bank = bank.clone();
            if malformed_bank {
                // Declares an emitter which the bounded member does not contain.
                bank[2..4].copy_from_slice(&1_u16.to_le_bytes());
            }
            let effects = archive(&[descriptor.clone(), bank.clone(), unknown.clone()]);
            let wrapped = archive(&[Vec::new(), jkr(&effects)]);
            let document = inspect("unrelated.bin", wrapped.into());
            let layer = document.nodes[0].children[1];
            let root = document.nodes[layer].children[0];
            assert_eq!(document.nodes[root].kind, Kind::EffectArchive);
            assert_eq!(document.bytes(root).unwrap(), effects);
            let children = &document.nodes[root].children;
            assert_eq!(children.len(), 3);
            assert_eq!(document.bytes(children[0]).unwrap(), descriptor);
            assert_eq!(document.nodes[children[1]].kind, Kind::EffectBank);
            assert_eq!(document.bytes(children[1]).unwrap(), bank);
            assert_eq!(document.nodes[children[1]].error.is_some(), malformed_bank);
            assert_eq!(document.nodes[children[2]].kind, Kind::Unknown);
            assert_eq!(document.bytes(children[2]).unwrap(), unknown);
            assert!(document.nodes[children[2]].error.is_none());
            all_ranges_are_in_owned_buffers(&document);
        }

        // A directory with extra unrelated descriptor bytes is insufficient
        // evidence for a generic format probe, even if its first words match.
        let mut unrelated = descriptor;
        unrelated.extend_from_slice(b"unrelated tail");
        let bytes = archive(&[unrelated, bank, unknown]);
        let document = inspect("unrelated.bin", bytes.clone().into());
        assert_eq!(document.nodes[0].kind, Kind::Archive);
        assert_eq!(document.nodes[0].children.len(), 3);
        assert_eq!(document.bytes(0).unwrap(), bytes);
        all_ranges_are_in_owned_buffers(&document);
    }

    #[test]
    fn motion_details_expand_without_truncating_siblings_or_changing_source_offsets() {
        let channel = words(&[0x8021_0001, 1, 20, 1.0_f32.to_bits(), 0.0_f32.to_bits()]);
        let mut track = words(&[0x38, 1, 12 + channel.len() as u32]);
        track.extend(channel);
        let mut clip = words(&[1, 1, 20 + track.len() as u32, 0, 0]);
        clip.extend(track);
        let mut source = words(&[2, 16, 0, 24, 24, 24 + clip.len() as u32]);
        source.extend_from_slice(&clip);
        source.extend_from_slice(&clip);
        let source: Arc<[u8]> = source.into();
        let document = inspect("renamed.mot", source.clone());
        assert_eq!(document.nodes.len(), 5);
        assert!(document.nodes.iter().all(|node| node.error.is_none()));
        let motions: Vec<_> = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind == Kind::Motion)
            .map(|(index, _)| index)
            .collect();
        assert_eq!(motions.len(), 2);
        assert!(motions.iter().all(|&index| document.nodes[index].deferred));
        let expanded = expand(&document, motions[0]).unwrap();
        assert!(Arc::ptr_eq(&expanded.buffers[0], &source));
        assert_eq!(document.nodes.len(), 5);
        assert_eq!(expanded.nodes.len(), 7);
        assert!(!expanded.nodes[motions[0]].deferred);
        assert!(expanded.nodes[motions[1]].deferred);
        for &index in &motions {
            assert_eq!(document.bytes(index), expanded.bytes(index));
        }
        let track = expanded.nodes[motions[0]].children[0];
        assert_eq!(expanded.nodes[track].range.start, 24 + 20);
        assert_eq!(
            expanded.nodes[expanded.nodes[track].children[0]]
                .range
                .start,
            24 + 20 + 12
        );
        all_ranges_are_in_owned_buffers(&expanded);
    }

    #[test]
    fn detail_expansion_preserves_existing_dependency_declarations() {
        use crate::metadata::ModelResources;

        let geometry = archive(&[words(&[1, 0, 12]), words(&[0xc000_0000, 0, 12])]);
        let textures = archive(&[Vec::new()]);
        let mut effects = b"KEFFECT\0".to_vec();
        effects.extend(words(&[0, 0]));
        effects.push(0xaa);
        let mut document = inspect("model.pac", archive(&[geometry, textures, effects]).into());
        let model = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::Fmod)
            .unwrap();
        let scope = document
            .metadata()
            .resolve::<ModelResources>(model)
            .unwrap()
            .source;
        document.nodes[scope].metadata.block::<ModelResources>();
        let details = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::KeyEffects)
            .unwrap();
        assert!(document.nodes[details].deferred);
        let field_count = document.nodes[details].fields.len();

        let expanded = expand(&document, details).unwrap();
        assert!(!expanded.nodes[details].deferred);
        assert!(expanded.nodes[details].fields.len() > field_count);
        assert!(
            expanded
                .metadata()
                .resolve::<ModelResources>(model)
                .is_none()
        );
        assert_eq!(
            expanded.metadata().origins(model),
            document.metadata().origins(model)
        );
        assert!(document.nodes[details].deferred);
        all_ranges_are_in_owned_buffers(&expanded);
    }

    #[test]
    fn unverified_motion_directory_counts_are_not_inferred_from_first_offset() {
        let document = inspect("unknown.mot", words(&[0, 16, 0, 16]).into());
        assert_eq!(document.nodes[0].kind, Kind::Unknown);
        assert!(document.nodes[0].error.is_some());
        assert!(document.nodes[0].children.is_empty());
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn real_model_packages_keep_all_resources_and_expand_effects_with_absolute_offsets() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        for name in [
            "dat/emmodel/em001.pac",
            "dat/emmodel-hd/em019-hd.pac",
            "dat/emmodel/em121.pac",
            "dat/emmodel/em172.pac",
            "dat/emmodel-hd/em150-hd.pac",
        ] {
            let bytes: Arc<[u8]> = std::fs::read(root.join(name)).unwrap().into();
            // Package structure supplies formats even when its filename changes.
            let document = inspect("renamed.bin", bytes);
            assert!(
                document
                    .nodes
                    .iter()
                    .all(|node| node.kind != Kind::Unknown && node.error.is_none()),
                "{name} has an unresolved resource"
            );
            assert!(
                document
                    .nodes
                    .iter()
                    .any(|node| node.kind == Kind::MotionArchive)
            );
            all_ranges_are_in_owned_buffers(&document);
            let mut expanded_kinds = Vec::new();
            for (index, node) in document.nodes.iter().enumerate() {
                if !node.deferred
                    || (node.kind == Kind::Motion && expanded_kinds.contains(&Kind::Motion))
                {
                    continue;
                }
                let expanded = expand(&document, index).unwrap();
                assert!(
                    expanded.nodes.iter().all(|node| node.error.is_none()),
                    "{name} detail expansion was truncated"
                );
                assert_eq!(expanded.bytes(index), document.bytes(index));
                assert!(!expanded.nodes[index].deferred);
                assert!(node.deferred);
                assert!(Arc::ptr_eq(
                    &expanded.buffers[node.buffer],
                    &document.buffers[node.buffer]
                ));
                all_ranges_are_in_owned_buffers(&expanded);
                expanded_kinds.push(node.kind);
            }
            eprintln!(
                "{name}: all resource kinds resolved, {} expansions checked",
                expanded_kinds.len()
            );
        }
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn real_effect_collection_preserves_every_member_and_expands_all_banks() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let source: Arc<[u8]> = std::fs::read(root.join("dat/effect.bin")).unwrap().into();
        // Identification must follow the bytes when the collection is renamed.
        let document = inspect("renamed.bin", source.clone());
        assert!(Arc::ptr_eq(&source, &document.buffers[0]));
        assert!(
            document
                .nodes
                .iter()
                .all(|node| node.kind != Kind::Unknown && node.error.is_none())
        );
        let directory = SimpleArchive::parse(&source, 100).unwrap();
        let children = &document.nodes[document.root].children;
        assert_eq!(children.len(), directory.entries.len());
        for (&index, entry) in children.iter().zip(&directory.entries) {
            assert_eq!(
                document.bytes(index).unwrap(),
                entry.payload(&source).unwrap()
            );
            if entry.size != 0 {
                assert_eq!(document.nodes[index].range.start, entry.offset as usize);
            }
        }
        let repaired = &document.nodes[children[2]];
        assert_eq!(repaired.kind, Kind::Archive);
        assert_eq!(repaired.range, 0x11bc39..0x11bc39 + 9346);
        assert_eq!(document.nodes[children[5]].kind, Kind::Txb);
        for (kind, expected) in [
            (Kind::Fmod, 8),
            (Kind::Fskl, 8),
            (Kind::Txb, 9),
            (Kind::EffectBank, 61),
        ] {
            assert_eq!(
                document
                    .nodes
                    .iter()
                    .filter(|node| node.kind == kind)
                    .count(),
                expected
            );
        }
        all_ranges_are_in_owned_buffers(&document);
        let mut expanded_banks = 0;
        for (index, node) in document.nodes.iter().enumerate() {
            if node.kind != Kind::EffectBank {
                continue;
            }
            let expanded = expand(&document, index).unwrap();
            assert!(expanded.nodes.iter().all(|node| node.error.is_none()));
            assert_eq!(expanded.bytes(index), document.bytes(index));
            assert!(!expanded.nodes[index].deferred);
            assert!(Arc::ptr_eq(
                &expanded.buffers[node.buffer],
                &document.buffers[node.buffer]
            ));
            all_ranges_are_in_owned_buffers(&expanded);
            expanded_banks += 1;
        }
        assert_eq!(expanded_banks, 61);
        eprintln!(
            "effect.bin: all 20 members retained; 8 models, 8 skeletons, 9 texture directories, and 61 fully expanded effect banks"
        );
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original game files only"]
    fn real_model_skeleton_txb_and_motion_documents() {
        let root = std::path::PathBuf::from(
            std::env::var_os("MHF_RESOURCE_GAME_ROOT").expect("set MHF_RESOURCE_GAME_ROOT"),
        );
        for name in [
            "dat/parts/m00/m_editpl.bin",
            "dat/emmodel/em019.pac",
            "dat/motion/npc41.mot",
            "dat/emmodel-hd/em077_b-hd.pac",
            "dat/wd000snd.abn",
            "dat/sound/grdn_fes.snd",
            "dat/sound/mus/s_m68_02.mus",
        ] {
            let path = root.join(name);
            let bytes: Arc<[u8]> = std::fs::read(&path).unwrap().into();
            let document = inspect(&path.to_string_lossy(), bytes);
            all_ranges_are_in_owned_buffers(&document);
            let errors: Vec<_> = document
                .nodes
                .iter()
                .filter_map(|node| {
                    node.error
                        .as_ref()
                        .map(|error| format!("{}: {error}", node.name))
                })
                .collect();
            assert!(errors.is_empty(), "{name}: {errors:?}");
            if name.contains("m_editpl") || name.contains("em019") || name.contains("em077") {
                assert!(document.nodes.iter().any(|node| node.kind == Kind::Fmod));
                assert!(document.nodes.iter().any(|node| node.kind == Kind::Fskl));
                assert!(document.nodes.iter().any(|node| node.kind == Kind::Txb));
            }
            if name.contains("npc41") || name.contains("em019") {
                assert!(document.nodes.iter().any(|node| node.kind == Kind::Motion));
            }
            eprintln!(
                "{name}: {} buffers, {} nodes",
                document.buffers.len(),
                document.nodes.len()
            );
        }
    }
}
