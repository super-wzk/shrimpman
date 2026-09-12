//! Old stage formats are selected by their native container roles, not by a
//! filename, isolated payload length, or the newer HD lighting/table layout.

use super::{Builder, Hint, Kind, hex};
use crate::field::{FieldType, ScalarType, formatted, typed};
use mhf_resource::{
    container::SimpleArchive,
    stage::{LegacyLighting, LegacyRenderTables},
};

impl Builder {
    /// 1089EF20 selects geometry slot 0 or 1, then 108E0460 reads its third
    /// member as environment data. The same outer stage container supplies TXB
    /// slot 3 and area-camera slot 29. Slot 31's independently validated Stage
    /// directory and the geometry/skeleton pair establish this context.
    pub(super) fn inspect_legacy_stage_contexts(&mut self) {
        let containers: Vec<_> = self
            .document
            .nodes
            .iter()
            .enumerate()
            .filter_map(|(index, node)| (node.kind == Kind::Archive).then_some(index))
            .collect();
        for container in containers {
            let children = self.document.nodes[container].children.clone();
            if children.len() < 32 {
                continue;
            }
            let owner = &self.document.nodes[container];
            let buffer = self.document.buffers[owner.buffer].clone();
            let Ok(directory) =
                SimpleArchive::parse(&buffer[owner.range.clone()], owner.range.len())
            else {
                continue;
            };
            if directory.table_offset != 4 || directory.entries.len() != children.len() {
                continue;
            }
            let Some(stage) = self.document.payload(children[31]) else {
                continue;
            };
            if self.document.nodes[stage].kind != Kind::Stage
                || self.document.nodes[stage].error.is_some()
            {
                continue;
            }
            let mut environments = Vec::new();
            for &candidate in &children[..2] {
                let Some(geometry) = self.document.payload(candidate) else {
                    continue;
                };
                let geometry = &self.document.nodes[geometry];
                if geometry.kind != Kind::Archive
                    || geometry.children.len() != 3
                    || geometry.error.is_some()
                {
                    continue;
                }
                let (Some(model), Some(skeleton), Some(environment)) = (
                    self.document.payload(geometry.children[0]),
                    self.document.payload(geometry.children[1]),
                    self.document.payload(geometry.children[2]),
                ) else {
                    continue;
                };
                if self.document.nodes[model].kind == Kind::Fmod
                    && self.document.nodes[model].error.is_none()
                    && self.document.nodes[skeleton].kind == Kind::Fskl
                    && self.document.nodes[skeleton].error.is_none()
                    && !self.document.nodes[environment].range.is_empty()
                {
                    environments.push(environment);
                }
            }
            if environments.is_empty() {
                continue;
            }
            for node in environments {
                if self.document.nodes[node].kind == Kind::LegacyStageLighting {
                    continue;
                }
                let owner = &self.document.nodes[node];
                let range = owner.range.clone();
                let buffer = self.document.buffers[owner.buffer].clone();
                self.document.nodes[node].kind = Kind::LegacyStageLighting;
                match LegacyLighting::parse(&buffer[range.clone()]) {
                    Ok(file) => self.legacy_stage_lighting(node, &file, range.start),
                    Err(error) => self.fail(node, error.to_string()),
                }
            }
            if let Some(texture) = self.document.payload(children[3])
                && self.document.nodes[texture].kind == Kind::Unknown
                && self
                    .document
                    .bytes(texture)
                    .is_some_and(|bytes| bytes == [0; 4])
            {
                self.inspect_node(
                    texture,
                    Hint {
                        directory: true,
                        txb: true,
                        ..Hint::default()
                    },
                );
            }
            if let Some(camera) = self.document.payload(children[29])
                && !self.document.nodes[camera].range.is_empty()
                && self.document.nodes[camera].kind != Kind::StageAreaCamera
            {
                let owner = &self.document.nodes[camera];
                let range = owner.range.clone();
                let buffer = self.document.buffers[owner.buffer].clone();
                self.inspect_stage_area_camera(camera, &buffer[range.clone()], range.start);
            }
        }
    }

    fn legacy_stage_lighting(&mut self, node: usize, file: &LegacyLighting<'_>, base: usize) {
        self.field(node, "版本", file.version, base, 1);
        self.field(node, "unknown_01", file.unknown_01, base + 1, 1);
        self.field(
            node,
            "color_02",
            formatted(file.color_02, format!("{:#010X}", file.color_02)),
            base + 2,
            4,
        );
        for (offset, bits) in [(6, file.value_06_bits), (10, file.value_0a_bits)] {
            self.field(
                node,
                format!("value_{offset:02X}"),
                typed(
                    format!("{} ({bits:#010X})", f32::from_bits(bits)),
                    FieldType::Scalar(ScalarType::F32),
                ),
                base + offset,
                4,
            );
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
        self.document.nodes[node].deferred = true;
    }

    pub(super) fn legacy_stage_lighting_details(
        &mut self,
        node: usize,
        file: &LegacyLighting<'_>,
        base: usize,
    ) {
        for (group, vectors) in file.vector_groups_0e_bits.iter().enumerate() {
            for (index, bits) in vectors.iter().enumerate() {
                let offset = 14 + group * 36 + index * 12;
                self.field(
                    node,
                    format!("向量组 {group} · {index}"),
                    formatted(
                        bits.map(f32::from_bits),
                        format!("{:?} · {bits:08X?}", bits.map(f32::from_bits)),
                    ),
                    base + offset,
                    12,
                );
            }
        }
        if let Some(extension) = &file.extension {
            for (index, bits) in extension.words_00.iter().enumerate() {
                self.field(
                    node,
                    format!("扩展 word_{:02X}", index * 4),
                    formatted(bits, format!("{bits:#010X}")),
                    base + 122 + index * 4,
                    4,
                );
            }
            for (index, bits) in extension.color_bits_10.iter().enumerate() {
                self.field(
                    node,
                    format!("扩展 color_{:02X}", 16 + index * 4),
                    formatted(bits, format!("{bits:#010X}")),
                    base + 138 + index * 4,
                    4,
                );
            }
        }
        if let Some(tables) = &file.tables {
            self.field(node, "24字节记录数", tables.count_aa, base + 170, 4);
            self.field(
                node,
                "16字节记录数",
                tables.count_16,
                base + tables.count_16_offset,
                4,
            );
            let buffer = self.document.nodes[node].buffer;
            for (index, record) in tables.records_24.iter().enumerate() {
                let at = base + record.offset;
                let Some(child) = self.child(
                    node,
                    format!("24字节记录 {index}"),
                    Kind::Block,
                    buffer,
                    at..at + 24,
                ) else {
                    break;
                };
                for (word, bits) in record.words.iter().enumerate() {
                    self.field(
                        child,
                        format!("word_{:02X}", word * 4),
                        formatted(bits, format!("{bits:#010X}")),
                        at + word * 4,
                        4,
                    );
                }
                self.field(child, "short_14", record.shorts[0], at + 20, 2);
                self.field(child, "short_16", record.shorts[1], at + 22, 2);
            }
            for (index, record) in tables.records_16.iter().enumerate() {
                let at = base + record.offset;
                let Some(child) = self.child(
                    node,
                    format!("16字节记录 {index}"),
                    Kind::Block,
                    buffer,
                    at..at + 16,
                ) else {
                    break;
                };
                for (word, bits) in record.words.iter().enumerate() {
                    self.field(
                        child,
                        format!("word_{:02X}", word * 4),
                        formatted(bits, format!("{bits:#010X}")),
                        at + word * 4,
                        4,
                    );
                }
            }
        }
    }

    pub(super) fn inspect_legacy_stage_render_tables(
        &mut self,
        node: usize,
        bytes: &[u8],
        base: usize,
    ) {
        self.document.nodes[node].kind = Kind::LegacyStageRenderTables;
        match LegacyRenderTables::parse(bytes) {
            Ok(file) => {
                self.field(node, "版本", file.version, base, 2);
                self.field(node, "control", file.control, base + 14, 2);
                for (index, table) in file.tables.iter().enumerate() {
                    self.field(
                        node,
                        format!("表 {index} 记录数"),
                        table.count,
                        base + table.count_offset,
                        2,
                    );
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
                self.document.nodes[node].deferred =
                    file.tables.iter().any(|table| table.count != 0);
            }
            Err(error) => self.fail(node, error.to_string()),
        }
    }

    pub(super) fn legacy_stage_render_details(
        &mut self,
        node: usize,
        file: &LegacyRenderTables<'_>,
        base: usize,
    ) {
        let buffer = self.document.nodes[node].buffer;
        for (index, table) in file.tables.iter().enumerate() {
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
            for (index, record) in table.records().enumerate() {
                let words: Vec<_> = record
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|word| u32::from_le_bytes(*word))
                    .collect();
                self.field(
                    child,
                    format!("记录 {index}"),
                    formatted(&words, format!("{words:08X?}")),
                    at + index * table.record_size,
                    record.len(),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::inspect;
    use mhf_resource::stage::AreaCamera;
    use std::sync::Arc;

    #[test]
    #[ignore = "requires MHF_RESOURCE_GAME_ROOT; verifies repeated legacy detail expansion"]
    fn original_legacy_details_are_not_reintroduced_by_context_postprocessing() {
        let root = std::path::PathBuf::from(std::env::var_os("MHF_RESOURCE_GAME_ROOT").unwrap());
        let source = std::fs::read(root.join("dat/stage/st001.pac")).unwrap();
        let document = inspect("st001.pac", source.into());
        for kind in [Kind::LegacyStageLighting, Kind::StageAreaCamera] {
            let node = document
                .nodes
                .iter()
                .position(|node| node.kind == kind)
                .unwrap();
            assert!(document.nodes[node].deferred);
            let expanded = crate::inspect::expand(&document, node).unwrap();
            assert!(!expanded.nodes[node].deferred);
            for (index, before) in document.nodes.iter().enumerate() {
                if index != node {
                    assert_eq!(expanded.nodes[index].fields.len(), before.fields.len());
                    assert_eq!(expanded.nodes[index].deferred, before.deferred);
                }
            }
            let repeated = crate::inspect::expand(&expanded, node).unwrap();
            assert_eq!(repeated.nodes.len(), expanded.nodes.len());
            assert_eq!(
                repeated.nodes[node].fields.len(),
                expanded.nodes[node].fields.len()
            );
            assert_eq!(repeated.buffers.len(), document.buffers.len());
        }
    }

    #[test]
    fn bare_legacy_payloads_are_not_identified_from_names_or_lengths() {
        let mut lighting = vec![0; 170];
        lighting[0] = 2;
        lighting[2..6].copy_from_slice(&0xff2f_2320_u32.to_le_bytes());
        lighting[10..14].copy_from_slice(&60_000.0_f32.to_bits().to_le_bytes());
        assert!(LegacyLighting::parse(&lighting).is_ok());

        // Native 1082F740's empty region/grid header is a valid area camera,
        // but still needs its containing stage's member-29 context.
        let mut camera = vec![0; 48];
        camera[..2].copy_from_slice(&0x0102_u16.to_le_bytes());
        assert!(AreaCamera::parse(&camera).is_ok());
        let empty_txb = vec![0; 4];
        assert_eq!(SimpleArchive::parse(&empty_txb, 1).unwrap().count, 0);

        for bytes in [lighting, empty_txb, camera] {
            let source: Arc<[u8]> = bytes.into();
            for name in ["renamed.bin", "stage/st001.pac", "motion/evcam0001.bin"] {
                let document = inspect(name, source.clone());
                assert_eq!(document.nodes[document.root].kind, Kind::Unknown, "{name}");
                assert_eq!(document.bytes(document.root).unwrap(), source.as_ref());
                assert!(Arc::ptr_eq(&document.buffers[0], &source));
            }
        }
    }
}
