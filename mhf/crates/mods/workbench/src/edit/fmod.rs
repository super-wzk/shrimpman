//! Structured edits for validated model resources.

use crate::{
    edit,
    inspect::{Document, Kind},
};
use mhf_resource::fmod::{Fmod, ObjectEntry, RENDERING_VERSION, RENDERING_WORDS, Section};

/// Create the `0xF0000` child that one placeholder item declares. The
/// initialized record uses the confirmed version word and leaves every other
/// word at zero.
pub fn initialize_rendering_block(
    document: &Document,
    block_node: usize,
) -> Result<Document, String> {
    let item = document.nodes.get(block_node).ok_or("渲染参数节点不存在")?;
    if item.kind != Kind::MissingBlock || !item.range.is_empty() {
        return Err("该节点已包含渲染参数数据块".into());
    }
    // Walk the ownership tree up to the FMOD file, remembering the ordinal of
    // the model object among its MAIN siblings. That ordinal counts unknown
    // entries, exactly like the parser's entry index.
    let ownership = edit::parents(document);
    let mut current = block_node;
    // `(MAIN ordinal, byte offset)` of the owning model object.
    let mut object = None;
    let fmod_node = loop {
        let (parent, index) = ownership
            .get(current)
            .copied()
            .flatten()
            .ok_or("渲染参数节点不在模型对象下")?;
        if document.nodes[current].kind == Kind::Object {
            object = Some((index, document.nodes[current].range.start));
        }
        if document.nodes[parent].kind == Kind::Fmod {
            break parent;
        }
        current = parent;
    };
    let (ordinal, object_start) = object.ok_or("渲染参数节点不在模型对象下")?;
    let base = document.nodes[fmod_node].range.start;
    let source = document.bytes(fmod_node).ok_or("FMOD 数据不存在")?;
    let file = Fmod::parse(source).map_err(|error| error.to_string())?;
    let target = file.sections.iter().find_map(|section| match section {
        Section::Meshes(meshes) => meshes.entries.get(ordinal),
        _ => None,
    });
    let Some(ObjectEntry::Object(target)) = target else {
        return Err("FMOD 对象序号与当前节点不一致".into());
    };
    if base + target.block.offset() != object_start {
        return Err("FMOD 对象与当前节点范围不一致".into());
    }
    let mut words = [0; RENDERING_WORDS];
    words[0] = RENDERING_VERSION;
    let bytes = file
        .with_rendering_block(ordinal, words)
        .map_err(|error| error.to_string())?;
    edit::replace(document, fmod_node, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{action::NodeAction, inspect};
    use mhf_resource::fmod::{FILE, HEADER_SIZE, MAIN, OBJECT};

    fn block(kind: u32, count: u32, payload: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::new();
        for word in [kind, count, (payload.len() + HEADER_SIZE) as u32] {
            bytes.extend_from_slice(&word.to_le_bytes());
        }
        bytes.extend_from_slice(payload);
        bytes
    }

    #[test]
    fn initializes_the_item_declared_for_a_missing_block() {
        let source = block(FILE, 1, &block(MAIN, 1, &block(OBJECT, 0, &[0xab, 0xcd])));
        let document = inspect::inspect("model.bin", source.into());
        let item = placeholder(&document);
        assert_eq!(document.nodes[item].name, "渲染参数");
        assert!(document.nodes[item].range.is_empty());
        let at = document.nodes[item].range.start;
        assert_eq!(
            document.nodes[item].action,
            Some(NodeAction::InitializeRenderingBlock)
        );
        let source = document.bytes(document.root).unwrap();
        assert!(
            Fmod::parse(source)
                .unwrap()
                .rendering_block(0)
                .unwrap()
                .is_none()
        );
        let updated = initialize_rendering_block(&document, item).unwrap();
        let root = updated.payload(updated.root).unwrap();
        let file = Fmod::parse(updated.bytes(root).unwrap()).unwrap();
        let rendering = file.rendering_block(0).unwrap().unwrap();
        assert_eq!(rendering.words[0], RENDERING_VERSION);
        assert!(rendering.words[1..].iter().all(|&word| word == 0));
        assert_eq!(rendering.words[mhf_resource::fmod::UV_TRANSFORM_WORD], 0);
        // The created block replaces the item, so nothing offers the action and
        // the stale index can no longer insert a second child.
        assert!(updated.nodes.iter().all(|node| node.action.is_none()));
        let block = updated
            .nodes
            .iter()
            .position(|node| node.name == "渲染参数")
            .unwrap();
        assert_eq!(updated.nodes[block].kind, Kind::Block);
        assert!(!updated.nodes[block].fields.is_empty());
        // The created child occupies the position the item declared.
        assert_eq!(updated.nodes[block].range.start, at);
        assert!(initialize_rendering_block(&updated, block).is_err());
    }

    #[test]
    fn object_ordinal_counts_unknown_main_entries() {
        const UNKNOWN: u32 = 0x00ff_0000;
        let mut main = block(UNKNOWN, 0, &[]);
        main.extend_from_slice(&block(OBJECT, 0, &[0xab, 0xcd]));
        let source = block(FILE, 1, &block(MAIN, 2, &main));
        let document = inspect::inspect("model.bin", source.into());
        let updated = initialize_rendering_block(&document, placeholder(&document)).unwrap();
        let root = updated.payload(updated.root).unwrap();
        let file = Fmod::parse(updated.bytes(root).unwrap()).unwrap();
        assert!(file.rendering_block(0).is_err());
        assert!(file.rendering_block(1).unwrap().is_some());
    }

    /// The rendering-parameter item of the only object in a test model.
    fn placeholder(document: &Document) -> usize {
        document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::MissingBlock)
            .unwrap()
    }
}
