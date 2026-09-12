use std::{collections::BTreeMap, ops::Range, sync::Arc};

use mhf_resource::{
    crypto::{Ecd, Exf},
    jkr::Jkr,
};

use crate::inspect::{self, Document, Kind};

use super::{parents, repack, restore_expanded};

pub(super) struct Change {
    pub buffer: usize,
    pub range: Range<usize>,
    pub bytes: Vec<u8>,
    pub resource: Option<usize>,
}

pub(super) fn rebuild(document: &Document, changes: Vec<Change>) -> Result<Document, String> {
    if changes.is_empty() {
        return Ok(document.clone());
    }
    let ownership = parents(document);
    let mut owners = vec![None; document.buffers.len()];
    let mut pending: Vec<Vec<Change>> = (0..document.buffers.len()).map(|_| Vec::new()).collect();
    for change in changes {
        pending
            .get_mut(change.buffer)
            .ok_or("编辑数据层不存在")?
            .push(change);
    }
    for (index, node) in document.nodes.iter().enumerate() {
        if !matches!(node.kind, Kind::Ecd | Kind::Exf | Kind::Jkr) {
            continue;
        }
        if let [child] = node.children.as_slice() {
            let child = &document.nodes[*child];
            if child.buffer != node.buffer
                && child.range == (0..document.buffers[child.buffer].len())
                && (child.buffer <= node.buffer || owners[child.buffer].replace(index).is_some())
            {
                return Err("解码数据层所有权无效或存在循环".into());
            }
        }
    }
    // Decoded buffers are allocated after their owner. Accumulate siblings in
    // their shared parent buffer before encoding that parent exactly once.
    for buffer in (0..pending.len()).rev() {
        let changes = std::mem::take(&mut pending[buffer]);
        if changes.is_empty() {
            continue;
        }
        let bytes = rebuild_buffer(document, &ownership, buffer, changes)?;
        let root = &document.nodes[document.root];
        if buffer == root.buffer {
            if root.range != (0..document.buffers[buffer].len()) {
                return Err("根资源未覆盖完整源文件".into());
            }
            return Ok(restore_expanded(
                document,
                inspect::inspect(&root.name, Arc::from(bytes)),
            ));
        }
        let owner = owners[buffer].ok_or("解码数据层缺少编码所有者")?;
        let node = &document.nodes[owner];
        let source = document.bytes(owner).ok_or("编码层原始数据不存在")?;
        let bytes = match node.kind {
            Kind::Ecd => Ecd::parse(source).and_then(|file| file.encode(&bytes)),
            Kind::Exf => Exf::parse(source).and_then(|file| file.encode(&bytes)),
            Kind::Jkr => Jkr::parse(source).and_then(|file| file.encode_stored(&bytes)),
            _ => unreachable!("owner is a decoded envelope"),
        }
        .map_err(|error| format!("{}：{error}", node.kind))?;
        pending[node.buffer].push(Change {
            buffer: node.buffer,
            range: node.range.clone(),
            bytes,
            resource: Some(owner),
        });
    }
    Err("编辑数据层未连接到源文件".into())
}

fn rebuild_buffer(
    document: &Document,
    parents: &[Option<(usize, usize)>],
    buffer: usize,
    mut changes: Vec<Change>,
) -> Result<Vec<u8>, String> {
    validate_changes(&mut changes)?;
    let original = &document.buffers[buffer];
    if changes.len() == 1 && changes[0].range == (0..original.len()) {
        return Ok(changes.pop().unwrap().bytes);
    }
    let mut source = original.to_vec();
    let mut groups = BTreeMap::<usize, Vec<Change>>::new();
    for change in changes {
        let target = source
            .get_mut(change.range.clone())
            .ok_or("替换范围超出原始资源")?;
        if target.len() == change.bytes.len() {
            target.copy_from_slice(&change.bytes);
        } else {
            enqueue(document, parents, &mut groups, change)?;
        }
    }
    // Ordinary ownership children have larger indices than their parent. Each
    // group is complete before rebuilding its directory; physical offsets are
    // sorted separately because archive table order need not be file order.
    while let Some((parent, mut children)) = groups.pop_last() {
        validate_changes(&mut children)?;
        let node = &document.nodes[parent];
        let mut bytes = source[node.range.clone()].to_vec();
        for child in children.into_iter().rev() {
            let range = child.range.start - node.range.start..child.range.end - node.range.start;
            bytes = repack::replace(node.kind, &bytes, range, &child.bytes)?;
        }
        if node.range == (0..source.len()) {
            if !groups.is_empty() {
                return Err("同一数据层存在不相连的目录所有者".into());
            }
            return Ok(bytes);
        }
        if node.range.len() == bytes.len() {
            source[node.range.clone()].copy_from_slice(&bytes);
            continue;
        }
        enqueue(
            document,
            parents,
            &mut groups,
            Change {
                buffer,
                range: node.range.clone(),
                bytes,
                resource: Some(parent),
            },
        )?;
    }
    Ok(source)
}

fn enqueue(
    document: &Document,
    parents: &[Option<(usize, usize)>],
    groups: &mut BTreeMap<usize, Vec<Change>>,
    change: Change,
) -> Result<(), String> {
    let node = change.resource.ok_or("内部字段不能改变数据长度")?;
    let (parent, _) = parents[node].ok_or("变长资源缺少所属目录")?;
    let container = &document.nodes[parent];
    if parent >= node
        || container.buffer != change.buffer
        || change.range.start < container.range.start
        || change.range.end > container.range.end
    {
        return Err("变长资源未完整包含在所属目录中".into());
    }
    groups.entry(parent).or_default().push(change);
    Ok(())
}

fn validate_changes(changes: &mut Vec<Change>) -> Result<(), String> {
    changes.sort_by_key(|change| (change.range.start, change.range.end));
    changes.dedup_by(|right, left| right.range == left.range && right.bytes == left.bytes);
    if changes
        .windows(2)
        .any(|pair| pair[0].range.end > pair[1].range.start)
    {
        return Err("编辑范围重叠；不能同时修改同一字节的原始层和解码层或不同别名".into());
    }
    Ok(())
}
