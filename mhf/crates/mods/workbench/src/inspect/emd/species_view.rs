use super::{
    Builder, Emd, Kind, ROOT_LABELS, RecordKind, SpeciesTable, relations,
    script_links::ScriptSource,
};

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

        if let Some(group) = self.emd_group(node, "AI 脚本") {
            self.emd_species_ai(group, &file, base, species);
            if self.document.nodes[group].children.is_empty() {
                self.field(group, "脚本", "无可显示的关联脚本", base, 0);
            }
        }
        for (name, slots) in [
            ("基本参数", &[2, 5, 21][..]),
            ("部位参数", &[11, 12]),
            ("参数配置", &[1, 4, 10]),
            ("条件与修正", &[7, 13, 16, 18, 19, 22]),
        ] {
            let Some(group) = self.emd_group(node, name) else {
                break;
            };
            for &slot in slots {
                self.emd_species_relations(group, &file, base, species, slot);
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
        let index = relations::script_links(file);
        for slot in [9, 17] {
            let mut records = index.records_for(slot, Some(species)).peekable();
            let mut errors = index
                .errors
                .iter()
                .filter(|error| error.slot == slot)
                .peekable();
            if records.peek().is_none() && errors.peek().is_none() {
                continue;
            }
            let Some(group) = self.emd_group(parent, ROOT_LABELS[slot]) else {
                return;
            };
            for record in records {
                let reference = record.reference;
                let label = if slot == 9 {
                    format!("脚本 {:03}", reference.record)
                } else {
                    format!("记录 {:03}", reference.record)
                };
                let node = match self.emd_record_node(group, file, base, reference, &label) {
                    Ok(node) => node,
                    Err(error) => {
                        self.emd_relation_error(group, label, error);
                        continue;
                    }
                };
                if slot != 9 {
                    continue;
                }
                for link in record.links.iter().filter(|link| link.species == species) {
                    self.emd_script_source(node, link.source);
                }
            }
            for error in errors {
                self.emd_relation_error(group, ROOT_LABELS[slot], error.message.clone());
            }
        }
    }

    fn emd_script_source(&mut self, node: usize, source: ScriptSource) {
        let (name, value) = match source {
            ScriptSource::Condition {
                record,
                key,
                selector,
            } => (
                format!("条件引用 {record:03}"),
                format!("状态 {key} · 选择 {selector}"),
            ),
            ScriptSource::Native(binding) => {
                let parameter = binding
                    .parameter
                    .map_or_else(String::new, |value| format!(" · 参数 {value}"));
                (
                    format!("原生绑定 {:03} · {}", binding.record, binding.group),
                    format!("DLL 子表 1 · 槽 {}{parameter}", binding.slot),
                )
            }
        };
        let at = self.document.nodes[node].range.start;
        self.field(node, name, value, at, 0);
    }
}
