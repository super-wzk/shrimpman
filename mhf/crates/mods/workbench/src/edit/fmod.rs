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
