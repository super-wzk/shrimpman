use super::{Builder, Kind, hex};
use mhf_resource::PathSegment::{Field as Key, Index};
use mhf_resource::{
    PathSegment,
    emd::{Emd, ROOT_LABELS, ROOT_SIZE, RecordKind, SPECIES_STRIDE, SpeciesTable, Table},
    species,
};

mod ai;
mod global_view;
mod native_script_bindings;
mod relations;
mod script_links;
mod species_view;

use relations::RecordRef;

/// Associated records can live in an unopened species branch. Locate the
/// owning branch from native references, without treating its UI ordinal as
/// part of the address or opening every species.
pub(super) fn resource_expansion(
    document: &super::Document,
    node: usize,
    target: &[PathSegment],
) -> Option<usize> {
    let [Index(slot), Index(first), rest @ ..] = target else {
        return None;
    };
    let root = document.nodes.get(node)?;
    let file = Emd::parse(document.bytes(node)?).ok()?;
    let slot = *slot as usize;
    let first = *first as usize;
    let species = match slot {
        2 | 5 | 11 | 12 | 21 => vec![u8::try_from(first).ok()?],
        1 | 4 | 10 => {
            let Index(record) = rest.first()? else {
                return None;
            };
            vec![u8::try_from(*record).ok()?]
        }
        9 => relations::script_links(&file)
            .records
            .iter()
            .filter(|record| record.reference.slot == slot && record.reference.record == first)
            .flat_map(|record| record.links.iter().map(|link| link.species))
            .collect(),
        7 | 13 | 16 | 17 | 18 | 19 | 22 => {
            let table = if slot == 16 {
                file.directory_table(slot, first).ok()??
            } else {
                file.root_table(slot).ok()??
            };
            let index = if slot == 16 {
                let Index(index) = rest.first()? else {
                    return None;
                };
                *index as usize
            } else {
                first
            };
            let (_, bytes) = table.record(index).ok()?;
            let reader = mhf_resource::binary::Reader::new(bytes);
            let species = match slot {
                7 | 17 => u16::from(reader.read_at::<u8>(0).ok()?.value),
                18 => u16::from(reader.read_at::<u8>(16).ok()?.value),
                16 => u16::try_from(reader.read_at::<i16>(16).ok()?.value).ok()?,
                19 => reader.read_at::<u16>(2).ok()?.value,
                _ => reader.read_at::<u16>(0).ok()?.value,
            };
            vec![u8::try_from(species).ok()?]
        }
        _ => return None,
    };
    root.children.iter().copied().find(|&child| {
        let value = &document.nodes[child];
        matches!(value.kind, Kind::EmdSpecies(id) if species.contains(&id)) && value.deferred
    })
}

impl Builder {
    pub(super) fn inspect_emd(&mut self, node: usize, bytes: &[u8], base: usize) {
        if let Err(error) = self.emd_contents(node, bytes, base) {
            self.fail(node, error);
        }
    }

    fn emd_contents(&mut self, node: usize, bytes: &[u8], base: usize) -> Result<(), String> {
        let file = Emd::parse(bytes).map_err(|error| error.to_string())?;
        self.read::<u8>(node, "species_slot_count", base + file.header_offset + 4)?;
        let buffer = self.document.nodes[node].buffer;
        for slot in 0..ROOT_LABELS.len() {
            self.read::<u32>(node, format!("table_{slot:02}_offset"), base + slot * 4)?;
        }
        for species in file.species() {
            let at = base + species.offset;
            let Some(child) = self.child(
                node,
                format!("物种 {:03} · {}", species.id, species::name(species.id)),
                Kind::EmdSpecies(species.id),
                buffer,
                at..at + SPECIES_STRIDE,
            ) else {
                break;
            };
            self.set_address(child, node, [Index(3), Index(u32::from(species.id))]);
            self.document.nodes[child].deferred = true;
        }
        let global_group = self.emd_group(node, "全局数据").ok_or("无法创建全局目录")?;
        for slot in 0..ROOT_LABELS.len() {
            match slot {
                // Profile directories expose shared pointers, not duplicate
                // species parameter tables. Root 15 also exposes group links.
                0 | 1 | 4 | 6 | 8 | 10 | 14 | 15 | 20 | 23 => {
                    self.emd_root_table_node(global_group, &file, base, slot);
                }
                7 | 9 | 13 | 16 | 17 | 18 | 19 | 22 => {
                    self.emd_global_table_node(global_group, &file, base, slot);
                }
                _ => {}
            }
        }
        Ok(())
    }

    fn emd_group(&mut self, parent: usize, name: impl Into<String>) -> Option<usize> {
        let owner = &self.document.nodes[parent];
        self.child(
            parent,
            name,
            Kind::EmdGroup,
            owner.buffer,
            owner.range.start..owner.range.start,
        )
    }

    fn emd_root_table_node(&mut self, parent: usize, file: &Emd<'_>, base: usize, slot: usize) {
        let table = file.root_table(slot);
        let range = table
            .as_ref()
            .ok()
            .and_then(|table| table.as_ref())
            .map_or(slot * 4..slot * 4 + 4, |table| table.range.clone());
        let label = match slot {
            1 => "部位参数配置索引",
            4 => "80 字节参数配置索引",
            10 => "90 字节参数配置索引",
            15 => "32 字节记录组索引",
            _ => ROOT_LABELS[slot],
        };
        let label = super::field_label(Kind::Emd, label);
        let Some(child) = self.child(
            parent,
            format!("{slot:02} · {label}"),
            Kind::EmdTable(slot, None),
            self.document.nodes[parent].buffer,
            base + range.start..base + range.end,
        ) else {
            return;
        };
        if let Ok(root) = self.emd_root(child) {
            self.set_address(child, root, [Index(slot as u32)]);
        }
        if let Err(error) = self.read::<u32>(child, "root_offset", base + slot * 4) {
            self.fail(child, error);
        }
        match table {
            Ok(Some(table)) => self.emd_table_info(child, &table, base),
            Ok(None) => self.field(child, "解析状态", "未解析", base + slot * 4, 0),
            Err(error) => self.fail(child, error.to_string()),
        }
    }

    fn emd_table_info(&mut self, node: usize, table: &Table<'_>, base: usize) {
        self.field(node, "记录数", table.count, base + table.range.start, 0);
        self.field(node, "步长", table.stride, base + table.range.start, 0);
        if let Some(range) = &table.terminator {
            self.field(
                node,
                "终止标记",
                hex(&self.document.buffers[self.document.nodes[node].buffer]
                    [base + range.start..base + range.end]),
                base + range.start,
                range.len(),
            );
        }
        self.document.nodes[node].deferred = table.count != 0;
    }

    pub(super) fn emd_table_records(
        &mut self,
        node: usize,
        slot: usize,
        directory: Option<usize>,
    ) -> Result<(), String> {
        let root = &self.document.nodes[self.emd_root(node)?];
        let buffer = self.document.buffers[root.buffer].clone();
        let base = root.range.start;
        let file = Emd::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;
        let table = match directory {
            Some(index) => file.directory_table(slot, index),
            None => file.root_table(slot),
        }
        .map_err(|error| error.to_string())?
        .ok_or("EMD 表不存在或暂不支持展开")?;
        for record in 0..table.count {
            self.emd_record_node(
                node,
                &file,
                base,
                RecordRef {
                    slot,
                    directory,
                    record,
                    fallback: false,
                },
                format!("记录 {record:03}"),
            )?;
        }
        Ok(())
    }

    fn emd_record_node(
        &mut self,
        parent: usize,
        file: &Emd<'_>,
        base: usize,
        reference: RecordRef,
        name: impl Into<String>,
    ) -> Result<usize, String> {
        let RecordRef {
            slot,
            directory,
            record,
            fallback,
        } = reference;
        let table = match directory {
            Some(index) => file.directory_table(slot, index),
            None => file.root_table(slot),
        }
        .map_err(|error| error.to_string())?
        .ok_or("EMD 表不存在或暂不支持展开")?;
        let (at, bytes) = table.record(record).map_err(|error| error.to_string())?;
        let buffer = self.document.nodes[parent].buffer;
        let child = self
            .child(
                parent,
                name,
                Kind::EmdRecord(table.kind),
                buffer,
                base + at..base + at + bytes.len(),
            )
            .ok_or("无法创建 EMD 记录节点")?;
        let mut coordinate = vec![Index(slot as u32)];
        if let Some(directory) = directory {
            coordinate.push(Index(directory as u32));
            if let Some(field) = match slot {
                3 => Some("parameter_links"),
                6 => Some("weighted_pairs"),
                7 => Some("probability_rows"),
                19 => Some("action_rules"),
                _ => None,
            } {
                coordinate.push(Key(field.into()));
            }
        }
        if slot != 0 {
            coordinate.push(Index(record as u32));
        }
        self.set_address(child, self.emd_root(child)?, coordinate);
        self.document.nodes[child].deferred = true;
        if fallback {
            self.field(child, "匹配方式", "默认回退", base + at, 0);
        }
        if directory.is_none() && matches!(slot, 9 | 17) {
            let pointer = if slot == 9 { 0 } else { 4 };
            let offset =
                u32::from_le_bytes(bytes[pointer..pointer + 4].try_into().unwrap()) as usize;
            if offset < ROOT_SIZE
                || (file.header_offset..file.header_offset + mhf_resource::emd::HEADER_SIZE)
                    .contains(&offset)
                || offset >= file.as_bytes().len()
            {
                self.fail(child, "脚本偏移位于根目录、头部或资源范围之外".into());
            } else if let Some(script) = self.child(
                child,
                "AI 脚本",
                Kind::EmdAiScript(record),
                buffer,
                base + offset..base + offset + 1,
            ) {
                self.set_address(script, child, [Key("script".into())]);
                self.document.nodes[script].deferred = true;
            }
        }
        if directory.is_none() && slot == 15 {
            // Group counts and pointers form a shared directory even when a
            // group is empty or every record belongs to a species.
            match file.root_table(16).and_then(|table| {
                table
                    .expect("root 16 has a fixed directory layout")
                    .record(record)
            }) {
                Ok((offset, _)) => {
                    self.read::<u32>(child, "group_offset", base + offset)?;
                }
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        if directory.is_none() && matches!(slot, 6 | 7 | 19) {
            match file.directory_table(slot, record) {
                Ok(Some(target)) => {
                    if let Some(target_node) = self.child(
                        child,
                        "引用记录表",
                        Kind::EmdTable(slot, Some(record)),
                        buffer,
                        base + target.range.start..base + target.range.end,
                    ) {
                        self.set_address(
                            target_node,
                            child,
                            [Key(match slot {
                                6 => "weighted_pairs",
                                7 => "probability_rows",
                                19 => "action_rules",
                                _ => unreachable!(),
                            }
                            .into())],
                        );
                        self.emd_table_info(target_node, &target, base);
                    }
                }
                Ok(None) => {}
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        Ok(child)
    }

    pub(super) fn emd_ai_script(&mut self, node: usize) -> Result<(), String> {
        let root = &self.document.nodes[self.emd_root(node)?];
        let range = root.range.clone();
        let buffer = self.document.buffers[root.buffer].clone();
        let offset = self.document.nodes[node].range.start - range.start;
        if let Err(error) =
            self.emd_ai_script_fields(node, &buffer[range.clone()], range.start, offset)
        {
            self.fail(node, error);
        }
        Ok(())
    }

    fn emd_root(&self, node: usize) -> Result<usize, String> {
        self.ancestor(node, |kind| kind == Kind::Emd)
            .ok_or_else(|| "EMD 节点缺少所属资源".into())
    }

    pub(super) fn emd_species_table_records(
        &mut self,
        node: usize,
        species: u8,
        kind: SpeciesTable,
    ) -> Result<(), String> {
        let root = &self.document.nodes[self.emd_root(node)?];
        let base = root.range.start;
        let buffer = root.buffer;
        let bytes = self.document.buffers[buffer].clone();
        let file = Emd::parse(&bytes[root.range.clone()]).map_err(|error| error.to_string())?;
        let table = file
            .species_table(species, kind)
            .map_err(|error| error.to_string())?
            .ok_or("EMD 物种参数表为空")?;
        for record in 0..table.count {
            let (at, bytes) = table.record(record).map_err(|error| error.to_string())?;
            let Some(child) = self.child(
                node,
                format!("记录 {record:03}"),
                Kind::EmdRecord(table.kind),
                buffer,
                base + at..base + at + bytes.len(),
            ) else {
                break;
            };
            if matches!(kind, SpeciesTable::AngerProfile(_)) {
                self.set_address(child, node, []);
            } else {
                self.set_address(child, node, [Index(record as u32)]);
            }
            self.document.nodes[child].deferred = true;
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
        self.unclassified_fields(
            node,
            &buffer[range.clone()],
            range.start,
            &covered,
            |start| format!("raw_{start:02x}"),
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests;
