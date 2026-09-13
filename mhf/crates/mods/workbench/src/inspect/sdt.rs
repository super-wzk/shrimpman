use super::{Builder, Kind, hex};
use mhf_resource::sdt::{
    DIRECTORY_STRIDE, FieldLayout, HITBOX_GROUP_STRIDE, HITBOX_SLOTS, HITBOX_STRIDE, Sdt, TableKind,
};
use std::{ops::Range, sync::Arc};

#[cfg(test)]
mod tests;

struct SdtSource {
    buffer: usize,
    range: Range<usize>,
    bytes: Arc<[u8]>,
}

impl Builder {
    pub(super) fn inspect_sdt(&mut self, node: usize, bytes: &[u8], base: usize) {
        if let Err(error) = self.sdt_contents(node, bytes, base) {
            self.fail(node, error);
        }
    }

    fn sdt_contents(&mut self, node: usize, bytes: &[u8], base: usize) -> Result<(), String> {
        let file = Sdt::parse(bytes).map_err(|error| error.to_string())?;
        self.field(node, "目录项数", file.entries().len(), base, 0);
        self.field(
            node,
            "目录顺序",
            "保留文件顺序；原生加载后会另行排序",
            base,
            0,
        );
        self.field(
            node,
            "参数范围",
            "攻击参数、辅助参数、分组判定与附加参数；未知字段保留原值",
            base,
            0,
        );
        let buffer = self.document.nodes[node].buffer;
        for entry in file.entries() {
            let at = base + entry.offset;
            let Some(child) = self.child(
                node,
                format!(
                    "目录 {:03} · 类别 {} / 子类别 {}",
                    entry.index, entry.kind, entry.subtype
                ),
                Kind::SdtEntry(entry.index),
                buffer,
                at..at + DIRECTORY_STRIDE,
            ) else {
                return Ok(());
            };
            for (offset, name) in [
                (0, "子类别键"),
                (2, "类别键"),
                (4, "攻击记录数 / 辅助索引上限"),
                (6, "判定组数"),
            ] {
                self.read::<u16>(child, name, at + offset)?;
            }
            for (offset, name) in [
                (8, "攻击表偏移"),
                (12, "辅助表偏移"),
                (16, "判定组目录偏移"),
                (20, "附加表偏移"),
                (24, "附加记录数"),
            ] {
                self.read::<u32>(child, name, at + offset)?;
            }
            self.document.nodes[child].deferred = true;
        }
        let terminator = file.terminator();
        if let Some(child) = self.child(
            node,
            "目录终止记录",
            Kind::Block,
            buffer,
            base + terminator.start..base + terminator.end,
        ) {
            self.field(
                child,
                "原始终止记录",
                hex(&bytes[terminator.clone()]),
                base + terminator.start,
                terminator.len(),
            );
        }
        if let Some(child) = self.child(
            node,
            "未归属数据区",
            Kind::SdtUnclaimed,
            buffer,
            base..base + bytes.len(),
        ) {
            self.field(
                child,
                "范围说明",
                "展开后列出未被有效目录、表及终止记录覆盖的字节；不推断记录含义",
                base,
                0,
            );
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    fn sdt_source(&self, node: usize) -> Result<SdtSource, String> {
        let owner = self
            .sdt_ancestor(node, |kind| kind == Kind::Sdt)
            .ok_or("SDT 数据缺少所属文件")?;
        let root = &self.document.nodes[owner];
        Ok(SdtSource {
            buffer: root.buffer,
            range: root.range.clone(),
            bytes: self.document.buffers[root.buffer].clone(),
        })
    }

    fn sdt_ancestor(&self, node: usize, matches: impl Fn(Kind) -> bool) -> Option<usize> {
        let mut parent = self.parents[node];
        while let Some(index) = parent {
            if matches(self.document.nodes[index].kind) {
                return Some(index);
            }
            parent = self.parents[index];
        }
        None
    }

    fn sdt_entry_index(&self, node: usize) -> Result<usize, String> {
        let entry = self
            .sdt_ancestor(node, |kind| matches!(kind, Kind::SdtEntry(_)))
            .ok_or("SDT 数据缺少所属目录项")?;
        let Kind::SdtEntry(index) = self.document.nodes[entry].kind else {
            unreachable!()
        };
        Ok(index)
    }

    pub(super) fn sdt_entry_tables(&mut self, node: usize, index: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        for (table_kind, kind, label, field) in [
            (TableKind::Attack, Kind::SdtAttackTable, "攻击参数", 8),
            (
                TableKind::Auxiliary,
                Kind::SdtAuxiliaryTable,
                "辅助参数",
                12,
            ),
            (TableKind::Extra, Kind::SdtExtraTable, "附加参数", 20),
        ] {
            let table = file.table(entry, table_kind);
            let range = match &table {
                Ok(Some(table)) => table.range.clone(),
                _ => entry.offset + field..entry.offset + field + 4,
            };
            let Some(child) = self.child(
                node,
                label,
                kind,
                source.buffer,
                base + range.start..base + range.end,
            ) else {
                return Ok(());
            };
            self.read::<u32>(child, "表偏移", base + entry.offset + field)?;
            match table {
                Ok(Some(table)) => {
                    self.field(
                        child,
                        if table_kind == TableKind::Auxiliary {
                            "原生索引上限"
                        } else {
                            "记录数"
                        },
                        table.count,
                        base + range.start,
                        0,
                    );
                    self.field(child, "记录步长", table.stride, base + range.start, 0);
                    if table_kind == TableKind::Auxiliary {
                        self.field(
                            child,
                            "浏览范围",
                            "按原生读取上限展示；引用可能共享或与其他区域重叠",
                            base + range.start,
                            0,
                        );
                    }
                    self.document.nodes[child].deferred = table.count != 0;
                }
                Ok(None) => self.field(child, "引用", "空引用", base + range.start, 0),
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        let groups = file.hitbox_groups(entry);
        let range = match &groups {
            Ok(Some(range)) => range.clone(),
            _ => entry.offset + 16..entry.offset + 20,
        };
        if let Some(child) = self.child(
            node,
            "分组判定",
            Kind::SdtCollisionGroups,
            source.buffer,
            base + range.start..base + range.end,
        ) {
            self.read::<u32>(child, "组目录偏移", base + entry.offset + 16)?;
            self.field(
                child,
                "组数",
                entry.hitbox_group_count,
                base + entry.offset + 6,
                2,
            );
            match groups {
                Ok(Some(_)) => self.document.nodes[child].deferred = entry.hitbox_group_count != 0,
                Ok(None) => self.field(child, "引用", "空引用", base + range.start, 0),
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        Ok(())
    }

    pub(super) fn sdt_table_records(&mut self, node: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let index = self.sdt_entry_index(node)?;
        let (table_kind, record_kind) = match self.document.nodes[node].kind {
            Kind::SdtAttackTable => (TableKind::Attack, Kind::SdtAttack),
            Kind::SdtAuxiliaryTable => (TableKind::Auxiliary, Kind::SdtAuxiliary),
            Kind::SdtExtraTable => (TableKind::Extra, Kind::SdtExtra),
            _ => return Err("SDT 表类型无效".into()),
        };
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        let table = file
            .table(entry, table_kind)
            .map_err(|error| error.to_string())?;
        let Some(table) = table else {
            return Ok(());
        };
        for index in 0..table.count {
            let record = table.record(index).map_err(|error| error.to_string())?;
            let at = base + record.offset;
            let Some(child) = self.child(
                node,
                format!("记录 {index:05}"),
                record_kind,
                source.buffer,
                at..at + record.as_bytes().len(),
            ) else {
                break;
            };
            self.field(child, "记录编号", index, at, 0);
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    pub(super) fn sdt_record_fields(&mut self, node: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let index = self.sdt_entry_index(node)?;
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        let offset = self.document.nodes[node].range.start - base;
        let record = match self.document.nodes[node].kind {
            Kind::SdtAttack | Kind::SdtAuxiliary | Kind::SdtExtra => {
                let kind = match self.document.nodes[node].kind {
                    Kind::SdtAttack => TableKind::Attack,
                    Kind::SdtAuxiliary => TableKind::Auxiliary,
                    _ => TableKind::Extra,
                };
                let table = file
                    .table(entry, kind)
                    .map_err(|error| error.to_string())?
                    .ok_or("SDT 记录表为空")?;
                let index = offset
                    .checked_sub(table.range.start)
                    .ok_or("SDT 记录位于表之前")?
                    / table.stride;
                table.record(index).map_err(|error| error.to_string())?
            }
            Kind::SdtCollision => {
                let group_node = self
                    .sdt_ancestor(node, |kind| matches!(kind, Kind::SdtCollisionGroup(_)))
                    .ok_or("SDT 判定记录缺少所属组")?;
                let list_node = self
                    .sdt_ancestor(node, |kind| matches!(kind, Kind::SdtCollisionList(_)))
                    .ok_or("SDT 判定记录缺少所属表")?;
                let Kind::SdtCollisionGroup(group_index) = self.document.nodes[group_node].kind
                else {
                    unreachable!()
                };
                let Kind::SdtCollisionList(slot) = self.document.nodes[list_node].kind else {
                    unreachable!()
                };
                let group = file
                    .hitbox_group(entry, group_index)
                    .map_err(|error| error.to_string())?;
                let list = file
                    .hitboxes(&group, slot)
                    .map_err(|error| error.to_string())?;
                let index = offset
                    .checked_sub(list.range.start)
                    .ok_or("SDT 判定记录位于表之前")?
                    / HITBOX_STRIDE;
                list.record(index).map_err(|error| error.to_string())?
            }
            _ => return Err("SDT 记录类型无效".into()),
        };
        self.sdt_fields(
            node,
            record.as_bytes(),
            base + record.offset,
            record.fields(),
        )
    }

    fn sdt_fields(
        &mut self,
        node: usize,
        bytes: &[u8],
        base: usize,
        fields: &[FieldLayout],
    ) -> Result<(), String> {
        let mut covered = vec![false; bytes.len()];
        for field in fields {
            let start = usize::from(field.offset);
            let end = start + field.scalar.size();
            covered
                .get_mut(start..end)
                .ok_or("SDT 字段超出记录范围")?
                .fill(true);
            self.read_scalar(node, field.name, base + start, field.scalar)?;
        }
        let mut offset = 0;
        while offset < covered.len() {
            if covered[offset] {
                offset += 1;
                continue;
            }
            let start = offset;
            while offset < covered.len() && !covered[offset] {
                offset += 1;
            }
            self.field(
                node,
                format!("未定义字段 {start:#04X}"),
                hex(&bytes[start..offset]),
                base + start,
                offset - start,
            );
        }
        Ok(())
    }

    pub(super) fn sdt_hitbox_groups(&mut self, node: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let index = self.sdt_entry_index(node)?;
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        for index in 0..usize::from(entry.hitbox_group_count) {
            let group = file
                .hitbox_group(entry, index)
                .map_err(|error| error.to_string())?;
            let at = base + group.offset;
            let Some(child) = self.child(
                node,
                format!("判定组 {index:04}"),
                Kind::SdtCollisionGroup(index),
                source.buffer,
                at..at + HITBOX_GROUP_STRIDE,
            ) else {
                break;
            };
            for slot in 0..HITBOX_SLOTS {
                self.read::<u32>(child, format!("槽 {slot} 偏移"), at + slot * 4)?;
            }
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    pub(super) fn sdt_hitbox_lists(
        &mut self,
        node: usize,
        group_index: usize,
    ) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let index = self.sdt_entry_index(node)?;
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        let group = file
            .hitbox_group(entry, group_index)
            .map_err(|error| error.to_string())?;
        for slot in 0..HITBOX_SLOTS {
            let list = file.hitboxes(&group, slot);
            let range = match &list {
                Ok(list) => list.range.clone(),
                _ => group.offset + slot * 4..group.offset + slot * 4 + 4,
            };
            let Some(child) = self.child(
                node,
                format!("判定槽 {slot}"),
                Kind::SdtCollisionList(slot),
                source.buffer,
                base + range.start..base + range.end,
            ) else {
                break;
            };
            self.read::<u32>(child, "记录表偏移", base + group.offset + slot * 4)?;
            match list {
                Ok(list) => {
                    self.field(child, "判定数", list.count, base + range.start, 0);
                    self.field(
                        child,
                        "终止记录",
                        hex(&file.as_bytes()[list.terminator.clone()]),
                        base + list.terminator.start,
                        list.terminator.len(),
                    );
                    self.document.nodes[child].deferred = list.count != 0;
                }
                Err(error) => self.fail(child, error.to_string()),
            }
        }
        Ok(())
    }

    pub(super) fn sdt_hitbox_records(&mut self, node: usize, slot: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let index = self.sdt_entry_index(node)?;
        let group_node = self
            .sdt_ancestor(node, |kind| matches!(kind, Kind::SdtCollisionGroup(_)))
            .ok_or("SDT 判定表缺少所属组")?;
        let Kind::SdtCollisionGroup(group_index) = self.document.nodes[group_node].kind else {
            unreachable!()
        };
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        let entry = file.entry(index).map_err(|error| error.to_string())?;
        let group = file
            .hitbox_group(entry, group_index)
            .map_err(|error| error.to_string())?;
        let list = file
            .hitboxes(&group, slot)
            .map_err(|error| error.to_string())?;
        for index in 0..list.count {
            let record = list.record(index).map_err(|error| error.to_string())?;
            let at = base + record.offset;
            let Some(child) = self.child(
                node,
                format!("判定 {index:04}"),
                Kind::SdtCollision,
                source.buffer,
                at..at + record.as_bytes().len(),
            ) else {
                break;
            };
            self.field(child, "判定编号", index, at, 0);
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    pub(super) fn sdt_unclaimed(&mut self, node: usize) -> Result<(), String> {
        let source = self.sdt_source(node)?;
        let base = source.range.start;
        let file =
            Sdt::parse(&source.bytes[source.range.clone()]).map_err(|error| error.to_string())?;
        for range in file.unclaimed_ranges() {
            let Some(child) = self.child(
                node,
                format!("未归属 {:#X} · {} 字节", range.start, range.len()),
                Kind::Block,
                source.buffer,
                base + range.start..base + range.end,
            ) else {
                break;
            };
            self.field(
                child,
                "原始数据",
                hex(&file.as_bytes()[range.clone()]),
                base + range.start,
                range.len(),
            );
        }
        Ok(())
    }
}
