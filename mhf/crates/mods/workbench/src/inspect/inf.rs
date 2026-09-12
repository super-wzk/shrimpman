use super::{Builder, Kind, hex};
use crate::field::{FieldType, ScalarType, TextEncoding, typed};
use mhf_resource::inf::{self, Inf};

include!(concat!(env!("OUT_DIR"), "/inf_layout.rs"));

impl Builder {
    pub(super) fn inspect_inf(&mut self, node: usize, bytes: &[u8], base: usize) {
        if let Err(error) = self.inf_contents(node, bytes, base) {
            self.fail(node, error);
        }
    }

    fn inf_contents(&mut self, node: usize, bytes: &[u8], base: usize) -> Result<(), String> {
        let file = Inf::parse(bytes, QUEST_LAYOUT).map_err(|error| error.to_string())?;
        self.read::<u32>(node, "version", base + 4)?;
        self.read::<u32>(node, "unknown_08", base + 8)?;
        self.read::<u32>(node, "header_size", base + 12)?;
        for offset in (16..inf::HEADER_SIZE).step_by(4) {
            let name = if offset == QUEST_LAYOUT.root_field as usize {
                "categories_offset".into()
            } else if offset == QUEST_LAYOUT.count_root_field as usize {
                "counts_offset".into()
            } else {
                format!("root_{offset:02x}")
            };
            self.read::<u32>(node, name, base + offset)?;
        }
        self.field(
            node,
            "静态任务查找",
            "首个任务 ID ≤ 上界或上界 0 的分类；槽 = ID % 100；40000 起使用其他资源",
            base,
            0,
        );
        self.field(
            node,
            "任务记录范围",
            format!(
                "仅解析已知的 {} 字节前缀；其余数据保留在完整 INF 原文中",
                QUEST_LAYOUT.quest_prefix_size()
            ),
            base,
            0,
        );
        if let Some(offset) = file
            .pointer(QUEST_LAYOUT.count_root_field as usize)
            .map_err(|error| error.to_string())?
        {
            self.read::<u16>(node, "category_count", base + offset)?;
        }
        let count = file.category_count().map_err(|error| error.to_string())?;
        let buffer = self.document.nodes[node].buffer;
        for index in 0..count {
            let category = file.category(index).map_err(|error| error.to_string())?;
            let at = base + category.offset;
            let Some(child) = self.child(
                node,
                format!("分类 {index:03} · ID 上界 {}", category.quest_id_limit),
                Kind::InfCategory(index),
                buffer,
                at..at + category.as_bytes().len(),
            ) else {
                break;
            };
            self.read::<u16>(child, "quest_id_limit", at)?;
            self.read::<u16>(
                child,
                "slot_count",
                at + usize::from(QUEST_LAYOUT.category_count_field),
            )?;
            self.read::<u32>(
                child,
                "slots_offset",
                at + usize::from(QUEST_LAYOUT.category_records_field),
            )?;
            if category.quest_id_limit == 0 {
                self.field(child, "查找状态", "停止扫描；仍按余数查询本分类槽表", at, 0);
            }
            self.document.nodes[child].deferred = category.slot_count != 0;
        }
        Ok(())
    }

    fn inf_owner(&self, node: usize) -> Result<usize, String> {
        let mut parent = self.parents[node];
        while let Some(index) = parent {
            if self.document.nodes[index].kind == Kind::Inf {
                return Ok(index);
            }
            parent = self.parents[index];
        }
        Err("INF 记录缺少所属数据文件".into())
    }

    pub(super) fn inf_category_records(&mut self, node: usize, index: usize) -> Result<(), String> {
        let owner = self.inf_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer_index = root.buffer;
        let buffer = self.document.buffers[buffer_index].clone();
        let base = root.range.start;
        let file = Inf::parse(&buffer[root.range.clone()], QUEST_LAYOUT)
            .map_err(|error| error.to_string())?;
        let category = file.category(index).map_err(|error| error.to_string())?;
        for index in 0..usize::from(category.slot_count) {
            let slot = file
                .slot(&category, index)
                .map_err(|error| error.to_string())?;
            let quest = (slot.offset != 0).then(|| file.quest(slot.offset));
            let (name, kind, range) = match &quest {
                Some(Ok(quest)) => (
                    format!("槽 {index:03} · 任务 {} · 已知前缀", quest.quest_id),
                    Kind::InfQuest,
                    quest.offset..quest.offset + quest.prefix().len(),
                ),
                _ => (
                    format!("槽 {index:03}"),
                    Kind::Block,
                    slot.field..slot.field + 4,
                ),
            };
            let Some(child) = self.child(
                node,
                name,
                kind,
                buffer_index,
                base + range.start..base + range.end,
            ) else {
                break;
            };
            self.read::<u32>(child, "slot_offset", base + slot.field)?;
            match quest {
                Some(Ok(_)) => self.document.nodes[child].deferred = true,
                Some(Err(error)) => self.fail(child, error.to_string()),
                None => self.field(child, "引用", "空槽", base + slot.field, 0),
            }
        }
        Ok(())
    }

    pub(super) fn inf_quest_fields(&mut self, node: usize) -> Result<(), String> {
        let owner = self.inf_owner(node)?;
        let root = &self.document.nodes[owner];
        let buffer = self.document.buffers[root.buffer].clone();
        let base = root.range.start;
        let file = Inf::parse(&buffer[root.range.clone()], QUEST_LAYOUT)
            .map_err(|error| error.to_string())?;
        let offset = self.document.nodes[node].range.start - base;
        let quest = file
            .quest(u32::try_from(offset).map_err(|_| "INF 任务偏移超过 32 位")?)
            .map_err(|error| error.to_string())?;
        let mut fields = [
            (
                usize::from(QUEST_LAYOUT.record_text_field),
                ScalarType::U32,
                "text_table_offset",
            ),
            (
                usize::from(QUEST_LAYOUT.record_id_field),
                ScalarType::U16,
                "quest_id",
            ),
        ];
        fields.sort_unstable_by_key(|field| field.0);
        let mut end = 0;
        for (field, scalar, name) in fields {
            if field > end {
                self.field(
                    node,
                    format!("unknown_{end:02x}"),
                    hex(&quest.prefix()[end..field]),
                    base + offset + end,
                    field - end,
                );
            }
            self.read_scalar(node, name, base + offset + field, scalar)?;
            end = field + scalar.size();
        }
        self.field(
            node,
            "记录范围",
            format!(
                "{} 字节最小已知前缀，不代表完整任务记录",
                quest.prefix().len()
            ),
            base + offset,
            0,
        );
        let lookup = match file.lookup(quest.quest_id) {
            Ok(Some(selected)) if selected.offset == quest.offset => "当前 ID 命中本记录".into(),
            Ok(Some(selected)) => format!("当前 ID 指向 {:#X}", selected.offset),
            Ok(None) => "当前 ID 未命中静态 INF 任务".into(),
            Err(error) => format!("当前 ID 查找失败：{error}"),
        };
        self.field(node, "查找状态", lookup, base + offset, 0);
        let table = match file.text_table_range(&quest) {
            Ok(Some(table)) => table,
            Ok(None) => {
                self.field(node, "文本表", "空引用", base + offset, 0);
                return Ok(());
            }
            Err(error) => {
                self.fail(node, error.to_string());
                return Ok(());
            }
        };
        for part in 0..usize::from(QUEST_LAYOUT.parts) {
            let field = table.start + part * 4;
            self.read::<u32>(node, format!("text_{part}_offset"), base + field)?;
            match file.text(&quest, part) {
                Ok(Some(text)) => {
                    let value = encoding_rs::SHIFT_JIS
                        .decode_without_bom_handling(text.bytes)
                        .0
                        .into_owned();
                    self.field(
                        node,
                        format!("文本 {part}"),
                        typed(
                            value,
                            FieldType::Text {
                                encoding: TextEncoding::ShiftJis,
                                terminated: true,
                            },
                        ),
                        base + text.offset,
                        text.bytes.len() + 1,
                    );
                }
                Ok(None) => self.field(node, format!("文本 {part}"), "空引用", base + field, 0),
                Err(error) => self.fail(node, format!("文本 {part}：{error}")),
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        field::Field,
        inspect::{self, Document},
    };

    fn word(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn sample() -> Vec<u8> {
        let mut bytes = vec![0xa5; 512];
        bytes[..inf::HEADER_SIZE].fill(0);
        bytes[..4].copy_from_slice(inf::MAGIC);
        word(&mut bytes, 4, inf::VERSION);
        word(&mut bytes, 12, inf::HEADER_SIZE as u32);
        word(&mut bytes, 16, 137);
        word(&mut bytes, 20, 151);
        bytes[137..139].copy_from_slice(&2u16.to_le_bytes());
        bytes[151..153].copy_from_slice(&100u16.to_le_bytes());
        bytes[153..155].copy_from_slice(&3u16.to_le_bytes());
        word(&mut bytes, 155, 180);
        bytes[159..161].copy_from_slice(&0u16.to_le_bytes());
        bytes[161..163].copy_from_slice(&0u16.to_le_bytes());
        word(&mut bytes, 163, u32::MAX);
        for (index, value) in [0, 220, 220].into_iter().enumerate() {
            word(&mut bytes, 180 + index * 4, value);
        }
        word(&mut bytes, 260, 300);
        bytes[266..268].copy_from_slice(&1u16.to_le_bytes());
        for (part, value) in [350, 360, 0, 350, 360, 0, 0, 0].into_iter().enumerate() {
            word(&mut bytes, 300 + part * 4, value);
        }
        bytes[350..355].copy_from_slice(&[0x82, 0xa0, 0x82, 0xa2, 0]);
        bytes[360..364].copy_from_slice(b"abc\0");
        bytes
    }

    fn field<'a>(document: &'a Document, node: usize, name: &str) -> &'a Field {
        document.nodes[node]
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
    }

    #[test]
    fn deferred_categories_and_quest_fields_bind_their_actual_discontiguous_ranges() {
        let inf_bytes = sample();
        let mut bytes = vec![0; 12];
        word(&mut bytes, 0, 1);
        word(&mut bytes, 4, 12);
        word(&mut bytes, 8, inf_bytes.len() as u32);
        bytes.extend_from_slice(&inf_bytes);
        let document = inspect::inspect("outer.bin", bytes.into());
        let root = document.nodes[document.root].children[0];
        assert_eq!(document.nodes[root].kind, Kind::Inf);
        assert!(document.nodes[root].error.is_none());
        assert_eq!(
            field(&document, root, "counts_offset").binding.range,
            28..32
        );
        assert_eq!(
            field(&document, root, "category_count").binding.range,
            149..151
        );
        let categories = document.nodes[root].children.clone();
        assert_eq!(categories.len(), 2);
        assert_eq!(document.nodes[categories[0]].kind, Kind::InfCategory(0));
        assert!(document.nodes[categories[0]].deferred);
        assert!(document.nodes[categories[0]].children.is_empty());
        assert!(!document.nodes[categories[1]].deferred);
        let limit = field(&document, categories[0], "quest_id_limit");
        assert_eq!(limit.binding.range, 163..165);
        assert_eq!(limit.binding.format, FieldType::Scalar(ScalarType::U16));
        let document = inspect::expand(&document, categories[0]).unwrap();
        let slots = document.nodes[categories[0]].children.clone();
        assert_eq!(slots.len(), 3);
        assert_eq!(document.nodes[slots[0]].kind, Kind::Block);
        assert_eq!(document.nodes[slots[0]].range, 192..196);
        for &slot in &slots[1..] {
            assert_eq!(document.nodes[slot].kind, Kind::InfQuest);
            assert_eq!(document.nodes[slot].range, 232..280);
            assert!(document.nodes[slot].deferred);
        }
        assert_eq!(
            field(&document, slots[1], "slot_offset").binding.range,
            196..200
        );
        assert_eq!(
            field(&document, slots[2], "slot_offset").binding.range,
            200..204
        );
        assert_eq!(document.bytes(slots[1]).unwrap(), &inf_bytes[220..268]);
        let document = inspect::expand(&document, slots[1]).unwrap();
        assert!(document.nodes[slots[1]].error.is_none());
        assert!(document.nodes[slots[2]].deferred);
        assert_eq!(
            field(&document, slots[1], "quest_id").binding.range,
            278..280
        );
        assert_eq!(
            field(&document, slots[1], "text_table_offset")
                .binding
                .range,
            272..276
        );
        assert_eq!(
            field(&document, slots[1], "text_0_offset").binding.range,
            312..316
        );
        let text = field(&document, slots[1], "文本 0");
        assert_eq!(text.binding.range, 362..367);
        assert_eq!(
            text.binding.format,
            FieldType::Text {
                encoding: TextEncoding::ShiftJis,
                terminated: true
            }
        );
        assert!(text.writable);
        assert_eq!(text.read(&document.buffers).unwrap(), "あい");
        assert_eq!(
            field(&document, slots[1], "文本 3").binding.range,
            text.binding.range
        );
        assert_eq!(
            field(&document, slots[1], "unknown_00").binding.range,
            232..272
        );
        assert_eq!(
            field(&document, slots[1], "unknown_2c").binding.range,
            276..278
        );
        assert_eq!(
            field(&document, slots[1], "查找状态").value,
            "当前 ID 命中本记录"
        );
    }

    #[test]
    fn invalid_quest_and_text_pointers_remain_local_and_keep_their_editable_fields() {
        let mut bytes = sample();
        word(&mut bytes, 184, u32::MAX);
        word(&mut bytes, 304, u32::MAX);
        let document = inspect::inspect("mhfinf.bin", bytes.into());
        assert_eq!(document.nodes[document.root].kind, Kind::Inf);
        let category = document.nodes[document.root].children[0];
        let document = inspect::expand(&document, category).unwrap();
        let slots = document.nodes[category].children.clone();
        let invalid = slots[1];
        assert_eq!(document.nodes[invalid].kind, Kind::Block);
        assert!(document.nodes[invalid].error.is_some());
        let pointer = field(&document, invalid, "slot_offset");
        assert_eq!(pointer.binding.range, 184..188);
        assert_eq!(pointer.binding.format, FieldType::Scalar(ScalarType::U32));
        assert!(pointer.writable);
        let document = inspect::expand(&document, slots[2]).unwrap();
        let quest = slots[2];
        assert!(
            document.nodes[quest]
                .error
                .as_ref()
                .unwrap()
                .contains("文本 1")
        );
        assert_eq!(
            field(&document, quest, "quest_id")
                .read(&document.buffers)
                .unwrap(),
            "1"
        );
        assert_eq!(
            field(&document, quest, "text_1_offset").binding.range,
            304..308
        );
        assert!(field(&document, quest, "text_1_offset").writable);
        assert_eq!(
            field(&document, quest, "文本 0")
                .read(&document.buffers)
                .unwrap(),
            "あい"
        );
        assert_eq!(
            field(&document, quest, "文本 3")
                .read(&document.buffers)
                .unwrap(),
            "あい"
        );
        assert!(document.nodes[document.root].error.is_none());
        assert!(document.nodes[category].error.is_none());
    }
}
