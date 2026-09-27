//! Byte edits belong to the decoded buffer that exposes the field. Repacking
//! walks physical owners back to the source file; parsed nodes are never used
//! as independent files or serialized from their display strings.

use std::ops::Range;

use crate::{
    action::NodeAction,
    field::{FieldType, Patch},
    inspect::{self, Document, Kind},
};

#[cfg(test)]
mod alignment_native_tests;
#[cfg(test)]
mod alignment_tests;
mod batch;
#[cfg(test)]
mod filename_tests;
mod filenames;
mod fmod;
#[cfg(test)]
mod inf_tests;
mod repack;
#[cfg(test)]
mod tests;

/// Child ordinals in the ownership tree, independent of buffer allocation and
/// lazy expansion order. Validated reference edges do not create new owners.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NodeKey {
    path: Vec<usize>,
    emd: Option<EmdIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct EmdIdentity {
    kind: Kind,
    name: String,
    range: Range<usize>,
}

impl EmdIdentity {
    fn new(node: &inspect::Node, root: &inspect::Node) -> Option<Self> {
        let range = Self::relative_range(node, root)?;
        Some(Self {
            kind: node.kind,
            name: node.name.clone(),
            range,
        })
    }

    fn relative_range(node: &inspect::Node, root: &inspect::Node) -> Option<Range<usize>> {
        if node.buffer != root.buffer {
            return None;
        }
        Some(
            node.range.start.checked_sub(root.range.start)?
                ..node.range.end.checked_sub(root.range.start)?,
        )
    }
}

#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
impl NodeKey {
    /// EMD association lists can change membership after an edit. Their child
    /// ordinals alone must not redirect a selection or draft to another record.
    pub(crate) fn matches_emd_identity(&self, document: &Document, node: usize) -> bool {
        self.emd.as_ref().is_none_or(|identity| {
            node_key(document, node).is_some_and(|key| key.emd.as_ref() == Some(identity))
        })
    }
}

pub fn node_key(document: &Document, mut node: usize) -> Option<NodeKey> {
    let parents = parents(document);
    let target = document.nodes.get(node)?;
    let mut path = Vec::new();
    let mut emd = None;
    for _ in 0..document.nodes.len() {
        let current = document.nodes.get(node)?;
        if current.kind == Kind::Emd && emd.is_none() {
            emd = Some(EmdIdentity::new(target, current)?);
        }
        if node == document.root {
            path.reverse();
            return Some(NodeKey { path, emd });
        }
        let (parent, ordinal) = *parents.get(node)?.as_ref()?;
        path.push(ordinal);
        node = parent;
    }
    None
}

pub fn locate(document: &Document, key: &NodeKey) -> Option<usize> {
    let mut node = document.root;
    let mut emd = (document.nodes.get(node)?.kind == Kind::Emd).then_some(node);
    for &ordinal in &key.path {
        node = *document.nodes.get(node)?.children.get(ordinal)?;
        if document.nodes.get(node)?.kind == Kind::Emd {
            emd = Some(node);
        }
    }
    if let Some(identity) = &key.emd {
        let current = EmdIdentity::new(document.nodes.get(node)?, document.nodes.get(emd?)?)?;
        if &current != identity {
            return None;
        }
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

/// Perform one operation a validated node recorded. The rebuilt document goes
/// through the same layer repacking as every other byte edit.
pub fn apply_node_action(
    document: &Document,
    node: usize,
    action: NodeAction,
) -> Result<Document, String> {
    match action {
        NodeAction::InitializeRenderingBlock => fmod::initialize_rendering_block(document, node),
    }
}

/// Validate encoded envelopes and MHA ID ranges before writing. Intermediate
/// edits remain inspectable, and raw byte export remains a verbatim view.
#[cfg(any(test, all(feature = "provider", windows, target_arch = "x86")))]
pub(crate) fn prepare_pack(document: &Document, patches: &[Patch]) -> Result<Document, String> {
    let updated = apply_many(document, patches)?;
    for (index, node) in updated.nodes.iter().enumerate() {
        match node.kind {
            Kind::Mha => {
                let bytes = updated.bytes(index).ok_or("MHA 数据不存在")?;
                mhf_resource::container::MhaArchive::parse(bytes, bytes.len())
                    .and_then(|archive| archive.file_id_index())
                    .map_err(|error| format!("MHA 资源 ID 索引无效：{error}"))?;
            }
            Kind::Ecd | Kind::Exf | Kind::Jkr => {
                if let Some(error) = &node.error {
                    return Err(format!("{} 编码资源未通过解析校验：{error}", node.kind));
                }
            }
            _ => {}
        }
    }
    Ok(updated)
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

/// Direct physical owners as `(parent, ordinal)`. Validated stage references
/// never reparent their targets, so ownership stays a tree.
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
    let mut pending = vec![(previous.root, updated.root, None)];
    let mut visited = vec![false; previous.nodes.len()];
    while let Some((old_index, new_index, mut emd)) = pending.pop() {
        if visited[old_index] {
            continue;
        }
        visited[old_index] = true;
        let old = &previous.nodes[old_index];
        if old.kind != updated.nodes[new_index].kind {
            continue;
        }
        if old.kind == Kind::Emd {
            emd = Some((old_index, new_index));
        }
        if let Some((old_root, new_root)) = emd {
            let new = &updated.nodes[new_index];
            let old_range = EmdIdentity::relative_range(old, &previous.nodes[old_root]);
            let new_range = EmdIdentity::relative_range(new, &updated.nodes[new_root]);
            // A lazy script initially spans one byte; expansion determines its
            // end. Check its source first, then let locate check the full range.
            if old.name != new.name
                || !old_range
                    .zip(new_range)
                    .is_some_and(|(old, new)| old.start == new.start)
            {
                continue;
            }
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
                    .zip(updated.nodes[new_index].children.iter().copied())
                    .map(|(old, new)| (old, new, emd)),
            );
        }
    }
    updated
}

#[cfg(test)]
mod emd_identity_tests {
    use super::*;
    use crate::preview::ResourceRef;
    use mhf_resource::emd::RecordKind;
    use std::sync::Arc;

    fn fixture(base: usize) -> Document {
        fn node(
            name: &str,
            kind: Kind,
            range: Range<usize>,
            children: Vec<usize>,
            deferred: bool,
        ) -> inspect::Node {
            inspect::Node {
                name: name.into(),
                kind,
                buffer: 0,
                range,
                fields: Vec::new(),
                metadata: Default::default(),
                children,
                action: None,
                deferred,
                error: None,
            }
        }
        Document {
            nodes: vec![
                node("archive.bin", Kind::Archive, 0..base + 512, vec![1], false),
                node("mhfemd.bin", Kind::Emd, base..base + 512, vec![2], false),
                node("条件记录", Kind::EmdGroup, base..base, vec![3, 4], false),
                node(
                    "记录 000",
                    Kind::EmdRecord(RecordKind::Modifiers),
                    base + 128..base + 156,
                    vec![],
                    true,
                ),
                node(
                    "记录 001",
                    Kind::EmdRecord(RecordKind::Modifiers),
                    base + 156..base + 184,
                    vec![],
                    true,
                ),
            ],
            buffers: vec![vec![0; base + 512].into()],
            root: 0,
        }
    }

    #[test]
    fn removed_emd_relation_cannot_redirect_keys_or_selection_to_next_row() {
        let previous = Arc::new(inspect::expand(&fixture(32), 3).unwrap());
        let removed = node_key(&previous, 3).unwrap();
        let shifted = node_key(&previous, 4).unwrap();
        let selected = ResourceRef::new(previous.clone(), 3);
        let mut updated = fixture(32);
        updated.nodes[2].children.remove(0);
        let updated = Arc::new(restore_expanded(&previous, updated));
        assert!(locate(&updated, &removed).is_none());
        assert!(locate(&updated, &shifted).is_none());
        assert!(selected.remap_path(updated.clone()).is_err());
        assert!(updated.nodes[4].deferred);
        assert!(updated.nodes[4].fields.is_empty());
    }

    #[test]
    fn emd_identity_survives_outer_relocation_and_buffer_reallocation() {
        let previous = Arc::new(inspect::expand(&fixture(32), 3).unwrap());
        let key = node_key(&previous, 3).unwrap();
        let selected = ResourceRef::new(previous.clone(), 3);
        let mut updated = fixture(96);
        updated.buffers.push(updated.buffers[0].clone());
        for node in &mut updated.nodes[1..] {
            node.buffer = 1;
        }
        let updated = Arc::new(restore_expanded(&previous, updated));
        assert_eq!(locate(&updated, &key), Some(3));
        assert_eq!(selected.remap_path(updated.clone()).unwrap().node, 3);
        assert!(!updated.nodes[3].deferred);
        assert!(
            updated.nodes[3].fields.iter().all(|field| {
                field.binding.buffer == 1 && field.binding.range.start >= 96 + 128
            })
        );
    }

    #[test]
    fn emd_identity_checks_source_range_kind_and_empty_group_name() {
        let previous = fixture(32);
        let key = node_key(&previous, 3).unwrap();
        let group = node_key(&previous, 2).unwrap();
        let mut updated = previous.clone();
        updated.nodes[3].range = updated.nodes[4].range.clone();
        assert!(locate(&updated, &key).is_none());
        updated = previous.clone();
        updated.nodes[3].kind = Kind::EmdRecord(RecordKind::SpeciesModifiers);
        assert!(locate(&updated, &key).is_none());
        updated = previous;
        updated.nodes[2].name = "另一组条件".into();
        assert!(locate(&updated, &group).is_none());
    }
}
