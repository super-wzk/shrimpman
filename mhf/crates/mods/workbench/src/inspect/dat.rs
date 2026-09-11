use super::{Builder, Kind, hex};
use mhf_resource::dat::{self, Dat, RecordCount, RecordFormat, TableLayout};

include!(concat!(env!("OUT_DIR"), "/dat_text.rs"));

mod effects;
#[cfg(test)]
mod tests;

fn layout(index: usize) -> Option<&'static TableLayout> {
    dat::DATA_TABLES
        .iter()
        .chain(dat::EFFECT_TABLES)
        .chain(TEXT_TABLES)
        .nth(index)
}

fn source_text(bytes: &[u8]) -> String {
    encoding_rs::SHIFT_JIS
        .decode_without_bom_handling(bytes)
        .0
        .into_owned()
}

impl Builder {
    pub(super) fn inspect_dat(&mut self, node: usize, bytes: &[u8], base: usize) {
        let file = match Dat::parse(bytes) {
            Ok(file) => file,
            Err(error) => {
                self.fail(node, error.to_string());
                return;
            }
        };
        self.field(node, "版本", dat::VERSION, base + 4, 4);
        self.field(
            node,
            "unknown_08",
            format!("{:#010X}", file.unknown_08),
            base + 8,
            4,
        );
        self.field(node, "根结构长度", dat::HEADER_SIZE, base + 12, 4);
        self.field(
            node,
            "文本显示编码",
            "CP932（日文资源）；原始字节保留",
            base,
            0,
        );
        let buffer = self.document.nodes[node].buffer;
        for (name, tables, first_index) in [
            ("装备、物品与生产数据", dat::DATA_TABLES, 0),
            ("特效绑定与定义", dat::EFFECT_TABLES, dat::DATA_TABLES.len()),
            (
                "文本及关联记录",
                TEXT_TABLES,
                dat::DATA_TABLES.len() + dat::EFFECT_TABLES.len(),
            ),
        ] {
            let Some(group) = self.child(node, name, Kind::Block, buffer, base..base + bytes.len())
            else {
                return;
            };
            for (index, layout) in tables.iter().enumerate() {
                let table = file.table(layout);
                let range = table.as_ref().map_or(0..0, |table| table.range.clone());
                let Some(child) = self.child(
                    group,
                    layout.label,
                    Kind::DatTable(first_index + index),
                    buffer,
                    base + range.start..base + range.end,
                ) else {
                    return;
                };
                self.field(child, "表标识", layout.id, base, 0);
                self.field(child, "根字段路径", format!("{:X?}", layout.root), base, 0);
                self.field(child, "记录步长", layout.stride, base, 0);
                match table {
                    Ok(table) => {
                        self.field(child, "记录数", table.count, base + table.range.start, 0);
                        if let Some(field) = table.root_field {
                            self.field(
                                child,
                                "表偏移",
                                format!("{:#X}", file.u32(field).unwrap()),
                                base + field,
                                4,
                            );
                        }
                        if layout.first_record != 0 {
                            self.field(
                                child,
                                "物理起始记录",
                                layout.first_record,
                                base + table.range.start,
                                0,
                            );
                        }
                        if let Some(end) = table.terminator {
                            self.field(
                                child,
                                "计数终止字段",
                                hex(&bytes[end.clone()]),
                                base + end.start,
                                end.len(),
                            );
                        }
                        self.document.nodes[child].deferred = table.count != 0;
                    }
                    Err(error) => self.fail(child, error.to_string()),
                }
            }
        }
        self.dat_roots(node, &file, base);
    }

    /// Only slots explicitly relocated by 10AF5140 are followed. Scalar counts
    /// interspersed among the later roots are not mistaken for small pointers.
    fn dat_roots(&mut self, node: usize, file: &Dat<'_>, base: usize) {
        let pointer_slot = |slot: usize| {
            slot == 4
                || (13..=747).contains(&slot)
                    && !matches!(
                        slot,
                        92..=95 | 523 | 616 | 618 | 625 | 627 | 629 | 648 | 659 | 665 | 669 | 671
                    )
        };
        let buffer = self.document.nodes[node].buffer;
        let Some(group) = self.child(
            node,
            "根字段与原始数据区",
            Kind::Block,
            buffer,
            base..base + dat::HEADER_SIZE,
        ) else {
            return;
        };
        let mut boundaries = (4..dat::HEADER_SIZE / 4)
            .filter(|&slot| pointer_slot(slot))
            .filter_map(|slot| file.pointer(slot * 4).ok().flatten())
            .collect::<Vec<_>>();
        boundaries.push(file.as_bytes().len());
        boundaries.sort_unstable();
        boundaries.dedup();
        for slot in 4..dat::HEADER_SIZE / 4 {
            let field = slot * 4;
            let value = file.u32(field).unwrap();
            let Some(child) = self.child(
                group,
                format!("根字段 {field:#05X}"),
                Kind::Block,
                buffer,
                base + field..base + field + 4,
            ) else {
                return;
            };
            self.field(
                child,
                "原值",
                format!("{value} ({value:#010X})"),
                base + field,
                4,
            );
            if !pointer_slot(slot) {
                continue;
            }
            match file.pointer(field) {
                Ok(Some(start)) => {
                    let end = boundaries[boundaries.partition_point(|&offset| offset <= start)];
                    let Some(target) = self.child(
                        child,
                        format!("引用数据 {start:#X}"),
                        Kind::Block,
                        buffer,
                        base + start..base + end,
                    ) else {
                        return;
                    };
                    self.field(target, "来源根字段", format!("{field:#X}"), base + field, 4);
                    self.field(
                        target,
                        "浏览范围",
                        "从此偏移至下一根引用；不代表完整表长或记录布局",
                        base + start,
                        0,
                    );
                }
                Ok(None) => self.field(child, "引用", "空", base + field, 4),
                Err(error) => self.fail(child, error.to_string()),
            }
        }
    }

    fn dat_owner(&self, node: usize) -> Result<usize, String> {
        let mut parent = self.parents[node];
        while let Some(index) = parent {
            if self.document.nodes[index].kind == Kind::Dat {
                return Ok(index);
            }
            parent = self.parents[index];
        }
        Err("DAT 记录缺少所属数据文件".into())
    }

    pub(super) fn dat_table_records(&mut self, node: usize, index: usize) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let file = Dat::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;
        let layout = layout(index).ok_or("DAT 表布局不存在")?;
        let table = file.table(layout).map_err(|error| error.to_string())?;
        let names = match layout
            .names
            .map(|field| file.pointer(field as usize))
            .transpose()
        {
            Ok(names) => names.flatten(),
            Err(error) => {
                self.fail(node, format!("名称表：{error}"));
                None
            }
        };
        for record in 0..table.count {
            let (offset, bytes) = table.record(record).map_err(|error| error.to_string())?;
            let Some(child) = self.child(
                node,
                format!("记录 {record:05}"),
                Kind::DatRecord(index),
                buffer_index,
                base + offset..base + offset + bytes.len(),
            ) else {
                break;
            };
            self.field(child, "记录索引", record, base + offset, 0);
            if let Some(names) = names
                && let Some(name) =
                    self.dat_text_field(child, &file, names + record * 4, "名称", base)
                && !name.is_empty()
            {
                self.document.nodes[child].name = format!("{record:05} · {name}");
            }
            self.document.nodes[child].deferred = true;
        }
        Ok(())
    }

    pub(super) fn dat_record_fields(&mut self, node: usize, index: usize) -> Result<(), String> {
        let owner = self.dat_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer = self.document.buffers[root.buffer].clone();
        let base = root.range.start;
        let file = Dat::parse(&buffer[root.range.clone()]).map_err(|error| error.to_string())?;
        let range = self.document.nodes[node].range.clone();
        let bytes = &buffer[range.clone()];
        let layout = layout(index).ok_or("DAT 表布局不存在")?;
        let mut covered = vec![false; bytes.len()];
        match layout.format {
            RecordFormat::Effect(kind) => {
                return self.dat_effect_fields(node, &file, bytes, kind, base);
            }
            RecordFormat::Fields(fields) => {
                for field in fields {
                    let offset = usize::from(field.offset);
                    let size = field.scalar.size();
                    let value = field
                        .scalar
                        .read(bytes, offset)
                        .map_err(|error| error.to_string())?;
                    self.field(
                        node,
                        field.name,
                        format!("{value} · {}", hex(&bytes[offset..offset + size])),
                        range.start + offset,
                        size,
                    );
                    covered[offset..offset + size].fill(true);
                }
            }
            RecordFormat::Text { offset, parts } => {
                for part in 0..usize::from(parts) {
                    let offset = usize::from(offset) + part * 4;
                    if offset + 4 > bytes.len() {
                        return Err("DAT 文本字段超出记录".into());
                    }
                    self.dat_text_field(
                        node,
                        &file,
                        range.start - base + offset,
                        &format!("文本 {part:02}"),
                        base,
                    );
                    covered[offset..offset + 4].fill(true);
                }
            }
        }
        // Retain all unclassified bytes, including padding and non-text data
        // in the text catalog's mixed records. No zero-filled gaps are dropped.
        let mut offset = 0;
        while offset < bytes.len() {
            if covered[offset] {
                offset += 1;
                continue;
            }
            let start = offset;
            while offset < bytes.len() && !covered[offset] {
                offset += 1;
            }
            self.field(
                node,
                format!("未定义字段 {start:#04X}"),
                hex(&bytes[start..offset]),
                range.start + start,
                offset - start,
            );
        }
        Ok(())
    }

    fn dat_text_field(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        cell: usize,
        label: &str,
        base: usize,
    ) -> Option<String> {
        match file.u32(cell) {
            Ok(value) => self.field(
                node,
                format!("{label}偏移"),
                format!("{value:#X}"),
                base + cell,
                4,
            ),
            Err(error) => {
                self.fail(node, error.to_string());
                return None;
            }
        }
        match file.text(cell) {
            Ok(Some((offset, bytes))) => {
                let text = source_text(bytes);
                self.field(node, label, &text, base + offset, bytes.len() + 1);
                Some(text)
            }
            Ok(None) => {
                self.field(node, label, "空", base + cell, 4);
                None
            }
            Err(error) => {
                self.fail(node, error.to_string());
                None
            }
        }
    }
}
