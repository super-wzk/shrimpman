//! Byte edits belong to the decoded buffer that exposes the field. Repacking
//! walks physical owners back to the source file; parsed nodes are never used
//! as independent files or serialized from their display strings.

use std::ops::Range;

use crate::field::{FieldType, Patch};
use crate::inspect::{self, Document, Kind};

mod batch;
mod repack;
#[cfg(test)]
mod tests;

/// Child ordinals in the ownership tree, independent of buffer allocation and
/// lazy expansion order. Validated reference edges do not create new owners.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeKey(Vec<usize>);

pub fn node_key(document: &Document, mut node: usize) -> Option<NodeKey> {
    let parents = parents(document);
    let mut path = Vec::new();
    for _ in 0..document.nodes.len() {
        if node == document.root {
            path.reverse();
            return Some(NodeKey(path));
        }
        let (parent, ordinal) = *parents.get(node)?.as_ref()?;
        path.push(ordinal);
        node = parent;
    }
    None
}

pub fn locate(document: &Document, key: &NodeKey) -> Option<usize> {
    let mut node = document.root;
    for &ordinal in &key.0 {
        node = *document.nodes.get(node)?.children.get(ordinal)?;
    }
    document.nodes.get(node).map(|_| node)
}

/// Apply a fixed-size overwrite, then rebuild every enclosing envelope and
/// directory before inspecting the complete file again. Length-changing field
/// edits require a verified relocation model, which DAT and arbitrary binaries
/// do not provide. Re-encoding a compressed envelope can still change its size.
pub fn apply(
    document: &Document,
    buffer: usize,
    range: Range<usize>,
    replacement: &[u8],
) -> Result<Document, String> {
    let original = document.buffers.get(buffer).ok_or("编辑数据层不存在")?;
    let current = original.get(range.clone()).ok_or("编辑范围超出数据层")?;
    if range.len() != replacement.len() {
        return Err("字节编辑须等长覆盖；不能自动移动未知结构中的偏移或指针".into());
    }
    if current == replacement {
        return Ok(document.clone());
    }
    batch::rebuild(
        document,
        vec![batch::Change {
            buffer,
            range,
            bytes: replacement.to_vec(),
            resource: None,
        }],
    )
}

/// Apply a UI batch against one document revision. Every `before` is checked
/// before any work starts, and each affected encoding ancestor runs once.
pub fn apply_many(document: &Document, patches: &[Patch]) -> Result<Document, String> {
    let mut changes = Vec::new();
    for patch in patches {
        let before = patch.binding.bytes(&document.buffers)?;
        if before != patch.before {
            return Err("编辑基于旧数据，资源已更新；请重新编辑该字段".into());
        }
        if patch.binding.format == FieldType::ReadOnly {
            return Err("派生说明不能写入资源".into());
        }
        if patch.binding.range.len() != patch.after.len() {
            return Err("字段编辑须保持原有字节容量".into());
        }
        if before != patch.after {
            changes.push(batch::Change {
                buffer: patch.binding.buffer,
                range: patch.binding.range.clone(),
                bytes: patch.after.clone(),
                resource: None,
            });
        }
    }
    batch::rebuild(document, changes)
}

/// Replace an entire file or directory member, including imported image,
/// audio, model and motion assets. Internal records can only be overwritten
/// at their existing length; their containing file owns any relocation rules.
pub fn replace(document: &Document, node: usize, replacement: &[u8]) -> Result<Document, String> {
    let current = document.nodes.get(node).ok_or("替换资源不存在")?;
    if current.range.len() == replacement.len() {
        return apply(document, current.buffer, current.range.clone(), replacement);
    }
    let ownership = parents(document);
    let complete = node == document.root
        || ownership[node].is_some_and(|(parent, _)| {
            let parent = &document.nodes[parent];
            matches!(
                parent.kind,
                Kind::Archive
                    | Kind::Momo
                    | Kind::Mha
                    | Kind::Txb
                    | Kind::Stage
                    | Kind::StageObjectPackage
                    | Kind::EffectArchive
            ) || parent.buffer != current.buffer
                && matches!(parent.kind, Kind::Ecd | Kind::Exf | Kind::Jkr)
                && current.range == (0..document.buffers[current.buffer].len())
        });
    if !complete {
        return Err("内部记录或数据片段不能变长替换；请选择完整文件或目录成员".into());
    }
    batch::rebuild(
        document,
        vec![batch::Change {
            buffer: current.buffer,
            range: current.range.clone(),
            bytes: replacement.to_vec(),
            resource: Some(node),
        }],
    )
}

fn parents(document: &Document) -> Vec<Option<(usize, usize)>> {
    let mut parents = vec![None; document.nodes.len()];
    for (index, node) in document.nodes.iter().enumerate() {
        if node.kind != Kind::StageResourceReference {
            for (ordinal, &child) in node.children.iter().enumerate() {
                if let Some(parent) = parents.get_mut(child) {
                    *parent = Some((index, ordinal));
                }
            }
        }
    }
    parents
}

fn splice(source: &[u8], range: Range<usize>, replacement: &[u8]) -> Result<Vec<u8>, String> {
    source.get(range.clone()).ok_or("替换范围超出原始资源")?;
    let length = (source.len() - range.len())
        .checked_add(replacement.len())
        .ok_or("替换后的资源长度溢出")?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .map_err(|_| "无法分配编辑后的资源")?;
    bytes.extend_from_slice(&source[..range.start]);
    bytes.extend_from_slice(replacement);
    bytes.extend_from_slice(&source[range.end..]);
    Ok(bytes)
}

fn restore_expanded(previous: &Document, mut updated: Document) -> Document {
    let mut pending = vec![(previous.root, updated.root)];
    let mut visited = vec![false; previous.nodes.len()];
    while let Some((old_index, new_index)) = pending.pop() {
        if visited[old_index] {
            continue;
        }
        visited[old_index] = true;
        let old = &previous.nodes[old_index];
        if old.kind != updated.nodes[new_index].kind {
            continue;
        }
        if !old.deferred && updated.nodes[new_index].deferred {
            match inspect::expand(&updated, new_index) {
                Ok(expanded) => updated = expanded,
                Err(error) => updated.nodes[new_index].error = Some(error),
            }
        }
        // A terminated string reparses only up to its new terminator. Retain
        // capacity already observed in this session when shortening zero-filled
        // the rest; never infer free space by scanning beyond that known field.
        let new = &mut updated.nodes[new_index];
        for field in &mut new.fields {
            if !matches!(field.binding.format, FieldType::Text { .. }) {
                continue;
            }
            if let Some(previous) = old.fields.iter().find(|previous| {
                previous.name == field.name
                    && previous.binding.format == field.binding.format
                    && previous.binding.endian == field.binding.endian
                    && previous.binding.buffer == old.buffer
                    && field.binding.buffer == new.buffer
                    && previous.binding.range.start as i128 - old.range.start as i128
                        == field.binding.range.start as i128 - new.range.start as i128
                    && previous.binding.range.len() > field.binding.range.len()
            }) {
                let end = field
                    .binding
                    .range
                    .start
                    .checked_add(previous.binding.range.len());
                if end
                    .and_then(|end| updated.buffers[new.buffer].get(field.binding.range.end..end))
                    .is_some_and(|bytes| bytes.iter().all(|&byte| byte == 0))
                {
                    field.binding.range.end = end.unwrap();
                }
            }
        }
        if old.kind != Kind::StageResourceReference {
            pending.extend(
                old.children
                    .iter()
                    .copied()
                    .zip(updated.nodes[new_index].children.iter().copied()),
            );
        }
    }
    updated
}
