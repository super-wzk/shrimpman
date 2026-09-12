//! Encoded resource names belong to source files and named archive entries. Display labels
//! and reference traversal contexts do not identify a physical resource name.

use mhf_resource::{
    container::MhaArchive,
    crypto::{Ecd, Exf, filename_checksum},
};

use crate::inspect::{Document, Kind};

use super::{
    batch::{self, Change},
    parents,
};

pub(super) fn resource_name<'a>(
    document: &'a Document,
    ownership: &[Option<(usize, usize)>],
    mut node: usize,
) -> Result<Option<&'a [u8]>, String> {
    for _ in 0..document.nodes.len() {
        if node == document.root {
            return Ok(Some(document.nodes[node].name.as_bytes()));
        }
        let (parent, ordinal) = ownership[node].ok_or("资源缺少物理所属目录")?;
        match document.nodes[parent].kind {
            Kind::Mha => {
                let bytes = document.bytes(parent).ok_or("命名目录数据不存在")?;
                let archive =
                    MhaArchive::parse(bytes, bytes.len()).map_err(|error| error.to_string())?;
                return archive
                    .entries
                    .get(ordinal)
                    .map(|entry| Some(entry.name))
                    .ok_or_else(|| "命名目录中没有对应资源".into());
            }
            Kind::Ecd | Kind::Exf | Kind::Jkr => node = parent,
            // Anonymous children have no verified filename contract. In
            // particular, do not borrow their parent container's display name.
            _ => return Ok(None),
        }
    }
    Err("资源文件名作用域存在循环".into())
}

fn changes(document: &Document) -> Result<Vec<Change>, String> {
    let ownership = parents(document);
    let mut candidates = Vec::new();
    for (index, node) in document.nodes.iter().enumerate() {
        // Preserve damaged siblings for inspection and manual repair. Packing
        // validates encoding errors separately; a local edit must not require
        // every unrelated envelope to be decodable first.
        if !matches!(node.kind, Kind::Ecd | Kind::Exf) || node.error.is_some() {
            continue;
        }
        let source = document.bytes(index).ok_or("编码资源原始数据不存在")?;
        let (seed, stored) = match node.kind {
            Kind::Ecd => {
                let file = Ecd::parse(source).map_err(|error| error.to_string())?;
                if file.header.key_index < 4 {
                    continue;
                }
                (file.header.crc32, file.header.filename_checksum)
            }
            Kind::Exf => {
                let file = Exf::parse(source).map_err(|error| error.to_string())?;
                if file.header.key_index != 4 {
                    continue;
                }
                (file.header.seed, file.header.filename_checksum)
            }
            _ => unreachable!(),
        };
        let Some(filename) = resource_name(document, &ownership, index)? else {
            continue;
        };
        let expected = filename_checksum(seed, filename).map_err(|error| error.to_string())?;
        if expected != stored {
            candidates.push((
                index,
                Change {
                    buffer: node.buffer,
                    range: node.range.start + 6..node.range.start + 8,
                    bytes: expected.to_le_bytes().to_vec(),
                    resource: None,
                },
            ));
        }
    }
    if candidates.len() < 2 {
        return Ok(candidates.into_iter().map(|(_, change)| change).collect());
    }
    // An inner header edit re-encodes its enclosing envelopes, which already
    // regenerates their name checksums. Avoid conflicting raw-header patches
    // against those same envelopes in the ancestor's encoded buffer.
    let mut rebuilt_ancestors = vec![false; document.nodes.len()];
    for &(index, _) in &candidates {
        let mut parent = ownership[index];
        while let Some((node, _)) = parent {
            if rebuilt_ancestors[node] {
                break;
            }
            rebuilt_ancestors[node] = true;
            parent = ownership[node];
        }
    }
    Ok(candidates
        .into_iter()
        .filter_map(|(index, change)| (!rebuilt_ancestors[index]).then_some(change))
        .collect())
}

/// Covers decoded edits, imported whole encoded resources, renamed archive
/// members and packing an existing file whose old name checksum is stale.
pub(super) fn refresh(document: Document) -> Result<Document, String> {
    let patches = changes(&document)?;
    if patches.is_empty() {
        return Ok(document);
    }
    let updated = batch::rebuild_layers(&document, patches)?;
    if !changes(&updated)?.is_empty() {
        return Err("无法为重打包资源生成一致的文件名校验".into());
    }
    Ok(updated)
}
