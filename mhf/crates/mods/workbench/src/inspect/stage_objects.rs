//! Native stage member descriptors and references share the original nodes.

use super::{Builder, Document, Hint, InspectionTask, Kind, hex};
use crate::field::formatted;
use mhf_resource::{
    container::StageArchive,
    stage::{ObjectPackage, ObjectTables, ResourceReference},
};
use std::collections::HashSet;

impl Builder {
    pub(super) fn stage_reference(&mut self, node: usize, bytes: &[u8], base: usize) {
        self.document.nodes[node].kind = Kind::StageResourceReference;
        self.field(node, "引用标识", hex(&bytes[..16]), base, 16);
        match ResourceReference::parse(bytes) {
            Ok(reference) => {
                self.field(node, "resource_id", reference.resource_id, base + 16, 4);
                if bytes.len() > ResourceReference::SIZE {
                    self.field(
                        node,
                        "尾部原始字节",
                        hex(&bytes[20..]),
                        base + 20,
                        bytes.len() - 20,
                    );
                }
            }
            Err(error) => self.fail(node, error.to_string()),
        }
    }

    pub(super) fn stage_objects(&mut self, node: usize, package: &ObjectPackage<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        self.field(
            node,
            "目录项数",
            package.archive.count,
            base + package.archive.table_offset - 4,
            4,
        );
        let index = package.archive.entries[0];
        let index_base = base + index.offset as usize;
        let Some(descriptor) = self.child(
            node,
            "成员类型索引",
            Kind::Block,
            buffer,
            index_base..index_base + index.size as usize,
        ) else {
            return;
        };
        self.field(
            descriptor,
            "offset",
            formatted(index.offset, format!("{:#X}", index.offset)),
            base + package.archive.table_offset,
            4,
        );
        self.field(
            descriptor,
            "size",
            index.size,
            base + package.archive.table_offset + 4,
            4,
        );
        self.field(
            descriptor,
            "unknown_00",
            package.index.unknown_00,
            index_base,
            2,
        );
        self.field(descriptor, "成员数", package.index.count, index_base + 2, 2);
        for (index, &kind) in package.index.kinds.iter().enumerate() {
            self.field(
                descriptor,
                format!("成员 {} kind", index + 1),
                kind,
                index_base + 4 + index,
                1,
            );
        }
        if !package.index.trailing.is_empty() {
            let offset = index_base + 4 + package.index.kinds.len();
            self.field(
                descriptor,
                "尾部原始字节",
                hex(package.index.trailing),
                offset,
                package.index.trailing.len(),
            );
        }
        for member in &package.members {
            let entry = member.entry;
            let at = if entry.size == 0 {
                base
            } else {
                base + entry.offset as usize
            };
            let Some(child) = self.child(
                node,
                format!("{:04} · kind {}", entry.index, member.kind),
                Kind::Unknown,
                buffer,
                at..at + entry.size as usize,
            ) else {
                break;
            };
            let meta = base + package.archive.table_offset + entry.index * 8;
            self.field(
                child,
                "offset",
                formatted(entry.offset, format!("{:#X}", entry.offset)),
                meta,
                4,
            );
            self.field(child, "size", entry.size, meta + 4, 4);
            self.field(child, "kind", member.kind, index_base + 3 + entry.index, 1);
            if ResourceReference::has_magic(member.bytes) {
                self.stage_reference(child, member.bytes, at);
                continue;
            }
            self.inspect_node(
                child,
                Hint {
                    directory: true,
                    fmod: member.kind == 1,
                    fskl: member.kind == 2,
                    txb: member.kind == 3,
                    object_tables: member.kind == 4,
                    object_words: member.kind == 14,
                    motion: matches!(member.kind, 8..=11),
                    effect_archive: member.kind == 13,
                    ..Hint::default()
                },
            );
            self.work.push(InspectionTask::CheckObjectMember {
                node: child,
                kind: member.kind,
            });
        }
    }

    pub(super) fn check_stage_member(&mut self, node: usize, kind: u8) {
        if let Some(payload) = self.document.payload(node)
            && !member_kind_matches(kind, self.document.nodes[payload].kind)
        {
            self.fail(
                node,
                format!(
                    "成员类型 {kind} 与实际资源 {} 不一致",
                    self.document.nodes[payload].kind
                ),
            );
        }
    }

    pub(super) fn stage_object_words(
        &mut self,
        node: usize,
        file: &mhf_resource::stage::ObjectWordTable<'_>,
        base: usize,
    ) {
        self.field(
            node,
            "unknown_00",
            formatted(file.unknown_00, format!("{:#010X}", file.unknown_00)),
            base,
            4,
        );
        self.field(node, "值数量", file.count, base + 4, 4);
        if !file.trailing.is_empty() {
            self.field(
                node,
                "尾部原始字节",
                hex(file.trailing),
                base + file.source.len() - file.trailing.len(),
                file.trailing.len(),
            );
        }
        self.document.nodes[node].deferred = file.count != 0;
    }

    pub(super) fn stage_object_word_details(
        &mut self,
        node: usize,
        file: &mhf_resource::stage::ObjectWordTable<'_>,
        base: usize,
    ) {
        for (index, value) in file.values().enumerate() {
            self.field(
                node,
                format!("值 {index}"),
                formatted(value, format!("{value:#010X} · {value}")),
                base + 8 + index * 4,
                4,
            );
        }
    }

    pub(super) fn stage_object_tables(
        &mut self,
        node: usize,
        tables: &ObjectTables<'_>,
        base: usize,
    ) {
        self.field(node, "版本", tables.version, base, 2);
        self.field(node, "unknown_0c", tables.unknown_0c, base + 12, 2);
        self.field(node, "unknown_0e", tables.unknown_0e, base + 14, 2);
        for (index, table) in tables.tables.iter().enumerate() {
            self.field(
                node,
                format!("表 {index} 记录数"),
                table.count,
                base + table.count_offset,
                2,
            );
        }
        if !tables.trailing.is_empty() {
            let offset = base + tables.as_bytes().len() - tables.trailing.len();
            self.field(
                node,
                "尾部原始字节",
                hex(tables.trailing),
                offset,
                tables.trailing.len(),
            );
        }
        self.document.nodes[node].deferred = tables.tables.iter().any(|table| table.count != 0);
    }

    pub(super) fn stage_object_table_details(
        &mut self,
        node: usize,
        tables: &ObjectTables<'_>,
        base: usize,
    ) {
        let buffer = self.document.nodes[node].buffer;
        for (index, table) in tables.tables.iter().enumerate() {
            let at = base + table.offset;
            let Some(child) = self.child(
                node,
                format!("表 {index} · {} 字节/记录", table.record_size),
                Kind::Block,
                buffer,
                at..at + table.records.len(),
            ) else {
                break;
            };
            self.field(child, "记录数", table.count, base + table.count_offset, 2);
            for (record, bytes) in table.records().enumerate() {
                let offset = at + record * table.record_size;
                let words: Vec<_> = bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|word| u32::from_le_bytes(*word))
                    .collect();
                self.field(
                    child,
                    format!("记录 {record}"),
                    formatted(&words, format!("{words:08X?}")),
                    offset,
                    bytes.len(),
                );
            }
        }
    }

    /// Run after ordinary inspection so forward references can use the target's
    /// original member node, including its already decoded envelope children.
    pub(super) fn resolve_stage_references(&mut self) {
        let stages: Vec<_> = self
            .document
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.kind == Kind::Stage).then_some(index))
            .collect();
        for stage_node in stages {
            let owner = &self.document.nodes[stage_node];
            let buffer_index = owner.buffer;
            let range = owner.range.clone();
            let buffer = self.document.buffers[buffer_index].clone();
            let Ok(stage) = StageArchive::parse(&buffer[range.clone()], range.len()) else {
                continue;
            };
            let resources = self.document.nodes[stage_node].children.clone();
            for &resource in &resources {
                let Some(package_node) = self.document.payload(resource) else {
                    continue;
                };
                if self.document.nodes[package_node].kind != Kind::StageObjectPackage {
                    continue;
                }
                let package_owner = &self.document.nodes[package_node];
                let package_buffer = self.document.buffers[package_owner.buffer].clone();
                let Ok(package) = ObjectPackage::parse(
                    &package_buffer[package_owner.range.clone()],
                    package_owner.range.len(),
                ) else {
                    continue;
                };
                let members = self.document.nodes[package_node].children.clone();
                for member in &package.members {
                    let Some(&reference_node) = members.get(member.entry.index) else {
                        continue;
                    };
                    let reference_owner = &self.document.nodes[reference_node];
                    if reference_owner.kind != Kind::StageResourceReference
                        || reference_owner.error.is_some()
                        || !reference_owner.children.is_empty()
                    {
                        continue;
                    }
                    let reference_base = reference_owner.range.start;
                    let Ok(reference) = ResourceReference::parse(member.bytes) else {
                        continue;
                    };
                    let resolved =
                        match stage.resolve_member(reference.resource_id, member.kind, range.len())
                        {
                            Ok(resolved) => resolved,
                            Err(error) => {
                                self.fail(reference_node, error.to_string());
                                continue;
                            }
                        };
                    let target = stage
                        .entries
                        .iter()
                        .position(|entry| entry.resource_id == Some(resolved.resource_id))
                        .and_then(|index| resources.get(index).copied())
                        .and_then(|index| self.document.payload(index))
                        .filter(|&index| {
                            self.document.nodes[index].kind == Kind::StageObjectPackage
                        })
                        .and_then(|index| {
                            self.document.nodes[index]
                                .children
                                .get(resolved.entry_index)
                                .copied()
                        });
                    let Some(target) = target else {
                        self.fail(
                            reference_node,
                            format!(
                                "目标资源 {} 的成员 {} 未完整展开",
                                resolved.resource_id, resolved.entry_index
                            ),
                        );
                        continue;
                    };
                    let target_owner = &self.document.nodes[target];
                    if target_owner.buffer != buffer_index
                        || target_owner.range
                            != (range.start + resolved.offset
                                ..range.start + resolved.offset + resolved.bytes.len())
                        || self.document.bytes(target) != Some(resolved.bytes)
                        || target_owner.kind == Kind::StageResourceReference
                        || reaches(&self.document, target, reference_node)
                    {
                        self.fail(
                            reference_node,
                            "引用目标不是原始终点成员，或会形成资源节点环".into(),
                        );
                        continue;
                    }
                    self.document.nodes[reference_node].children.push(target);
                    self.field(
                        reference_node,
                        "解析资源链",
                        resolved
                            .references
                            .iter()
                            .map(u32::to_string)
                            .collect::<Vec<_>>()
                            .join(" → "),
                        reference_base + 16,
                        0,
                    );
                    self.field(
                        reference_node,
                        "目标成员",
                        resolved.entry_index,
                        reference_base + 16,
                        0,
                    );
                    if let Some(payload) = self.document.payload(target) {
                        let actual = self.document.nodes[payload].kind;
                        if !member_kind_matches(member.kind, actual) {
                            self.fail(
                                reference_node,
                                format!(
                                    "引用成员类型 {} 与目标资源 {} 不一致",
                                    member.kind, actual
                                ),
                            );
                        }
                        if let Some(error) = self.document.nodes[payload].error.clone() {
                            self.fail(reference_node, format!("目标成员解析失败：{error}"));
                        }
                    } else {
                        self.fail(
                            reference_node,
                            "目标成员的编码层或引用链未能完整解析".into(),
                        );
                    }
                }
            }
        }
        for node in &mut self.document.nodes {
            if node.kind == Kind::StageResourceReference
                && node.children.is_empty()
                && node.error.is_none()
            {
                node.error = Some(
                    "此引用需要包含它的 Stage 目录及对象成员类型，当前文档无法解析目标".into(),
                );
            }
        }
    }
}

fn member_kind_matches(kind: u8, actual: Kind) -> bool {
    actual == Kind::Empty
        || match kind {
            1 => actual == Kind::Fmod,
            2 => actual == Kind::Fskl,
            3 => actual == Kind::Txb,
            4 => actual == Kind::StageObjectTables,
            5 | 6 => actual == Kind::Hits,
            7 => actual == Kind::KeyEffects,
            8..=11 => matches!(actual, Kind::Motion | Kind::MotionArchive),
            13 => actual == Kind::EffectArchive,
            14 => actual == Kind::StageObjectWords,
            _ => true,
        }
}

fn reaches(document: &Document, from: usize, target: usize) -> bool {
    let mut pending = vec![from];
    let mut visited = HashSet::new();
    while let Some(index) = pending.pop() {
        if index == target {
            return true;
        }
        if visited.insert(index)
            && let Some(node) = document.nodes.get(index)
        {
            pending.extend(node.children.iter().copied());
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        inspect::{expand, inspect},
        preview::AssetBundle,
    };
    use mhf_resource::container::SimpleArchive;
    use std::sync::Arc;

    fn words(words: &[u32]) -> Vec<u8> {
        words.iter().flat_map(|word| word.to_le_bytes()).collect()
    }

    fn archive(members: &[Vec<u8>]) -> Vec<u8> {
        let mut bytes = words(&[members.len() as u32]);
        let mut offset = 4 + 8 * members.len();
        for member in members {
            bytes.extend(words(&[offset as u32, member.len() as u32]));
            offset += member.len();
        }
        for member in members {
            bytes.extend_from_slice(member);
        }
        bytes
    }

    fn package(kinds: &[u8], members: &[Vec<u8>]) -> Vec<u8> {
        let mut descriptor = vec![1, 0];
        descriptor.extend((kinds.len() as u16).to_le_bytes());
        descriptor.extend_from_slice(kinds);
        let mut entries = vec![descriptor];
        entries.extend_from_slice(members);
        archive(&entries)
    }

    fn reference(id: u32) -> Vec<u8> {
        let mut bytes: Vec<_> = (0xf0..=0xff).rev().collect();
        bytes.extend(id.to_le_bytes());
        bytes
    }

    fn stage(resources: &[(u32, Vec<u8>)]) -> Vec<u8> {
        let mut bytes = vec![0; 28 + 12 * resources.len()];
        bytes[24..28].copy_from_slice(&(resources.len() as u32).to_le_bytes());
        let placement = words(&[2, 0, u32::MAX, 0]);
        let offset = bytes.len() as u32;
        bytes[..4].copy_from_slice(&offset.to_le_bytes());
        bytes[4..8].copy_from_slice(&(placement.len() as u32).to_le_bytes());
        bytes.extend(placement);
        for (index, (id, member)) in resources.iter().enumerate() {
            let offset = bytes.len() as u32;
            let meta = 28 + index * 12;
            bytes[meta..meta + 4].copy_from_slice(&id.to_le_bytes());
            bytes[meta + 4..meta + 8].copy_from_slice(&offset.to_le_bytes());
            bytes[meta + 8..meta + 12].copy_from_slice(&(member.len() as u32).to_le_bytes());
            bytes.extend_from_slice(member);
        }
        bytes
    }

    fn geometry(compressed: bool) -> Vec<u8> {
        let model = words(&[1, 0, 12]);
        if !compressed {
            return model;
        }
        let mut bytes = b"JKR\x1a\x08\x01\0\0".to_vec();
        bytes.extend(words(&[16, model.len() as u32]));
        bytes.extend(model);
        bytes
    }

    fn target(compressed: bool) -> Vec<u8> {
        package(
            &[3, 2, 1],
            &[
                archive(&[Vec::new()]),
                words(&[0xc000_0000, 0, 12]),
                geometry(compressed),
            ],
        )
    }

    fn assert_graph(document: &Document) {
        for (index, node) in document.nodes.iter().enumerate() {
            assert_eq!(document.bytes(index).unwrap().len(), node.range.len());
            for field in &node.fields {
                assert!(
                    document.buffers[node.buffer]
                        .get(
                            field.binding.range.start
                                ..field.binding.range.start + field.binding.range.len()
                        )
                        .is_some()
                );
            }
        }
        let mut colors = vec![0; document.nodes.len()];
        for start in 0..document.nodes.len() {
            let mut pending = vec![(start, false)];
            while let Some((index, leave)) = pending.pop() {
                assert!(index < colors.len());
                if leave {
                    colors[index] = 2;
                    continue;
                }
                if colors[index] == 2 {
                    continue;
                }
                assert_ne!(colors[index], 1, "resource graph cycle at {index}");
                colors[index] = 1;
                pending.push((index, true));
                pending.extend(
                    document.nodes[index]
                        .children
                        .iter()
                        .rev()
                        .map(|&index| (index, false)),
                );
            }
        }
    }

    #[test]
    fn node_bundle_scopes_keep_the_referring_packages_own_texture_bank() {
        let source: Arc<[u8]> = stage(&[
            (10, target(false)),
            (
                20,
                package(
                    &[1, 2, 3],
                    &[reference(10), reference(10), archive(&[Vec::new()])],
                ),
            ),
            (
                30,
                package(&[1, 2, 3], &[reference(10), reference(10), reference(10)]),
            ),
        ])
        .into();
        let document = Arc::new(inspect("scene.bin", source));
        let packages: Vec<_> = document
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.kind == Kind::StageObjectPackage).then_some(index))
            .collect();
        let (bundles, by_node) = AssetBundle::find_with_nodes(document.clone());
        assert_eq!(bundles.len(), 2);
        assert!(bundles[0].model.same_source(&bundles[1].model));
        assert!(!bundles[0].textures[0].same_source(&bundles[1].textures[0]));
        assert_eq!(by_node[document.root], [0, 1]);
        assert_eq!(by_node[packages[0]], [0]);
        assert_eq!(by_node[packages[1]], [1]);
        assert_eq!(by_node[packages[2]], [0]);
        assert!(by_node[bundles[0].model.node].is_empty());
        let referring_model = document.nodes[packages[1]].children[1];
        assert!(by_node[referring_model].is_empty());
    }

    #[test]
    fn kind14_values_follow_descriptors_and_references_without_losing_source_offsets() {
        for header in [0, 0x0000_feff] {
            let values = [0x39a6_adef, 0, u32::MAX];
            let raw = words(&[header, values.len() as u32, values[0], values[1], values[2]]);
            let standalone = inspect("kind14.bin", raw.clone().into());
            assert!(
                standalone
                    .nodes
                    .iter()
                    .all(|node| node.kind != Kind::StageObjectWords)
            );

            let source: Arc<[u8]> = stage(&[
                (10, package(&[14], std::slice::from_ref(&raw))),
                (20, package(&[14], &[reference(10)])),
            ])
            .into();
            let document = inspect("renamed.bin", source.clone());
            assert!(document.nodes.iter().all(|node| node.error.is_none()));
            let node = document
                .nodes
                .iter()
                .position(|node| node.kind == Kind::StageObjectWords)
                .unwrap();
            let reference = document
                .nodes
                .iter()
                .position(|node| node.kind == Kind::StageResourceReference)
                .unwrap();
            assert_eq!(document.payload(reference), Some(node));
            assert_eq!(document.bytes(node).unwrap(), raw);
            let expanded = expand(&document, node).unwrap();
            assert!(Arc::ptr_eq(&source, &expanded.buffers[0]));
            assert_graph(&expanded);
            let base = expanded.nodes[node].range.start;
            for (index, value) in values.into_iter().enumerate() {
                let offset = base + 8 + index * 4;
                assert!(
                    expanded.nodes[node]
                        .fields
                        .iter()
                        .any(|field| field.binding.range.start == offset
                            && field.binding.range.len() == 4)
                );
                assert_eq!(
                    u32::from_le_bytes(expanded.buffers[0][offset..offset + 4].try_into().unwrap()),
                    value
                );
            }
        }
        let raw = words(&[0, 3, 1]);
        let document = inspect(
            "broken.bin",
            package(&[14], std::slice::from_ref(&raw)).into(),
        );
        let node = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::StageObjectWords)
            .unwrap();
        assert!(document.nodes[node].error.is_some());
        assert_eq!(document.bytes(node).unwrap(), raw);
    }

    #[test]
    fn shared_members_link_to_original_nodes_and_pair_by_descriptor_without_duplicates() {
        let source: Arc<[u8]> = stage(&[
            (99, target(true)),
            (
                77,
                package(&[1, 3, 2], &[reference(99), reference(99), reference(99)]),
            ),
            (
                88,
                package(&[2, 1, 3], &[reference(77), reference(77), reference(77)]),
            ),
        ])
        .into();
        let document = Arc::new(inspect("renamed.bin", source.clone()));
        assert_eq!(document.nodes[document.root].kind, Kind::Stage);
        assert!(Arc::ptr_eq(&document.buffers[0], &source));
        assert_eq!(
            document.buffers.len(),
            2,
            "shared references must not decode or copy their target again"
        );
        assert!(
            document
                .nodes
                .iter()
                .all(|node| node.error.is_none() && node.kind != Kind::Unknown)
        );
        let target_package = document.nodes[document.root].children[3];
        let target_members = &document.nodes[target_package].children;
        let references: Vec<_> = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| node.kind == Kind::StageResourceReference)
            .collect();
        assert_eq!(references.len(), 6);
        for (index, node) in references {
            assert_eq!(node.range.len(), ResourceReference::SIZE);
            assert_eq!(node.buffer, 0);
            assert!(ResourceReference::has_magic(document.bytes(index).unwrap()));
            assert_eq!(node.children.len(), 1);
            assert!(target_members.contains(&node.children[0]));
            assert!(
                node.children[0] < index,
                "reference must reuse its earlier target node"
            );
            assert_eq!(document.payload(index), document.payload(node.children[0]));
        }
        assert_graph(&document);
        let bundles = AssetBundle::find_with_nodes(document.clone()).0;
        assert_eq!(bundles.len(), 1);
        assert_eq!(
            bundles[0].model.node,
            document.payload(target_members[3]).unwrap()
        );
        assert_eq!(
            bundles[0].skeleton.as_ref().unwrap().node,
            target_members[2]
        );
        assert_eq!(bundles[0].textures[0].node, target_members[1]);
    }

    #[test]
    fn distinct_original_alias_entries_keep_their_preview_identity() {
        let package = target(false);
        let mut source = stage(&[(10, package.clone()), (11, package)]);
        let first_offset: [u8; 4] = source[32..36].try_into().unwrap();
        source[44..48].copy_from_slice(&first_offset);
        let source: Arc<[u8]> = source.into();
        let document = Arc::new(inspect("renamed.bin", source.clone()));
        assert!(document.nodes.iter().all(|node| node.error.is_none()));
        let bundles = AssetBundle::find_with_nodes(document.clone()).0;
        assert_eq!(bundles.len(), 2);
        assert_eq!(
            bundles[0].model.bytes().unwrap(),
            bundles[1].model.bytes().unwrap()
        );
        assert_eq!(
            document.nodes[bundles[0].model.node].range,
            document.nodes[bundles[1].model.node].range
        );
        assert_ne!(bundles[0].model.node, bundles[1].model.node);
        assert!(!bundles[0].same_source(&bundles[1]));
        assert!(Arc::ptr_eq(&document.buffers[0], &source));
        assert_graph(&document);
    }

    #[test]
    fn missing_and_cyclic_stage_references_remain_errors_with_original_bytes() {
        for source in [
            stage(&[
                (1, package(&[1], &[reference(2)])),
                (2, package(&[1], &[reference(1)])),
            ]),
            stage(&[(1, package(&[1], &[reference(404)]))]),
            reference(404),
        ] {
            let source: Arc<[u8]> = source.into();
            let document = Arc::new(inspect("renamed.bin", source.clone()));
            assert!(Arc::ptr_eq(&document.buffers[0], &source));
            let references: Vec<_> = document
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.kind == Kind::StageResourceReference)
                .collect();
            assert!(!references.is_empty());
            for (index, node) in references {
                assert!(node.error.is_some());
                assert!(node.children.is_empty());
                assert!(ResourceReference::has_magic(document.bytes(index).unwrap()));
                assert_eq!(document.payload(index), None);
            }
            assert!(AssetBundle::find_with_nodes(document.clone()).0.is_empty());
            assert_graph(&document);
        }
        let mut document = inspect("reference.bin", reference(1).into());
        document.nodes[0].children.push(0);
        assert_eq!(document.payload(0), None);
        assert!(
            AssetBundle::find_with_nodes(Arc::new(document))
                .0
                .is_empty()
        );
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; reads original st001 and st017 stage packages"]
    fn actual_stage_objects_share_references_and_produce_unique_preview_bundles() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        for (name, expected_packages, expected_references, expected_bundles, known_unknown) in [
            ("dat/stage/st001.pac", 5, 0, 5, 4),
            ("dat/stage/st017.pac", 10, 25, 4, 2),
        ] {
            let source: Arc<[u8]> = std::fs::read(root.join(name)).unwrap().into();
            let document = Arc::new(inspect("renamed.bin", source.clone()));
            assert!(Arc::ptr_eq(&document.buffers[0], &source));
            assert!(
                document.nodes.iter().all(|node| node.error.is_none()),
                "{name}"
            );
            assert_graph(&document);
            let unknown = document
                .nodes
                .iter()
                .filter(|node| node.kind == Kind::Unknown)
                .count();
            assert!(
                unknown <= known_unknown,
                "{name}: {unknown} unknown resources"
            );
            assert_eq!(
                document
                    .nodes
                    .iter()
                    .filter(|node| node.kind == Kind::StageObjectPackage)
                    .count(),
                expected_packages
            );
            let stage_node = document
                .nodes
                .iter()
                .position(|node| node.kind == Kind::Stage)
                .unwrap();
            let stage_owner = &document.nodes[stage_node];
            let directory = StageArchive::parse(document.bytes(stage_node).unwrap(), 1000).unwrap();
            let mut references = 0;
            for &package_node in &stage_owner.children {
                if document.nodes[package_node].kind != Kind::StageObjectPackage {
                    continue;
                }
                let package =
                    ObjectPackage::parse(document.bytes(package_node).unwrap(), 100).unwrap();
                let members = &document.nodes[package_node].children;
                assert_eq!(members.len(), package.archive.entries.len());
                for member in &package.members {
                    let node_index = members[member.entry.index];
                    let node = &document.nodes[node_index];
                    assert_eq!(document.bytes(node_index).unwrap(), member.bytes);
                    if ResourceReference::has_magic(member.bytes) {
                        let reference = ResourceReference::parse(member.bytes).unwrap();
                        let resolved = directory
                            .resolve_member(reference.resource_id, member.kind, 100)
                            .unwrap();
                        assert_eq!(node.kind, Kind::StageResourceReference);
                        let target = &document.nodes[node.children[0]];
                        assert_eq!(target.buffer, stage_owner.buffer);
                        assert_eq!(
                            target.range.start,
                            stage_owner.range.start + resolved.offset
                        );
                        assert_eq!(document.bytes(node.children[0]).unwrap(), resolved.bytes);
                        assert_eq!(
                            document.payload(node_index),
                            document.payload(node.children[0])
                        );
                        references += 1;
                    }
                    if node.kind == Kind::StageObjectTables && node.deferred {
                        let expanded = expand(&document, node_index).unwrap();
                        assert!(Arc::ptr_eq(
                            &expanded.buffers[node.buffer],
                            &document.buffers[node.buffer]
                        ));
                        assert_eq!(expanded.bytes(node_index), document.bytes(node_index));
                        assert!(expanded.nodes.iter().all(|node| node.error.is_none()));
                        assert_graph(&expanded);
                    }
                }
            }
            assert_eq!(references, expected_references);
            let bundles = AssetBundle::find_with_nodes(document.clone()).0;
            assert_eq!(bundles.len(), expected_bundles, "{name}");
            for (index, bundle) in bundles.iter().enumerate() {
                mhf_resource::fmod::Fmod::parse(bundle.model.bytes().unwrap()).unwrap();
                mhf_resource::fskl::Fskl::parse(bundle.skeleton.as_ref().unwrap().bytes().unwrap())
                    .unwrap();
                SimpleArchive::parse(bundle.textures[0].bytes().unwrap(), 1000).unwrap();
                assert!(
                    bundles[..index]
                        .iter()
                        .all(|earlier| !earlier.same_source(bundle))
                );
            }
            eprintln!(
                "{name}: {expected_packages} object packages, {references} shared references, {} unique preview bundles, {unknown} remaining outer resources, zero errors",
                bundles.len()
            );
        }
    }
}
