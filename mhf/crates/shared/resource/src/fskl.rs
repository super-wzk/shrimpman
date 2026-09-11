//! FSKL skeleton blocks in file order, without converting them to runtime nodes.

use crate::{
    Result,
    fmod::{Block, HEADER_SIZE, IndexBlock, float_array, word},
};

pub const SKELETON: u32 = 0xc000_0000;
pub const ROOT_INDICES: u32 = 0;
pub const BONE: u32 = 0x4000_0001;
pub const BONE_HD: u32 = 0x4000_0002;
/// An additional transform-node variant found as a root in em150; its role is
/// unresolved, but its stored links and 256-byte transform payload are shared.
pub const BONE_3: u32 = 0x4000_0003;
pub const BONE_RECORD_SIZE: usize = 256;

/// A file node's three stored four-float vectors. These are not a matrix.
/// No quaternion normalization, coordinate conversion, or fourth-component
/// replacement is performed. See the evidence document for semantic limits.
#[derive(Clone, Copy, Debug)]
pub struct Transform {
    pub scale: [f32; 4],
    pub rotation: [f32; 4],
    pub translation: [f32; 4],
}

#[derive(Clone, Debug)]
pub struct Bone<'a> {
    pub block: Block<'a>,
    pub node_id: i32,
    /// Links refer to the ordinal among non-metadata node blocks, not node_id.
    pub parent_index: i32,
    pub first_child_index: i32,
    pub next_sibling_index: i32,
    pub transform: Transform,
    /// Native 100022A0 copies the low WORD; the full file word is retained.
    pub unknown_40: u32,
    /// Public tools label this chainID, but its IK semantics are unverified.
    pub unknown_44: u32,
    pub unknown_48: &'a [u8],
    pub trailing: &'a [u8],
}

impl<'a> Bone<'a> {
    fn parse(block: Block<'a>) -> Result<Self> {
        block.require_payload(BONE_RECORD_SIZE)?;
        let data = block.payload();
        Ok(Self {
            block,
            node_id: word(&data[..4]) as i32,
            parent_index: word(&data[4..8]) as i32,
            first_child_index: word(&data[8..12]) as i32,
            next_sibling_index: word(&data[12..16]) as i32,
            transform: Transform {
                scale: float_array(&data[16..32]),
                rotation: float_array(&data[32..48]),
                translation: float_array(&data[48..64]),
            },
            unknown_40: word(&data[64..68]),
            unknown_44: word(&data[68..72]),
            unknown_48: &data[72..BONE_RECORD_SIZE],
            trailing: &data[BONE_RECORD_SIZE..],
        })
    }
}

#[derive(Clone, Debug)]
pub enum NodeEntry<'a> {
    Bone(Bone<'a>),
    Unknown(Block<'a>),
}

#[derive(Clone, Debug)]
pub struct Fskl<'a> {
    pub root: Block<'a>,
    /// Every immediate child, including root tables and unknown records.
    pub blocks: Vec<Block<'a>>,
    /// Multiple root tables are retained; the native lookup uses the first.
    pub root_tables: Vec<IndexBlock<'a>>,
    /// Non-metadata nodes in their original ordinal order.
    pub nodes: Vec<NodeEntry<'a>>,
    pub root_trailing: &'a [u8],
    pub trailing: &'a [u8],
    source: &'a [u8],
}

impl<'a> Fskl<'a> {
    pub fn parse(source: &'a [u8]) -> Result<Self> {
        let root = Block::parse(source)?;
        if root.header.kind != SKELETON {
            return Err(root.error(0, "expected FSKL skeleton block (0xc0000000)"));
        }
        let children = root.children()?;
        let mut root_tables = Vec::new();
        let mut nodes = Vec::new();
        for &block in &children.blocks {
            match block.header.kind {
                ROOT_INDICES => root_tables.push(IndexBlock::parse(block)?),
                BONE | BONE_HD | BONE_3 if block.header.count == 1 => {
                    nodes.push(NodeEntry::Bone(Bone::parse(block)?));
                }
                // Native 100021C0 skips blocks whose kind's low byte is zero.
                kind if kind & 0xff == 0 => {}
                _ => nodes.push(NodeEntry::Unknown(block)),
            }
        }
        Ok(Self {
            root,
            blocks: children.blocks,
            root_tables,
            nodes,
            root_trailing: children.trailing,
            trailing: &source[root.as_bytes().len()..],
            source,
        })
    }

    pub const fn as_bytes(&self) -> &'a [u8] {
        self.source
    }

    pub fn root_indices(&self) -> &[u32] {
        self.root_tables.first().map_or(&[], |table| &table.values)
    }

    pub fn bones(&self) -> impl Iterator<Item = &Bone<'a>> {
        self.nodes.iter().filter_map(|node| match node {
            NodeEntry::Bone(bone) => Some(bone),
            NodeEntry::Unknown(_) => None,
        })
    }

    /// Validate indices and the child/sibling traversal before visualization.
    /// Invalid data is still inspectable through `parse`; no links are repaired.
    pub fn validate_hierarchy(&self) -> Result<()> {
        let count = self.nodes.len();
        for table in &self.root_tables {
            for (i, &root) in table.values.iter().enumerate() {
                if root as usize >= count {
                    return Err(table
                        .block
                        .error(HEADER_SIZE + i * 4, "root node index is out of range"));
                }
            }
        }
        for node in &self.nodes {
            let bone = match node {
                NodeEntry::Bone(bone) => bone,
                NodeEntry::Unknown(block) => {
                    return Err(block.error(0, "cannot validate an unknown skeleton node layout"));
                }
            };
            for (offset, index) in [
                (4, bone.parent_index),
                (8, bone.first_child_index),
                (12, bone.next_sibling_index),
            ] {
                if index < -1 || index >= 0 && index as usize >= count {
                    return Err(bone
                        .block
                        .error(HEADER_SIZE + offset, "skeleton link is out of range"));
                }
            }
        }
        // Iterative DFS avoids stack overflow on untrusted or deeply nested files.
        let mut marks = vec![0u8; count];
        let mut stack = Vec::new();
        for start in 0..count {
            if marks[start] == 2 {
                continue;
            }
            stack.push((start, false));
            while let Some((index, finish)) = stack.pop() {
                if finish {
                    marks[index] = 2;
                    continue;
                }
                let NodeEntry::Bone(bone) = &self.nodes[index] else {
                    unreachable!()
                };
                if marks[index] == 1 {
                    return Err(bone
                        .block
                        .error(HEADER_SIZE + 8, "cycle in skeleton child/sibling links"));
                }
                if marks[index] == 2 {
                    continue;
                }
                marks[index] = 1;
                stack.push((index, true));
                for linked in [bone.next_sibling_index, bone.first_child_index] {
                    if linked >= 0 {
                        stack.push((linked as usize, false));
                    }
                }
            }
        }
        Ok(())
    }

    /// Change only the four stored translation components of one node ordinal.
    pub fn with_translation(&self, node: usize, value: [f32; 4]) -> Result<Vec<u8>> {
        let Some(NodeEntry::Bone(bone)) = self.nodes.get(node) else {
            return Err(self
                .root
                .error(0, "node ordinal does not select a known bone record"));
        };
        let offset = bone.block.offset() + HEADER_SIZE + 48;
        let mut output = self.source.to_vec();
        for (i, value) in value.into_iter().enumerate() {
            output[offset + i * 4..offset + i * 4 + 4]
                .copy_from_slice(&value.to_bits().to_le_bytes());
        }
        Ok(output)
    }
}

#[cfg(test)]
#[path = "tests/fskl.rs"]
mod tests;
