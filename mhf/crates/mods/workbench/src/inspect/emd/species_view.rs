use super::{Builder, Emd, Kind, ROOT_LABELS, RecordKind, RecordRef, SpeciesTable, relations};

impl Builder {
    pub(in crate::inspect) fn emd_species_contents(
        &mut self,
        node: usize,
        species: u8,
    ) -> Result<(), String> {
        self.emd_record_fields(node, RecordKind::Species)?;
        let root = &self.document.nodes[self.emd_root(node)?];
        let base = root.range.start;
        let buffer = self.document.buffers[root.buffer].clone();
        let file = Emd::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;

        for (name, slots) in [
            ("AI 脚本", &[9, 17][..]),
            ("基本参数", &[2, 5, 21]),
            ("部位参数", &[11, 12]),
            ("参数配置", &[1, 4, 10]),
            ("条件与修正", &[7, 13, 16, 18, 19, 22]),
        ] {
            let Some(group) = self.emd_group(node, name) else {
                break;
            };
            for &slot in slots {
                if slot == 9 {
                    self.emd_species_ai(group, &file, base, species);
                } else {
                    self.emd_species_relations(group, &file, base, species, slot);
                }
            }
            if slots == [9, 17] && self.document.nodes[group].children.is_empty() {
                self.field(group, "脚本", "无可显示的关联脚本", base, 0);
            }
        }

        for (name, count, table_kind) in [
            (
                "怒态配置",
                12,
                SpeciesTable::AngerProfile as fn(u8) -> SpeciesTable,
            ),
            ("参数组", 2, SpeciesTable::ParameterBank),
        ] {
            let Some(group) = self.emd_group(node, name) else {
                break;
            };
            for index in 0..count {
                let kind = table_kind(index);
                let label = match kind {
                    SpeciesTable::ParameterBank(bank) => format!("参数组 {bank} · 200 项"),
                    SpeciesTable::AngerProfile(profile) => format!("配置 {profile:02}"),
                };
                let Some(table) = file.species_table(species, kind).transpose() else {
                    continue;
                };
                let range = match &table {
                    Ok(table) => base + table.range.start..base + table.range.end,
                    Err(_) => self.document.nodes[node].range.clone(),
                };
                let Some(target) = self.child(
                    group,
                    label,
                    Kind::EmdSpeciesTable(species, kind),
                    self.document.nodes[node].buffer,
                    range,
                ) else {
                    break;
                };
                match table {
                    Ok(table) => self.emd_table_info(target, &table, base),
                    Err(error) => self.fail(target, error.to_string()),
                }
            }
        }
        match file.directory_table(3, usize::from(species)) {
            Ok(Some(table)) => {
                if let Some(directory) = self.child(
                    node,
                    "参数链接目录",
                    Kind::EmdTable(3, Some(usize::from(species))),
                    self.document.nodes[node].buffer,
                    base + table.range.start..base + table.range.end,
                ) {
                    self.emd_table_info(directory, &table, base);
                }
            }
            Ok(None) => {}
            Err(error) => self.emd_relation_error(node, "参数链接目录", error.to_string()),
        }
        Ok(())
    }

    fn emd_species_relations(
        &mut self,
        parent: usize,
        file: &Emd<'_>,
        base: usize,
        species: u8,
        slot: usize,
    ) {
        let related = relations::records_for_species(file, species, slot);
        if related.records.is_empty() && related.errors.is_empty() {
            return;
        }
        let direct = matches!(slot, 2 | 5 | 11 | 12 | 21);
        let group = if direct {
            parent
        } else {
            let Some(group) = self.emd_group(parent, ROOT_LABELS[slot]) else {
                return;
            };
            group
        };
        for reference in related.records {
            let label = if direct {
                ROOT_LABELS[slot].to_owned()
            } else if reference.fallback {
                format!("默认记录 {:03}", reference.record)
            } else if let Some(directory) = reference.directory {
                if slot == 16 {
                    format!("组 {directory:03} · 记录 {:03}", reference.record)
                } else {
                    format!("配置 {directory:02}")
                }
            } else {
                format!("记录 {:03}", reference.record)
            };
            if let Err(error) = self.emd_record_node(group, file, base, reference, &label) {
                self.emd_relation_error(group, label, error);
            }
        }
        for error in related.errors {
            let label = match error.directory {
                Some(index) if slot == 16 => format!("组 {index:03}"),
                Some(index) => format!("配置 {index:02}"),
                None => ROOT_LABELS[error.slot].to_owned(),
            };
            self.emd_relation_error(group, label, error.message);
        }
    }

    fn emd_relation_error(&mut self, parent: usize, name: impl Into<String>, error: String) {
        if let Some(node) = self.emd_group(parent, name) {
            self.fail(node, error);
        }
    }

    fn emd_species_ai(&mut self, parent: usize, file: &Emd<'_>, base: usize, species: u8) {
        if species != relations::ZINOGRE_SPECIES {
            return;
        }
        // 10E61520 selects these root-9 entries and 111A88A0 installs them in
        // descriptor[1][1]. These are candidates, not a simulated actor state.
        // See resource/docs/emd.md for the native predicates and call sites.
        for (name, first, parameters) in relations::ZINOGRE_AI_GROUPS {
            let Some(group) = self.emd_group(parent, name) else {
                return;
            };
            for (index, parameter) in parameters.enumerate() {
                let record = first + index;
                let label = format!("脚本 {record:03} · 参数 {parameter}");
                let reference = RecordRef {
                    slot: 9,
                    directory: None,
                    record,
                    fallback: false,
                };
                match self.emd_record_node(group, file, base, reference, &label) {
                    Ok(node) => {
                        self.field(node, "调用参数", parameter, base, 0);
                        self.field(node, "绑定位置", "DLL 子表 1 · 槽 1", base, 0);
                    }
                    Err(error) => self.emd_relation_error(group, label, error),
                }
            }
        }
    }
}
