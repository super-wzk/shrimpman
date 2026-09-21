use super::{Builder, Kind, hex};
use mhf_resource::{
    emd::{Emd, ROOT_LABELS, ROOT_SIZE, RecordKind, SPECIES_STRIDE, Table},
    species,
};

impl Builder {
    pub(super) fn inspect_emd(&mut self, node: usize, bytes: &[u8], base: usize) {
        if let Err(error) = self.emd_contents(node, bytes, base) {
            self.fail(node, error);
        }
    }

    fn emd_contents(&mut self, node: usize, bytes: &[u8], base: usize) -> Result<(), String> {
        let file = Emd::parse(bytes).map_err(|error| error.to_string())?;
        for offset in (0..ROOT_SIZE).step_by(4) {
            self.read::<u32>(
                node,
                format!("table_{:02}_offset", offset / 4),
                base + offset,
            )?;
        }
        self.read::<u8>(node, "species_slot_count", base + file.header_offset + 4)?;
        self.field(
            node,
            "范围",
            "资源槽不等于可生成怪物；中文名仅用于显示，未知字段及未解析表保留原文",
            base,
            0,
        );
        let buffer = self.document.nodes[node].buffer;
        for species in file.species() {
            let name = species::name(species.id);
            let at = base + species.offset;
            let Some(child) = self.child(
                node,
                format!("物种 {:03} · {name}", species.id),
                Kind::EmdSpecies,
                buffer,
                at..at + SPECIES_STRIDE,
            ) else {
                break;
            };
            self.document.nodes[child].deferred = true;
            match file.directory_table(3, usize::from(species.id)) {
                Ok(Some(table)) => {
                    if let Some(directory) = self.child(
                        child,
                        "+184 参数链接目录",
                        Kind::EmdTable(3, Some(usize::from(species.id))),
                        buffer,
                        base + table.range.start..base + table.range.end,
                    ) {
                        self.emd_table_info(directory, &table, base);
                        self.field(
                            directory,
                            "链接规则",
                            "两项均非零时原生才重定位；value_04 含义及目标布局未确认",
                            base + table.range.start,
                            0,
                        );
                    }
                }
                Ok(None) => {}
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        for (slot, label) in ROOT_LABELS.iter().enumerate() {
            if slot == 3 {
                continue;
            } // Species records are already listed above.
            let table = file.root_table(slot);
            let range = table
                .as_ref()
                .ok()
                .and_then(|table| table.as_ref())
                .map_or(slot * 4..slot * 4 + 4, |table| table.range.clone());
            let Some(child) = self.child(
                node,
                format!("{slot:02} · {label}"),
                Kind::EmdTable(slot, None),
                buffer,
                base + range.start..base + range.end,
            ) else {
                break;
            };
            self.read::<u32>(child, "root_offset", base + slot * 4)?;
            match table {
                Ok(Some(table)) => self.emd_table_info(child, &table, base),
                Ok(None) => self.field(child, "布局", "布局未确认；保留偏移，不推断长度", base, 0),
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        Ok(())
    }

    fn emd_table_info(&mut self, node: usize, table: &Table<'_>, base: usize) {
        self.field(node, "记录数", table.count, base + table.range.start, 0);
        self.field(node, "步长", table.stride, base + table.range.start, 0);
        if let Some(range) = &table.terminator {
            self.field(node, "零终止项", 0u32, base + range.start, 4);
        }
        self.document.nodes[node].deferred = table.count != 0;
    }

    pub(super) fn emd_table_records(
        &mut self,
        node: usize,
        slot: usize,
        index: Option<usize>,
    ) -> Result<(), String> {
        let mut owner = self.parents[node];
        while let Some(parent) = owner {
            if self.document.nodes[parent].kind == Kind::Emd {
                break;
            }
            owner = self.parents[parent];
        }
        let root = &self.document.nodes[owner.ok_or("EMD 表缺少所属资源")?];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let file = Emd::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;
        let table = match index {
            Some(index) => file.directory_table(slot, index),
            None => file.root_table(slot),
        }
        .map_err(|error| error.to_string())?
        .ok_or("EMD 表不存在或布局未确认")?;
        for record in 0..table.count {
            let (at, bytes) = table.record(record).map_err(|error| error.to_string())?;
            let Some(child) = self.child(
                node,
                format!("记录 {record:03}"),
                Kind::EmdRecord(table.kind),
                buffer_index,
                base + at..base + at + bytes.len(),
            ) else {
                break;
            };
            self.document.nodes[child].deferred = true;
            if index.is_none() && matches!(slot, 1 | 4 | 10 | 16 | 19) {
                // The directory field remains a separate editable record even
                // when its target is invalid or aliases another target.
                match file.directory_table(slot, record) {
                    Ok(Some(target)) => {
                        let Some(target_node) = self.child(
                            child,
                            "引用记录表",
                            Kind::EmdTable(slot, Some(record)),
                            buffer_index,
                            base + target.range.start..base + target.range.end,
                        ) else {
                            break;
                        };
                        self.emd_table_info(target_node, &target, base);
                    }
                    Ok(None) => {}
                    Err(error) => self.fail(child, error.to_string()),
                }
            }
        }
        Ok(())
    }

    pub(super) fn emd_record_fields(
        &mut self,
        node: usize,
        kind: RecordKind,
    ) -> Result<(), String> {
        let range = self.document.nodes[node].range.clone();
        let buffer = self.document.buffers[self.document.nodes[node].buffer].clone();
        let mut covered = vec![false; range.len()];
        for field in kind.fields() {
            let end = field.offset + field.scalar.size();
            if end > range.len() {
                return Err("EMD 字段超出记录".into());
            }
            self.read_scalar(node, &field.name, range.start + field.offset, field.scalar)?;
            covered[field.offset..end].fill(true);
        }
        let mut offset = 0;
        while offset < range.len() {
            if covered[offset] {
                offset += 1;
                continue;
            }
            let start = offset;
            while offset < range.len() && !covered[offset] {
                offset += 1;
            }
            let at = range.start + start;
            self.field(
                node,
                format!("raw_{start:02x}"),
                hex(&buffer[at..range.start + offset]),
                at,
                offset - start,
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inspect::{expand, inspect};

    fn sample() -> Vec<u8> {
        let mut bytes = vec![0; 144 + 178 * SPECIES_STRIDE];
        bytes[..4].copy_from_slice(&96u32.to_le_bytes());
        bytes[12..16].copy_from_slice(&144u32.to_le_bytes());
        bytes[100] = 178;
        bytes
    }

    #[test]
    fn species_parameter_directories_preserve_aliases_and_allow_scalar_edits() {
        let mut bytes = sample();
        let offset = bytes.len();
        bytes.resize(offset + 1600, 0);
        for id in [1, 2] {
            let link = 144 + id * SPECIES_STRIDE + 184;
            bytes[link..link + 4].copy_from_slice(&(offset as u32).to_le_bytes());
        }
        let document = inspect("mhfemd.bin", bytes.clone().into());
        let directories: Vec<_> = document
            .nodes
            .iter()
            .enumerate()
            .filter(|(_, node)| matches!(node.kind, Kind::EmdTable(3, Some(_))))
            .map(|(index, node)| (index, node.range.clone()))
            .collect();
        assert_eq!(directories.len(), 2);
        assert_eq!(directories[0].1, directories[1].1);
        let directory = directories[0].0;
        let document = expand(&document, directory).unwrap();
        assert_eq!(document.nodes[directory].children.len(), 200);
        let record = document.nodes[directory].children[0];
        let document = expand(&document, record).unwrap();
        let field = document.nodes[record]
            .fields
            .iter()
            .find(|field| field.name == "value_04")
            .unwrap();
        assert_eq!(field.binding.range, offset + 4..offset + 8);
        let edited = crate::edit::apply(
            &document,
            document.nodes[record].buffer,
            field.binding.range.clone(),
            &u32::MAX.to_le_bytes(),
        )
        .unwrap();
        bytes[offset + 4..offset + 8].copy_from_slice(&u32::MAX.to_le_bytes());
        assert_eq!(&edited.buffers[0][..], bytes);
    }

    #[test]
    fn named_resource_uses_count_and_preserves_unknown_species() {
        let document = inspect("mhfemd.bin", sample().into());
        assert_eq!(document.nodes[0].kind, Kind::Emd);
        assert_eq!(
            document.nodes[0]
                .children
                .iter()
                .filter(|&&node| document.nodes[node].kind == Kind::EmdSpecies)
                .count(),
            178
        );
        let first = document.nodes[0].children[1];
        assert!(document.nodes[first].name.contains("雌火龙"));
        let last = document.nodes[0].children[177];
        assert!(document.nodes[last].name.contains("em177"));
        let unnamed = document.nodes[0].children[18];
        assert!(document.nodes[unnamed].name.contains("em018"));
        let expanded = expand(&document, last).unwrap();
        assert!(
            expanded.nodes[last]
                .fields
                .iter()
                .any(|field| field.binding.range.start == 144 + 177 * SPECIES_STRIDE)
        );
    }

    #[test]
    fn encoded_resource_can_be_edited_without_losing_unknown_bytes() {
        use mhf_resource::crypto::Ecd;
        let bytes = sample();
        let mut jkr = b"JKR\x1a\x08\x01\0\0".to_vec();
        jkr.extend_from_slice(&16u32.to_le_bytes());
        jkr.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
        jkr.extend_from_slice(&bytes);
        let encoded = Ecd::parse(b"ecd\x1a\x04\0\0\0\0\0\0\0\0\0\0\0")
            .unwrap()
            .encode(&jkr, Some(b"mhfemd.bin"))
            .unwrap();
        let document = inspect("mhfemd.bin", encoded.into());
        let root = document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::Emd)
            .unwrap();
        let child = document.nodes[root].children[1];
        let document = expand(&document, child).unwrap();
        let field = document.nodes[child]
            .fields
            .iter()
            .find(|field| field.binding.range.len() == 4)
            .unwrap();
        let range = field.binding.range.clone();
        let buffer = document.nodes[child].buffer;
        let edited =
            crate::edit::apply(&document, buffer, range.clone(), &123u32.to_le_bytes()).unwrap();
        let root = edited
            .nodes
            .iter()
            .find(|node| node.kind == Kind::Emd)
            .unwrap();
        let mut expected = bytes;
        expected[range].copy_from_slice(&123u32.to_le_bytes());
        assert_eq!(&edited.buffers[root.buffer][root.range.clone()], expected);
        assert_eq!(
            root.children
                .iter()
                .filter(|&&node| edited.nodes[node].kind == Kind::EmdSpecies)
                .count(),
            178
        );
    }

    #[test]
    fn aliased_profile_tables_expand_lazily_and_edit_original_bytes() {
        let mut bytes = sample();
        let directory = bytes.len();
        let records = directory + 48;
        bytes.resize(records + 178 * 34, 0);
        bytes[4..8].copy_from_slice(&(directory as u32).to_le_bytes());
        for entry in 0..12 {
            let at = directory + 4 * entry;
            bytes[at..at + 4].copy_from_slice(&(records as u32).to_le_bytes());
        }
        bytes[records..records + 2].copy_from_slice(&321i16.to_le_bytes());
        let document = inspect("mhfemd.bin", bytes.clone().into());
        let table = document
            .nodes
            .iter()
            .position(|n| n.kind == Kind::EmdTable(1, None))
            .unwrap();
        assert!(document.nodes[table].deferred);
        assert!(document.nodes[table].children.is_empty());
        let document = expand(&document, table).unwrap();
        let entries = &document.nodes[table].children;
        assert_eq!(entries.len(), 12);
        let target = document.nodes[entries[0]].children[0];
        let alias = document.nodes[entries[1]].children[0];
        assert_eq!(document.nodes[target].range, document.nodes[alias].range);
        assert!(document.nodes[target].children.is_empty());
        let document = expand(&document, target).unwrap();
        let record = document.nodes[target].children[0];
        let document = expand(&document, record).unwrap();
        let field = document.nodes[record]
            .fields
            .iter()
            .find(|f| f.name == "part_0_initial_value")
            .unwrap();
        assert_eq!(field.binding.range, records..records + 2);
        assert_eq!(field.value, "321");
        let edited = crate::edit::apply(
            &document,
            document.nodes[record].buffer,
            field.binding.range.clone(),
            &456i16.to_le_bytes(),
        )
        .unwrap();
        bytes[records..records + 2].copy_from_slice(&456i16.to_le_bytes());
        assert_eq!(edited.buffers[0].as_ref(), bytes);
    }

    #[test]
    fn association_action_rules_expand_and_edit_without_normalizing_values() {
        let mut bytes = sample();
        let association = bytes.len();
        let rules = association + 32;
        bytes.resize(rules + 4, 0);
        bytes[76..80].copy_from_slice(&(association as u32).to_le_bytes());
        bytes[124..126].copy_from_slice(&1u16.to_le_bytes());
        bytes[association + 26..association + 28].copy_from_slice(&1u16.to_le_bytes());
        bytes[association + 28..association + 32].copy_from_slice(&(rules as u32).to_le_bytes());
        bytes[rules..].copy_from_slice(&[7, 2, 44, 1]);
        let document = inspect("mhfemd.bin", bytes.clone().into());
        let root = document
            .nodes
            .iter()
            .position(|n| n.kind == Kind::EmdTable(19, None))
            .unwrap();
        let document = expand(&document, root).unwrap();
        let association_node = document.nodes[root].children[0];
        let target = document.nodes[association_node].children[0];
        assert_eq!(document.nodes[target].range, rules..rules + 4);
        let document = expand(&document, target).unwrap();
        let record = document.nodes[target].children[0];
        let document = expand(&document, record).unwrap();
        let field = document.nodes[record]
            .fields
            .iter()
            .find(|f| f.name == "action_id")
            .unwrap();
        assert_eq!(field.value, "300");
        let edited = crate::edit::apply(
            &document,
            document.nodes[record].buffer,
            field.binding.range.clone(),
            &301u16.to_le_bytes(),
        )
        .unwrap();
        bytes[rules + 2..rules + 4].copy_from_slice(&301u16.to_le_bytes());
        assert_eq!(edited.buffers[0].as_ref(), bytes);
    }

    #[test]
    #[ignore = "requires local game resource via MHF_EMD_PATH"]
    fn real_emd_workbench_edit_preserves_payload_and_envelopes() {
        let path = std::env::var_os("MHF_EMD_PATH").expect("set MHF_EMD_PATH");
        let source = std::fs::read(path).unwrap();
        let document = inspect("mhfemd.bin", source.into());
        let root = document
            .nodes
            .iter()
            .position(|n| n.kind == Kind::Emd)
            .unwrap();
        assert!(document.nodes[root].error.is_none());
        let table = document.nodes[root]
            .children
            .iter()
            .copied()
            .find(|&n| document.nodes[n].kind == Kind::EmdTable(19, None))
            .unwrap();
        let document = expand(&document, table).unwrap();
        let record = document.nodes[table].children[0];
        let document = expand(&document, record).unwrap();
        let field = document.nodes[record]
            .fields
            .iter()
            .find(|f| f.name == "anchor_offset_x")
            .unwrap();
        let buffer = document.nodes[record].buffer;
        let range = field.binding.range.clone();
        let mut expected = document.buffers[buffer].to_vec();
        let original = i16::from_le_bytes(expected[range.clone()].try_into().unwrap());
        let replacement = original.wrapping_add(1).to_le_bytes();
        expected[range.clone()].copy_from_slice(&replacement);
        let edited = crate::edit::apply(&document, buffer, range, &replacement).unwrap();
        let reopened =
            mhf_resource::container::open_layers(&edited.buffers[0], 64 * 1024 * 1024, 8).unwrap();
        assert_eq!(reopened.payload(), expected);
        assert!(!reopened.layers.is_empty());
    }

    #[test]
    fn malformed_named_resource_is_not_silently_probed_as_an_archive() {
        let document = inspect("mhfemd.bin", vec![0; 16].into());
        assert_eq!(document.nodes[0].kind, Kind::Emd);
        assert!(document.nodes[0].children.is_empty());
        assert!(document.nodes[0].error.is_some());
    }
}
