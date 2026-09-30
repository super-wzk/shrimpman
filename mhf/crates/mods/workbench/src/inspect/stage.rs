//! Typed stage fields share their original inspection buffer and offsets.

mod render;

use super::{Builder, Kind, NativeId, hex, summary};
use crate::field::{FieldReference, FieldType, ReferenceCollection, ScalarType, formatted, typed};
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::stage::{
    Hits, KEffect, KEffectRecord, Lighting, Placement, PlacementTable, Record, RenderTables,
};

impl Builder {
    pub(super) fn stage_resource_target(&self, node: usize, id: u32) -> Option<usize> {
        let mut current = Some(node);
        while let Some(index) = current {
            let owner = self.document.nodes.get(index)?;
            if owner.kind == Kind::Stage {
                return owner.children.iter().copied().find(|&child| {
                    self.document.nodes[child].native_id == Some(NativeId::Stage(id))
                });
            }
            current = self.parents.get(index).copied().flatten();
        }
        None
    }

    pub(super) fn has_stage_lighting_prefix(&self, node: usize) -> bool {
        let children = &self.document.nodes[node].children;
        children.len() >= 2
            && self.document.nodes[self.payload(children[0])].kind == Kind::StageLighting
            && self.document.nodes[self.payload(children[1])].kind == Kind::Txb
    }

    pub(super) fn inspect_stage(
        &mut self,
        node: usize,
        bytes: &[u8],
        base: usize,
        render_hint: bool,
    ) -> bool {
        if bytes.starts_with(b"HITS") {
            self.document.nodes[node].kind = Kind::Hits;
            match Hits::parse(bytes) {
                Ok(file) => {
                    let header = file.header;
                    for (offset, name, value) in [
                        (4, "资源大小", header.size),
                        (8, "分区宽度 X", header.cell_size_x),
                        (12, "分区宽度 Z", header.cell_size_z),
                        (16, "分区数 X", header.cells_x),
                        (20, "分区数 Z", header.cells_z),
                        (24, "unknown_18", header.unknown_18),
                        (28, "unknown_1c", header.unknown_1c),
                        (32, "分区目录相对偏移", header.cell_table_offset),
                        (36, "三角形表相对偏移", header.record_table_offset),
                    ] {
                        self.field(node, name, value, base + offset, 4);
                    }
                    self.field(
                        node,
                        "三角形记录数",
                        file.records.len(),
                        base + file.records_offset,
                        0,
                    );
                    self.document.nodes[node].deferred = true;
                }
                Err(error) => self.fail(node, error.to_string()),
            }
            return true;
        }
        if bytes.starts_with(b"KEFFECT") {
            self.document.nodes[node].kind = Kind::KeyEffects;
            match KEffect::parse(bytes) {
                Ok(file) => {
                    self.field(node, "unknown_07", file.unknown_07, base + 7, 1);
                    self.field(node, "unknown_08", file.unknown_08, base + 8, 4);
                    self.field(node, "关键帧记录数", file.count, base + 12, 4);
                    self.document.nodes[node].deferred =
                        file.count != 0 || !file.trailing.is_empty();
                }
                Err(error) => self.fail(node, error.to_string()),
            }
            return true;
        }
        if let Ok(file) = PlacementTable::probe(bytes) {
            self.document.nodes[node].kind = Kind::StagePlacements;
            for (offset, name, value) in [
                (0, "版本", file.version),
                (4, "实例数", file.count),
                (8, "unknown_08", file.unknown_08),
                (12, "unknown_0c", file.unknown_0c),
            ] {
                self.field(node, name, value, base + offset, 4);
            }
            self.document.nodes[node].deferred = file.count != 0;
            return true;
        }
        if let Ok(file) = Lighting::probe(bytes) {
            self.document.nodes[node].kind = Kind::StageLighting;
            self.field(node, "版本", f32::from_bits(file.version_bits), base, 4);
            for (index, name) in [
                "点光源数",
                "环境立方体光源数",
                "光照组数",
                "光照碰撞体数",
                "光照动画组数",
            ]
            .into_iter()
            .enumerate()
            {
                self.field(node, name, file.counts[index], base + 4 + index, 1);
            }
            self.field(node, "reserved_09", hex(&file.reserved_09), base + 9, 3);
            self.document.nodes[node].deferred = true;
            return true;
        }
        let tables = if render_hint {
            RenderTables::parse(bytes)
        } else {
            RenderTables::probe(bytes)
        };
        match tables {
            Ok(file) => {
                self.document.nodes[node].kind = Kind::StageRenderTables;
                self.field(node, "版本", file.version, base, 2);
                self.field(node, "unknown_02", file.unknown_02, base + 2, 2);
                for table in &file.tables {
                    self.field(
                        node,
                        format!("表 {:02X} 记录数", table.count_offset),
                        table.count,
                        base + table.count_offset,
                        2,
                    );
                }
                self.field(node, "unknown_1e", file.unknown_1e, base + 30, 2);
                self.document.nodes[node].deferred = true;
                true
            }
            Err(error) if render_hint => {
                self.document.nodes[node].kind = Kind::StageRenderTables;
                if bytes.len() >= 2 {
                    self.field(
                        node,
                        "版本",
                        u16::from_le_bytes(bytes[..2].try_into().unwrap()),
                        base,
                        2,
                    );
                }
                self.fail(node, error.to_string());
                true
            }
            Err(_) => false,
        }
    }

    fn stage_record(
        &mut self,
        node: usize,
        name: impl Into<String>,
        record: &Record,
        base: usize,
    ) -> Option<usize> {
        let buffer = self.document.nodes[node].buffer;
        let at = base + record.offset;
        let child = self.child(node, name, Kind::Block, buffer, at..at + record.byte_len())?;
        for (index, &word) in record.words.iter().enumerate() {
            self.field(
                child,
                format!("word_{:02X}", index * 4),
                typed(
                    format!(
                        "{word:#010X} · i32 {} · f32 {}",
                        word as i32,
                        f32::from_bits(word)
                    ),
                    FieldType::Scalar(ScalarType::U32),
                ),
                at + index * 4,
                4,
            );
        }
        Some(child)
    }

    pub(super) fn hits_details(&mut self, node: usize, file: &Hits<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        let Some(cells) = self.child(
            node,
            "空间分区",
            Kind::Block,
            buffer,
            base + file.cell_table_offset..base + file.records_offset,
        ) else {
            return;
        };
        self.set_address(cells, node, [Key("cells".into())]);
        for cell in &file.cells {
            let at = base + cell.offset;
            let Some(child) = self.child(
                cells,
                format!("分区 {}", cell.index),
                Kind::Block,
                buffer,
                at..at + cell.references.len() + 4,
            ) else {
                break;
            };
            self.set_address(child, cells, [Index(cell.index as u32)]);
            self.field(
                child,
                "目录相对偏移",
                cell.relative_offset,
                base + file.cell_table_offset + cell.index * 4,
                4,
            );
            self.field(
                child,
                "记录相对偏移",
                typed(
                    summary(&cell.record_offsets().collect::<Vec<_>>()),
                    FieldType::Array(ScalarType::U32),
                ),
                at,
                cell.references.len(),
            );
            self.field(
                child,
                "三角形记录索引",
                summary(&cell.record_indices().collect::<Vec<_>>()),
                at,
                cell.references.len(),
            );
            self.field(
                child,
                "终止标记",
                "0xFFFFFFFF",
                at + cell.references.len(),
                4,
            );
        }
        let Some(records) = self.child(
            node,
            "碰撞三角形",
            Kind::Block,
            buffer,
            base + file.records_offset..base + file.header.size as usize,
        ) else {
            return;
        };
        self.set_address(records, node, [Key("records".into())]);
        for record in &file.records {
            let at = base + record.offset;
            let Some(child) = self.child(
                records,
                format!("三角形 {}", record.index),
                Kind::Block,
                buffer,
                at..at + record.as_bytes().len(),
            ) else {
                break;
            };
            self.set_address(child, records, [Index(record.index as u32)]);
            self.field(
                child,
                "unknown_00",
                formatted(record.unknown_00, format!("{:#010X}", record.unknown_00)),
                at,
                4,
            );
            for (index, vertex) in record.vertices.iter().enumerate() {
                self.field(
                    child,
                    format!("顶点 {index}"),
                    typed(format!("{vertex:?}"), FieldType::Array(ScalarType::F32)),
                    at + 4 + index * 12,
                    12,
                );
            }
            self.field(
                child,
                "平面系数",
                typed(
                    format!("{:?}", record.plane),
                    FieldType::Array(ScalarType::F32),
                ),
                at + 40,
                16,
            );
        }
        if !file.trailing.is_empty() {
            self.field(
                node,
                "尾部原始字节",
                hex(file.trailing),
                base + file.header.size as usize,
                file.trailing.len(),
            );
        }
    }

    pub(super) fn stage_placements(&mut self, node: usize, file: &PlacementTable<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (index, placement) in file.placements.iter().enumerate() {
            let at = base + placement.offset;
            let Some(child) = self.child(
                node,
                format!("实例 {index}"),
                Kind::Block,
                buffer,
                at..at + Placement::SIZE,
            ) else {
                break;
            };
            self.set_address(child, node, [Index(index as u32)]);
            for (offset, name, bits) in [
                (0, "vector_00", placement.vector_00_bits.as_slice()),
                (12, "vector_0c", placement.vector_0c_bits.as_slice()),
                (32, "vector_20", placement.vector_20_bits.as_slice()),
            ] {
                self.field(
                    child,
                    name,
                    typed(
                        format!(
                            "{:?} · {bits:08X?}",
                            bits.iter()
                                .map(|&value| f32::from_bits(value))
                                .collect::<Vec<_>>()
                        ),
                        FieldType::Array(ScalarType::F32),
                    ),
                    at + offset,
                    bits.len() * 4,
                );
            }
            for (offset, name, value) in [
                (24, "unknown_18", placement.unknown_18),
                (28, "unknown_1c", placement.unknown_1c),
                (56, "unknown_38", placement.unknown_38),
            ] {
                self.field(child, name, value, at + offset, 4);
            }
            for (offset, name, value) in [
                (48, "unknown_30", placement.unknown_30),
                (50, "unknown_32", placement.unknown_32),
                (52, "unknown_34", placement.unknown_34),
                (54, "resource_id", placement.resource_id),
            ] {
                self.field(child, name, value, at + offset, 2);
                if name == "resource_id" {
                    let field = self.document.nodes[child].fields.len() - 1;
                    self.document.nodes[child].fields[field].reference =
                        Some(FieldReference::Indexed(
                            ReferenceCollection::StageResources,
                            u32::from(value),
                            self.stage_resource_target(node, u32::from(value)),
                        ));
                }
            }
        }
    }

    pub(super) fn key_effects(&mut self, node: usize, file: &KEffect<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        for (index, record) in file.records.iter().enumerate() {
            let at = base + record.offset;
            let Some(child) = self.child(
                node,
                format!("记录 {index} · 帧 {}", record.frame()),
                Kind::Block,
                buffer,
                at..at + KEffectRecord::SIZE,
            ) else {
                break;
            };
            self.set_address(child, node, [Index(index as u32)]);
            self.field(child, "kind", record.kind, at, 4);
            self.field(child, "target_id", record.target_id, at + 4, 4);
            self.field(
                child,
                "frame",
                formatted(
                    record.frame(),
                    format!("{} ({:#010X})", record.frame(), record.frame_bits),
                ),
                at + 8,
                4,
            );
            for (offset, words) in [
                (12, record.parameters_0c.as_slice()),
                (68, record.unknown_44.as_slice()),
            ] {
                for (index, &bits) in words.iter().enumerate() {
                    self.field(
                        child,
                        format!("word_{:02X}", offset + index * 4),
                        typed(
                            format!("{bits:#010X} · f32 {}", f32::from_bits(bits)),
                            FieldType::Scalar(ScalarType::U32),
                        ),
                        at + offset + index * 4,
                        4,
                    );
                }
            }
            self.field(child, "render_mode", record.render_mode, at + 64, 4);
        }
        if !file.trailing.is_empty() {
            self.field(
                node,
                "尾部原始字节",
                hex(file.trailing),
                base + file.as_bytes().len() - file.trailing.len(),
                file.trailing.len(),
            );
        }
    }

    pub(super) fn stage_lighting_details(&mut self, node: usize, file: &Lighting<'_>, base: usize) {
        for (key, name, group, records) in [
            ("point_lights", "点光源", None, &file.point_lights),
            (
                "directional_lights",
                "方向光 A",
                Some(0),
                &file.directional_lights[0],
            ),
            (
                "directional_lights",
                "方向光 B",
                Some(1),
                &file.directional_lights[1],
            ),
            (
                "cube_map_lights",
                "环境立方体光源",
                None,
                &file.cube_map_lights,
            ),
            ("light_groups", "光照组", None, &file.light_groups),
        ] {
            for (index, record) in records.iter().enumerate() {
                if let Some(child) =
                    self.stage_record(node, format!("{name} {index}"), record, base)
                {
                    let mut segments = vec![Key(key.into())];
                    if let Some(group) = group {
                        segments.push(Index(group));
                    }
                    segments.push(Index(index as u32));
                    self.set_address(child, node, segments);
                }
            }
        }
        for (index, collision) in file.light_collisions.iter().enumerate() {
            for (key, label, record) in [
                ("header", format!("光照碰撞体 {index}"), &collision.header),
                (
                    "members",
                    format!("光照碰撞体 {index} 成员"),
                    &collision.members,
                ),
            ] {
                if let Some(child) = self.stage_record(node, label, record, base) {
                    self.set_address(
                        child,
                        node,
                        [
                            Key("light_collisions".into()),
                            Index(index as u32),
                            Key(key.into()),
                        ],
                    );
                }
            }
        }
        for (index, animation) in file.light_animations.iter().enumerate() {
            if let Some(child) =
                self.stage_record(node, format!("光照动画组 {index}"), &animation.header, base)
            {
                self.set_address(
                    child,
                    node,
                    [
                        Key("light_animations".into()),
                        Index(index as u32),
                        Key("header".into()),
                    ],
                );
            }
            for (channel_index, channel) in animation.channels.iter().enumerate() {
                if let Some(child) = self.stage_record(
                    node,
                    format!("动画组 {index} 通道 {channel_index}"),
                    &channel.header,
                    base,
                ) {
                    self.set_address(
                        child,
                        node,
                        [
                            Key("light_animations".into()),
                            Index(index as u32),
                            Key("channels".into()),
                            Index(channel_index as u32),
                            Key("header".into()),
                        ],
                    );
                }
                for (key, record) in channel.keys.iter().enumerate() {
                    if let Some(child) = self.stage_record(
                        node,
                        format!("动画组 {index} 通道 {channel_index} 关键帧 {key}"),
                        record,
                        base,
                    ) {
                        self.set_address(
                            child,
                            node,
                            [
                                Key("light_animations".into()),
                                Index(index as u32),
                                Key("channels".into()),
                                Index(channel_index as u32),
                                Key("keys".into()),
                                Index(key as u32),
                            ],
                        );
                    }
                }
            }
        }
        let post = &file.post_process;
        for (key, name, record) in [
            ("god_rays", "光束", &post.god_rays),
            ("height_fog", "高度雾", &post.height_fog),
            ("depth_fog", "深度雾", &post.depth_fog),
            ("depth_of_field", "景深", &post.depth_of_field),
            ("bloom", "辉光", &post.bloom),
            ("shadows", "阴影", &post.shadows),
            ("ssao", "环境光遮蔽", &post.ssao),
            ("gaussian_blur", "高斯模糊", &post.gaussian_blur),
        ] {
            if let Some(child) = self.stage_record(node, name, record, base) {
                self.set_address(child, node, [Key("post_process".into()), Key(key.into())]);
            }
        }
        let tone = &post.tone_mapping;
        self.field(node, "色调映射点数", tone.count, base + tone.offset, 1);
        for (index, record) in tone.points.iter().enumerate() {
            if let Some(child) =
                self.stage_record(node, format!("色调映射点 {index}"), record, base)
            {
                self.set_address(
                    child,
                    node,
                    [
                        Key("post_process".into()),
                        Key("tone_mapping".into()),
                        Key("points".into()),
                        Index(index as u32),
                    ],
                );
            }
        }
        if let Some(child) = self.stage_record(node, "色调映射标记", &tone.flags, base) {
            self.set_address(
                child,
                node,
                [
                    Key("post_process".into()),
                    Key("tone_mapping".into()),
                    Key("flags".into()),
                ],
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{Document, expand, inspect};
    use std::sync::Arc;

    fn check_document_ranges(document: &Document) {
        for (index, node) in document.nodes.iter().enumerate() {
            assert_eq!(document.bytes(index).unwrap().len(), node.range.len());
            for &child in &node.children {
                assert!(child < document.nodes.len());
            }
            for field in &node.fields {
                assert!(
                    document.buffers[node.buffer]
                        .get(
                            field.binding.range.start
                                ..field.binding.range.start + field.binding.range.len()
                        )
                        .is_some(),
                    "{}: {}",
                    node.name,
                    field.name
                );
            }
        }
    }

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; verifies stage field offsets in original resources"]
    fn original_stage_resources_expand_without_copying_or_rebasing_source_fields() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        for (path, required) in [
            ("dat/stage-hd/st001-hd.pac", Kind::StageLighting),
            ("dat/stage-hd/st002-hd.pac", Kind::StageLighting),
            ("dat/stage-hd/st105-hd.pac", Kind::StageLighting),
            ("dat/stage-hd/st255-hd.pac", Kind::StageRenderTables),
            ("dat/stage/st001.pac", Kind::StagePlacements),
            ("dat/stage/nso0309.pac", Kind::Hits),
            ("dat/stage/nso0200.pac", Kind::KeyEffects),
        ] {
            let source: Arc<[u8]> = std::fs::read(root.join(path)).unwrap().into();
            let document = inspect(path, source.clone());
            assert!(Arc::ptr_eq(&source, &document.buffers[0]));
            assert!(
                document.nodes.iter().any(|node| node.kind == required),
                "{path}: {required:?}"
            );
            if path == "dat/stage/st001.pac" {
                for kind in [
                    Kind::LegacyStageLighting,
                    Kind::LegacyStageRenderTables,
                    Kind::StageAreaCamera,
                ] {
                    assert!(
                        document.nodes.iter().any(|node| node.kind == kind),
                        "{path}: {kind:?}"
                    );
                }
                assert!(document.nodes.iter().all(|node| node.kind != Kind::Unknown));
            }
            check_document_ranges(&document);
            for (index, node) in document.nodes.iter().enumerate().filter(|(_, node)| {
                matches!(
                    node.kind,
                    Kind::StageLighting
                        | Kind::StageRenderTables
                        | Kind::StagePlacements
                        | Kind::Hits
                        | Kind::KeyEffects
                        | Kind::LegacyStageLighting
                        | Kind::LegacyStageRenderTables
                        | Kind::StageAreaCamera
                )
            }) {
                assert!(node.error.is_none(), "{path}: {:?}", node.error);
                let expanded = expand(&document, index).unwrap();
                assert_eq!(expanded.bytes(index), document.bytes(index));
                assert_eq!(expanded.buffers.len(), document.buffers.len());
                assert!(
                    expanded
                        .buffers
                        .iter()
                        .zip(&document.buffers)
                        .all(|(a, b)| Arc::ptr_eq(a, b))
                );
                check_document_ranges(&expanded);
                assert!(
                    expanded.nodes[document.nodes.len()..]
                        .iter()
                        .all(|node| node.error.is_none()),
                    "{path}: expansion failed"
                );
                if node.kind == Kind::StageRenderTables {
                    for child in &expanded.nodes[index].children {
                        for record in &expanded.nodes[*child].children {
                            let record = &expanded.nodes[*record];
                            for field in &record.fields {
                                if field.name == "目标动画记录" && !field.binding.range.is_empty()
                                {
                                    assert!(!field.writable);
                                    assert_eq!(field.binding.format, FieldType::ReadOnly);
                                    assert!(field.binding.range.start >= node.range.start);
                                    assert!(field.binding.range.end <= node.range.end);
                                } else {
                                    assert!(
                                        field.binding.range.start >= record.range.start
                                            && field.binding.range.end <= record.range.end
                                    );
                                }
                            }
                        }
                    }
                }
            }
            for (index, node) in document
                .nodes
                .iter()
                .enumerate()
                .filter(|(_, node)| node.kind == Kind::Stage)
            {
                let archive = mhf_resource::container::StageArchive::probe(
                    document.bytes(index).unwrap(),
                    node.range.len(),
                )
                .unwrap();
                for (item, &child) in archive.entries.iter().zip(&node.children).skip(3) {
                    let field = document.nodes[child]
                        .fields
                        .iter()
                        .find(|field| field.name == "resource_id")
                        .unwrap();
                    assert_eq!(
                        (field.binding.range.start, field.binding.range.len()),
                        (node.range.start + 28 + (item.entry.index - 3) * 12, 4)
                    );
                    let bytes = &document.buffers[node.buffer]
                        [field.binding.range.start..field.binding.range.start + 4];
                    assert_eq!(
                        u32::from_le_bytes(bytes.try_into().unwrap()),
                        item.resource_id.unwrap()
                    );
                }
            }
        }
    }
}
