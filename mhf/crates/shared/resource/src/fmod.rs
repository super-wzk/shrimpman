//! FMOD file blocks, before the client converts them to render buffers.
//!
//! Every view retains its complete source block. Unknown blocks and bytes after
//! counted records are preserved. Floats are decoded without arithmetic, colors
//! stay in their file range (normally 0..255), and weights are not normalized.
//! See `docs/model-formats.md` for the native evidence and unresolved fields.

use std::io::{Cursor, Read};

use crate::{Error, Result};

pub const HEADER_SIZE: usize = 12;
pub const FILE: u32 = 1;
pub const MAIN: u32 = 2;
pub const OBJECT: u32 = 4;
pub const FACE: u32 = 5;
pub const MATERIALS: u32 = 9;
pub const TEXTURES: u32 = 10;
pub const INIT: u32 = 0x0002_0000;
pub const STRIPS_A: u32 = 0x0003_0000;
pub const STRIPS_B: u32 = 0x0004_0000;
pub const MATERIAL_LIST: u32 = 0x0005_0000;
pub const MATERIAL_MAP: u32 = 0x0006_0000;
pub const POSITIONS: u32 = 0x0007_0000;
pub const NORMALS: u32 = 0x0008_0000;
pub const UVS: u32 = 0x000a_0000;
pub const COLORS: u32 = 0x000b_0000;
pub const WEIGHTS: u32 = 0x000c_0000;
pub const WORD_GROUPS: u32 = 0x000e_0000;
pub const RENDERING: u32 = 0x000f_0000;
pub const BONE_MAP: u32 = 0x0010_0000;
pub const ATTRIBUTE_12: u32 = 0x0012_0000;

/// All three words are encoded little-endian; size includes the header.
/// `kind` remains numeric because its meaning depends on the parent block.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlockHeader {
    pub kind: u32,
    pub count: u32,
    pub size: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct Block<'a> {
    pub header: BlockHeader,
    offset: usize,
    bytes: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct Children<'a> {
    pub blocks: Vec<Block<'a>>,
    pub trailing: &'a [u8],
}

impl<'a> Block<'a> {
    pub fn parse(bytes: &'a [u8]) -> Result<Self> {
        Self::parse_at(bytes, 0)
    }

    /// Parse a block at an absolute offset into an unrelocated file.
    pub fn parse_at(file: &'a [u8], offset: usize) -> Result<Self> {
        let remaining = file
            .get(offset..)
            .ok_or_else(|| Error::new(offset, "block offset exceeds file"))?;
        let header_bytes = remaining
            .get(..HEADER_SIZE)
            .ok_or_else(|| Error::new(offset, "truncated block header"))?;
        let header = BlockHeader {
            kind: word(&header_bytes[..4]),
            count: word(&header_bytes[4..8]),
            size: word(&header_bytes[8..12]),
        };
        if (header.size as usize) < HEADER_SIZE {
            return Err(Error::new(
                offset + 8,
                "block size is smaller than its header",
            ));
        }
        Ok(Self {
            header,
            offset,
            bytes: remaining
                .get(..header.size as usize)
                .ok_or_else(|| Error::new(offset + 8, "block size exceeds remaining file"))?,
        })
    }

    pub const fn offset(self) -> usize {
        self.offset
    }

    pub const fn as_bytes(self) -> &'a [u8] {
        self.bytes
    }

    pub fn payload(self) -> &'a [u8] {
        &self.bytes[HEADER_SIZE..]
    }

    /// Interpret this block's payload as child blocks only when its parent
    /// context establishes that layout. Material and texture record kind values
    /// overlap structural values, so recursive kind-only dispatch is incorrect.
    pub fn children(self) -> Result<Children<'a>> {
        if self.header.count as usize > self.payload().len() / HEADER_SIZE {
            return Err(self.error(4, "child count exceeds block payload"));
        }
        let mut blocks = Vec::new();
        let mut cursor = HEADER_SIZE;
        for _ in 0..self.header.count {
            let mut child =
                Self::parse_at(self.bytes, cursor).map_err(|e| self.error(e.offset, e.message))?;
            child.offset += self.offset;
            cursor += child.bytes.len();
            blocks.push(child);
        }
        Ok(Children {
            blocks,
            trailing: &self.bytes[cursor..],
        })
    }

    pub(crate) fn error(self, relative: usize, message: impl Into<String>) -> Error {
        Error::new(self.offset.saturating_add(relative), message)
    }

    pub(crate) fn require_payload(self, length: usize) -> Result<()> {
        if self.payload().len() < length {
            return Err(self.error(HEADER_SIZE, "truncated block record"));
        }
        Ok(())
    }

    pub(crate) fn records(self, stride: usize) -> Result<(&'a [u8], &'a [u8])> {
        if stride == 0 || self.header.count as usize > self.payload().len() / stride {
            return Err(self.error(4, "record count exceeds block payload"));
        }
        let end = self.header.count as usize * stride;
        Ok(self.payload().split_at(end))
    }
}

/// Vector components retain IEEE-754 bits, including signed zero and NaNs.
#[derive(Clone, Debug)]
pub struct Vector3Block<'a> {
    pub block: Block<'a>,
    pub values: Vec<[f32; 3]>,
    pub trailing: &'a [u8],
}

impl<'a> Vector3Block<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let (records, trailing) = block.records(12)?;
        let values = records
            .as_chunks::<12>()
            .0
            .iter()
            .map(|record| float_array::<3>(record))
            .collect();
        Ok(Self {
            block,
            values,
            trailing,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Vector4Block<'a> {
    pub block: Block<'a>,
    pub values: Vec<[f32; 4]>,
    pub trailing: &'a [u8],
}

impl<'a> Vector4Block<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let (records, trailing) = block.records(16)?;
        let values = records
            .as_chunks::<16>()
            .0
            .iter()
            .map(|record| float_array::<4>(record))
            .collect();
        Ok(Self {
            block,
            values,
            trailing,
        })
    }
}

#[derive(Clone, Debug)]
pub struct UvBlock<'a> {
    pub block: Block<'a>,
    pub values: Vec<[f32; 2]>,
    pub trailing: &'a [u8],
}

impl<'a> UvBlock<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let (records, trailing) = block.records(8)?;
        let values = records
            .as_chunks::<8>()
            .0
            .iter()
            .map(|record| float_array::<2>(record))
            .collect();
        Ok(Self {
            block,
            values,
            trailing,
        })
    }
}

#[derive(Clone, Debug)]
pub struct IndexBlock<'a> {
    pub block: Block<'a>,
    pub values: Vec<u32>,
    pub trailing: &'a [u8],
}

impl<'a> IndexBlock<'a> {
    pub(crate) fn parse(block: Block<'a>) -> Result<Self> {
        let (records, trailing) = block.records(4)?;
        let values = records
            .as_chunks::<4>()
            .0
            .iter()
            .map(|record| word(record))
            .collect();
        Ok(Self {
            block,
            values,
            trailing,
        })
    }
}

#[derive(Clone, Debug)]
pub struct WordGroup {
    /// File offset of this group's count word, before its original u32 values.
    pub offset: usize,
    pub words: Vec<u32>,
}

/// A structure verified from file bytes. The inspected native object converters
/// do not consume this block, and no index, flag, or sentinel meaning is assumed.
#[derive(Clone, Debug)]
pub struct WordGroupsBlock<'a> {
    pub block: Block<'a>,
    pub groups: Vec<WordGroup>,
    pub trailing: &'a [u8],
}

impl<'a> WordGroupsBlock<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        // Each group needs at least its count word, including empty groups.
        block.records(4)?;
        let data = block.payload();
        let mut cursor = Cursor::new(data);
        let mut groups = Vec::new();
        for _ in 0..block.header.count {
            let offset = block.offset + HEADER_SIZE + cursor.position() as usize;
            let mut count = [0; 4];
            cursor
                .read_exact(&mut count)
                .map_err(|_| Error::new(offset, "truncated word-group count"))?;
            let count = u32::from_le_bytes(count) as usize;
            let start = cursor.position() as usize;
            if count > (data.len() - start) / 4 {
                return Err(Error::new(offset, "word-group count exceeds block payload"));
            }
            let end = start + count * 4;
            let words = data[start..end]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|bytes| u32::from_le_bytes(*bytes))
                .collect();
            cursor.set_position(end as u64);
            groups.push(WordGroup { offset, words });
        }
        Ok(Self {
            block,
            groups,
            trailing: &data[cursor.position() as usize..],
        })
    }
}

/// The 18 original words read by native 10002560. Their individual meanings
/// remain unresolved; retain all bits instead of assigning render-state names.
#[derive(Clone, Debug)]
pub struct RenderingBlock<'a> {
    pub block: Block<'a>,
    pub words: [u32; 18],
    pub trailing: &'a [u8],
}

impl<'a> RenderingBlock<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let bytes = block
            .payload()
            .get(..72)
            .ok_or_else(|| block.error(HEADER_SIZE, "truncated rendering record"))?;
        let words = std::array::from_fn(|i| word(&bytes[i * 4..i * 4 + 4]));
        Ok(Self {
            block,
            words,
            trailing: &block.payload()[bytes.len()..],
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SkinInfluence {
    pub bone_index: u32,
    /// File values commonly use percentages, e.g. 100.0. Never normalize here.
    pub weight: f32,
}

#[derive(Clone, Debug)]
pub struct VertexWeights {
    pub offset: usize,
    pub influences: Vec<SkinInfluence>,
}

#[derive(Clone, Debug)]
pub struct WeightBlock<'a> {
    pub block: Block<'a>,
    pub vertices: Vec<VertexWeights>,
    pub trailing: &'a [u8],
}

impl<'a> WeightBlock<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        // Every vertex has at least the count word, even with zero influences.
        block.records(4)?;
        let data = block.payload();
        let mut cursor = Cursor::new(data);
        let mut vertices = Vec::new();
        for _ in 0..block.header.count {
            let offset = block.offset + HEADER_SIZE + cursor.position() as usize;
            let mut count_bytes = [0; 4];
            cursor
                .read_exact(&mut count_bytes)
                .map_err(|_| Error::new(offset, "truncated skin influence count"))?;
            let count = u32::from_le_bytes(count_bytes) as usize;
            let start = cursor.position() as usize;
            if count > (data.len() - start) / 8 {
                return Err(Error::new(
                    offset,
                    "skin influence count exceeds block payload",
                ));
            }
            let influences = data[start..start + count * 8]
                .as_chunks::<8>()
                .0
                .iter()
                .map(|v| SkinInfluence {
                    bone_index: word(&v[..4]),
                    weight: f32::from_bits(word(&v[4..])),
                })
                .collect();
            vertices.push(VertexWeights { offset, influences });
            cursor.set_position((start + count * 8) as u64);
        }
        Ok(Self {
            block,
            vertices,
            trailing: &data[cursor.position() as usize..],
        })
    }
}

#[derive(Clone, Debug)]
pub struct TriangleStrip {
    pub offset: usize,
    /// Bit 31 controls winding; all lower 31 bits are the index count.
    pub packed_count: u32,
    pub indices: Vec<u32>,
}

impl TriangleStrip {
    pub const fn reversed(&self) -> bool {
        self.packed_count & 0x8000_0000 != 0
    }
}

#[derive(Clone, Debug)]
pub struct StripBlock<'a> {
    pub block: Block<'a>,
    pub strips: Vec<TriangleStrip>,
    pub trailing: &'a [u8],
}

impl<'a> StripBlock<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        block.records(4)?;
        let data = block.payload();
        let mut cursor = Cursor::new(data);
        let mut strips = Vec::new();
        for _ in 0..block.header.count {
            let offset = block.offset + HEADER_SIZE + cursor.position() as usize;
            let mut count_bytes = [0; 4];
            cursor
                .read_exact(&mut count_bytes)
                .map_err(|_| Error::new(offset, "truncated triangle strip count"))?;
            let packed_count = u32::from_le_bytes(count_bytes);
            let count = (packed_count & 0x7fff_ffff) as usize;
            let start = cursor.position() as usize;
            if count > (data.len() - start) / 4 {
                return Err(Error::new(
                    offset,
                    "strip index count exceeds block payload",
                ));
            }
            // Zero-length and degenerate strips are file data, not a parse error.
            let indices = data[start..start + count * 4]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|record| word(record))
                .collect();
            strips.push(TriangleStrip {
                offset,
                packed_count,
                indices,
            });
            cursor.set_position((start + count * 4) as u64);
        }
        Ok(Self {
            block,
            strips,
            trailing: &data[cursor.position() as usize..],
        })
    }
}

#[derive(Clone, Debug)]
pub enum FaceGroup<'a> {
    Strips(StripBlock<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub struct Faces<'a> {
    pub block: Block<'a>,
    pub groups: Vec<FaceGroup<'a>>,
    pub trailing: &'a [u8],
}

impl<'a> Faces<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let children = block.children()?;
        let groups = children
            .blocks
            .into_iter()
            .map(|group| {
                Ok(match group.header.kind {
                    STRIPS_A | STRIPS_B => FaceGroup::Strips(StripBlock::parse(group)?),
                    _ => FaceGroup::Unknown(group),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            block,
            groups,
            trailing: children.trailing,
        })
    }

    pub fn strips(&self) -> impl Iterator<Item = (&StripBlock<'a>, &TriangleStrip)> {
        self.groups
            .iter()
            .filter_map(|group| match group {
                FaceGroup::Strips(strips) => Some(strips),
                FaceGroup::Unknown(_) => None,
            })
            .flat_map(|group| group.strips.iter().map(move |strip| (group, strip)))
    }
}

#[derive(Clone, Debug)]
pub enum Component<'a> {
    Faces(Faces<'a>),
    MaterialList(IndexBlock<'a>),
    MaterialMap(IndexBlock<'a>),
    Positions(Vector3Block<'a>),
    Normals(Vector3Block<'a>),
    Uvs(UvBlock<'a>),
    Colors(Vector4Block<'a>),
    Weights(WeightBlock<'a>),
    WordGroups(WordGroupsBlock<'a>),
    Rendering(RenderingBlock<'a>),
    BoneMap(IndexBlock<'a>),
    /// Native 10003190/10003790 copy four floats; their semantic is unresolved.
    Attribute12(Vector4Block<'a>),
    Unknown(Block<'a>),
}

impl<'a> Component<'a> {
    pub fn block(&self) -> Block<'a> {
        match self {
            Self::Faces(v) => v.block,
            Self::MaterialList(v) | Self::MaterialMap(v) | Self::BoneMap(v) => v.block,
            Self::Positions(v) | Self::Normals(v) => v.block,
            Self::Uvs(v) => v.block,
            Self::Colors(v) | Self::Attribute12(v) => v.block,
            Self::Weights(v) => v.block,
            Self::WordGroups(v) => v.block,
            Self::Rendering(v) => v.block,
            Self::Unknown(v) => *v,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Object<'a> {
    pub block: Block<'a>,
    /// File order and duplicate block kinds are retained.
    pub components: Vec<Component<'a>>,
    pub trailing: &'a [u8],
}

impl<'a> Object<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        let children = block.children()?;
        let components = children
            .blocks
            .into_iter()
            .map(|b| {
                Ok(match b.header.kind {
                    FACE => Component::Faces(Faces::parse(b)?),
                    MATERIAL_LIST => Component::MaterialList(IndexBlock::parse(b)?),
                    MATERIAL_MAP => Component::MaterialMap(IndexBlock::parse(b)?),
                    POSITIONS => Component::Positions(Vector3Block::parse(b)?),
                    NORMALS => Component::Normals(Vector3Block::parse(b)?),
                    UVS => Component::Uvs(UvBlock::parse(b)?),
                    COLORS => Component::Colors(Vector4Block::parse(b)?),
                    WEIGHTS => Component::Weights(WeightBlock::parse(b)?),
                    WORD_GROUPS => Component::WordGroups(WordGroupsBlock::parse(b)?),
                    RENDERING if b.header.count == 1 => {
                        Component::Rendering(RenderingBlock::parse(b)?)
                    }
                    BONE_MAP => Component::BoneMap(IndexBlock::parse(b)?),
                    ATTRIBUTE_12 => Component::Attribute12(Vector4Block::parse(b)?),
                    _ => Component::Unknown(b),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            block,
            components,
            trailing: children.trailing,
        })
    }

    pub fn positions(&self) -> Option<&Vector3Block<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Positions(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn normals(&self) -> Option<&Vector3Block<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Normals(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn uvs(&self) -> Option<&UvBlock<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Uvs(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn colors(&self) -> Option<&Vector4Block<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Colors(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn weights(&self) -> Option<&WeightBlock<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Weights(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    pub fn faces(&self) -> Option<&Faces<'a>> {
        self.components.iter().find_map(|c| {
            if let Component::Faces(v) = c {
                Some(v)
            } else {
                None
            }
        })
    }

    /// Cross-record checks are explicit; parsing never silently repairs files.
    pub fn validate_geometry(&self) -> Result<()> {
        let Some(positions) = self.positions() else {
            return Ok(());
        };
        let count = positions.block.header.count;
        for component in &self.components {
            let b = component.block();
            if matches!(
                component,
                Component::Normals(_)
                    | Component::Uvs(_)
                    | Component::Colors(_)
                    | Component::Weights(_)
                    | Component::Attribute12(_)
            ) && b.header.count != count
            {
                return Err(b.error(4, "vertex attribute count differs from positions"));
            }
        }
        if let Some(faces) = self.faces() {
            for (_, strip) in faces.strips() {
                for (i, &index) in strip.indices.iter().enumerate() {
                    if index >= count {
                        return Err(Error::new(
                            strip.offset + 4 + 4 * i,
                            "strip index exceeds position count",
                        ));
                    }
                }
            }
        }
        Ok(())
    }
}

/// A material table contains individually headed records, not a flat array.
#[derive(Clone, Debug)]
pub struct Material<'a> {
    pub block: Block<'a>,
    /// Native 100027B0 transfers these three four-component color values.
    /// Offset names avoid guessing shader semantics from the importer labels.
    pub color_00: [f32; 4],
    pub color_10: [f32; 4],
    pub color_20: [f32; 4],
    pub parameter_30: f32,
    pub texture_indices: Vec<u32>,
    pub unknown_38: &'a [u8],
    pub trailing: &'a [u8],
}

impl<'a> Material<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        block.require_payload(256)?;
        let data = block.payload();
        let count = word(&data[0x34..0x38]) as usize;
        if count > (data.len() - 256) / 4 {
            return Err(block.error(
                HEADER_SIZE + 0x34,
                "texture reference count exceeds material record",
            ));
        }
        let end = 256 + count * 4;
        Ok(Self {
            block,
            color_00: float_array(&data[..16]),
            color_10: float_array(&data[16..32]),
            color_20: float_array(&data[32..48]),
            parameter_30: f32::from_bits(word(&data[48..52])),
            texture_indices: data[256..end]
                .as_chunks::<4>()
                .0
                .iter()
                .map(|record| word(record))
                .collect(),
            unknown_38: &data[0x38..256],
            trailing: &data[end..],
        })
    }
}

#[derive(Clone, Debug)]
pub struct Texture<'a> {
    pub block: Block<'a>,
    /// Index into the accompanying texture bundle, not a file pointer.
    pub image_id: u32,
    pub width: u32,
    pub height: u32,
    pub unknown_0c: &'a [u8],
    pub trailing: &'a [u8],
}

impl<'a> Texture<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        block.require_payload(256)?;
        let data = block.payload();
        Ok(Self {
            block,
            image_id: word(&data[..4]),
            width: word(&data[4..8]),
            height: word(&data[8..12]),
            unknown_0c: &data[12..256],
            trailing: &data[256..],
        })
    }
}

#[derive(Clone, Debug)]
pub enum ObjectEntry<'a> {
    Object(Object<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub struct Meshes<'a> {
    pub block: Block<'a>,
    pub entries: Vec<ObjectEntry<'a>>,
    pub trailing: &'a [u8],
}

#[derive(Clone, Debug)]
pub struct MaterialTable<'a> {
    pub block: Block<'a>,
    pub records: Vec<MaterialEntry<'a>>,
    pub trailing: &'a [u8],
}

#[derive(Clone, Debug)]
pub enum MaterialEntry<'a> {
    Material(Material<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub struct TextureTable<'a> {
    pub block: Block<'a>,
    pub records: Vec<TextureEntry<'a>>,
    pub trailing: &'a [u8],
}

#[derive(Clone, Debug)]
pub enum TextureEntry<'a> {
    Texture(Texture<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub enum Section<'a> {
    Init(IndexBlock<'a>),
    Meshes(Meshes<'a>),
    Materials(MaterialTable<'a>),
    Textures(TextureTable<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub struct Fmod<'a> {
    pub root: Block<'a>,
    pub sections: Vec<Section<'a>>,
    pub root_trailing: &'a [u8],
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> Fmod<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let root = Block::parse(source)?;
        if root.header.kind != FILE {
            return Err(root.error(0, "expected FMOD FILE block (1)"));
        }
        let children = root.children()?;
        let mut sections = Vec::new();
        for block in children.blocks {
            sections.push(match block.header.kind {
                INIT => Section::Init(IndexBlock::parse(block)?),
                MAIN => {
                    let children = block.children()?;
                    let entries = children
                        .blocks
                        .into_iter()
                        .map(|b| {
                            Ok(if b.header.kind == OBJECT {
                                ObjectEntry::Object(Object::parse(b)?)
                            } else {
                                ObjectEntry::Unknown(b)
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Section::Meshes(Meshes {
                        block,
                        entries,
                        trailing: children.trailing,
                    })
                }
                MATERIALS => {
                    let children = block.children()?;
                    let records = children
                        .blocks
                        .into_iter()
                        .map(|b| {
                            Ok(if b.header.count == 1 {
                                MaterialEntry::Material(Material::parse(b)?)
                            } else {
                                MaterialEntry::Unknown(b)
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Section::Materials(MaterialTable {
                        block,
                        records,
                        trailing: children.trailing,
                    })
                }
                TEXTURES => {
                    let children = block.children()?;
                    let records = children
                        .blocks
                        .into_iter()
                        .map(|b| {
                            Ok(if b.header.count == 1 {
                                TextureEntry::Texture(Texture::parse(b)?)
                            } else {
                                TextureEntry::Unknown(b)
                            })
                        })
                        .collect::<Result<Vec<_>>>()?;
                    Section::Textures(TextureTable {
                        block,
                        records,
                        trailing: children.trailing,
                    })
                }
                _ => Section::Unknown(block),
            });
        }
        Ok(Self {
            root,
            sections,
            root_trailing: children.trailing,
            trailing: &source[root.as_bytes().len()..],
            source,
        })
    }

    /// Exact input, including unknown sections and trailing data.
    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn objects(&self) -> impl Iterator<Item = &Object<'a>> {
        self.sections
            .iter()
            .filter_map(|s| {
                if let Section::Meshes(m) = s {
                    Some(m)
                } else {
                    None
                }
            })
            .flat_map(|m| &m.entries)
            .filter_map(|e| {
                if let ObjectEntry::Object(o) = e {
                    Some(o)
                } else {
                    None
                }
            })
    }

    pub fn materials(&self) -> impl Iterator<Item = &Material<'a>> {
        self.sections
            .iter()
            .filter_map(|s| {
                if let Section::Materials(m) = s {
                    Some(m)
                } else {
                    None
                }
            })
            .flat_map(|m| &m.records)
            .filter_map(|e| {
                if let MaterialEntry::Material(m) = e {
                    Some(m)
                } else {
                    None
                }
            })
    }

    pub fn textures(&self) -> impl Iterator<Item = &Texture<'a>> {
        self.sections
            .iter()
            .filter_map(|s| {
                if let Section::Textures(t) = s {
                    Some(t)
                } else {
                    None
                }
            })
            .flat_map(|t| &t.records)
            .filter_map(|e| {
                if let TextureEntry::Texture(t) = e {
                    Some(t)
                } else {
                    None
                }
            })
    }

    /// Return a copy with exactly one position changed. The object number is its
    /// ordinal in the first MAIN block, including unknown entries, as in 10002680.
    /// This deliberately does not serialize decoded structures or rebuild blocks.
    pub fn with_vertex_position(
        &self,
        object: usize,
        vertex: usize,
        value: [f32; 3],
    ) -> Result<Vec<u8>> {
        let main = self
            .sections
            .iter()
            .find_map(|s| {
                if let Section::Meshes(m) = s {
                    Some(m)
                } else {
                    None
                }
            })
            .ok_or_else(|| self.root.error(0, "FMOD has no MAIN block"))?;
        let Some(ObjectEntry::Object(object)) = main.entries.get(object) else {
            return Err(main
                .block
                .error(0, "object ordinal does not select an OBJECT block"));
        };
        let positions = object
            .positions()
            .ok_or_else(|| object.block.error(0, "object has no positions"))?;
        if vertex >= positions.values.len() {
            return Err(positions.block.error(4, "vertex index is out of range"));
        }
        let offset = positions.block.offset + HEADER_SIZE + vertex * 12;
        let mut output = self.source.to_vec();
        for (i, value) in value.into_iter().enumerate() {
            output[offset + i * 4..offset + i * 4 + 4]
                .copy_from_slice(&value.to_bits().to_le_bytes());
        }
        Ok(output)
    }
}

pub(crate) fn word(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes[..4].try_into().expect("validated record width"))
}

pub(crate) fn float_array<const N: usize>(bytes: &[u8]) -> [f32; N] {
    std::array::from_fn(|i| f32::from_bits(word(&bytes[i * 4..i * 4 + 4])))
}

#[cfg(test)]
#[path = "tests/fmod.rs"]
mod tests;
