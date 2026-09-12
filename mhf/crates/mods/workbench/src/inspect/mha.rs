//! MHA physical members and derived native ID slots share the original tree.

use super::{Builder, Hint, Kind, archive_name, hex};
use crate::metadata;
use mhf_resource::container::{MhaArchive, MhaEntry, MhaHeader};

impl Builder {
    pub(super) fn inspect_mha(&mut self, node: usize, bytes: &[u8], base: usize) {
        self.document.nodes[node].kind = Kind::Mha;
        if let Err(error) = self.mha_contents(node, bytes, base) {
            self.fail(node, error);
        }
    }

    fn mha_contents(&mut self, node: usize, bytes: &[u8], base: usize) -> Result<(), String> {
        let archive = MhaArchive::parse(bytes, bytes.len());
        // A malformed directory can still expose a complete, editable header.
        if archive.is_err() {
            MhaHeader::parse(bytes).map_err(|error| error.to_string())?;
        }
        for (name, offset) in [
            ("entries_offset", 4),
            ("count", 8),
            ("names_offset", 12),
            ("names_size", 16),
        ] {
            self.read::<u32>(node, name, base + offset)?;
        }
        self.read::<i16>(node, "first_file_id", base + 20)?;
        self.read::<u16>(node, "file_id_count", base + 22)?;

        let archive = archive.map_err(|error| error.to_string())?;
        let h = archive.header;
        let index = match archive.file_id_index() {
            Ok(index) => {
                let assigned = index.slots.iter().filter(|slot| slot.is_some()).count();
                self.field(
                    node,
                    "ID 范围",
                    format!(
                        "{}..{}（不含上界）",
                        h.first_file_id,
                        i32::from(h.first_file_id) + i32::from(h.file_id_count),
                    ),
                    base + 20,
                    0,
                );
                self.field(node, "已分配 ID 槽", assigned, base + 22, 0);
                self.field(
                    node,
                    "未分配 ID 槽",
                    index.slots.len() - assigned,
                    base + 22,
                    0,
                );
                self.document.nodes[node].deferred = !index.slots.is_empty();
                Some(index)
            }
            Err(error) => {
                self.fail(node, error.to_string());
                None
            }
        };

        let buffer = self.document.nodes[node].buffer;
        for item in &archive.entries {
            let entry = item.entry;
            let name = archive_name(item.name);
            let at = if entry.size == 0 {
                base
            } else {
                base + entry.offset as usize
            };
            let Some(child) = self.child(
                node,
                format!("{:04} · {name}", entry.index),
                Kind::Unknown,
                buffer,
                at..at + entry.size as usize,
            ) else {
                break;
            };
            if let Some(value) = std::str::from_utf8(item.name)
                .ok()
                .and_then(metadata::from_filename)
            {
                self.document.nodes[child].metadata.insert(value);
            }
            let meta = base + h.entries_offset as usize + entry.index * 20;
            self.read::<u32>(child, "name_offset", meta)?;
            self.field(
                child,
                "原始名称",
                hex(item.name),
                base + h.names_offset as usize + item.name_offset as usize,
                item.name.len(),
            );
            self.read::<u32>(child, "offset", meta + 4)?;
            self.read::<u32>(child, "size", meta + 8)?;
            self.read::<u32>(child, "padded_size", meta + 12)?;
            self.read::<i16>(child, "file_id", meta + 16)?;
            self.read::<u16>(child, "file_id_high_raw", meta + 18)?;
            let file_id = i32::from(item.native_file_id());
            self.field(
                child,
                "ID 槽",
                file_id - i32::from(h.first_file_id),
                meta + 16,
                0,
            );
            let status = match index.as_ref().and_then(|index| index.entry_index(file_id)) {
                Some(selected) if selected != entry.index => format!("由目录项 {selected:04} 覆盖"),
                Some(_) => entry_status(item).into(),
                None => "ID 索引无效".into(),
            };
            self.field(child, "ID 索引状态", status, meta + 16, 0);
            self.inspect_node(
                child,
                Hint {
                    directory: true,
                    ..Hint::from_path(&name)
                },
            );
        }
        Ok(())
    }

    /// Expand lookup fields without adding or reordering physical children.
    pub(super) fn mha_id_details(
        &mut self,
        node: usize,
        archive: &MhaArchive<'_>,
        base: usize,
    ) -> Result<(), String> {
        let index = archive.file_id_index().map_err(|error| error.to_string())?;
        for (slot, entry) in index.slots.iter().enumerate() {
            let id = i32::from(index.first_file_id) + slot as i32;
            let (value, offset, size) = match entry {
                Some(entry) => {
                    let item = &archive.entries[*entry];
                    (
                        format!(
                            "目录项 {:04} · {} · {}",
                            item.entry.index,
                            archive_name(item.name),
                            entry_status(item),
                        ),
                        base + archive.header.entries_offset as usize + entry * 20 + 16,
                        2,
                    )
                }
                None => ("未分配".into(), base + 22, 0),
            };
            // Present IDs with no backing descriptor as derived values. A
            // populated slot can locate its selected record's real ID word.
            self.field(node, format!("ID {id} · 槽 {slot}"), value, offset, size);
        }
        Ok(())
    }
}

fn entry_status(item: &MhaEntry<'_>) -> &'static str {
    if item.entry.offset == 0 {
        "未找到（offset = 0）"
    } else if item.entry.size == 0 {
        "空资源（size = 0，不读取）"
    } else {
        "生效"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        edit,
        field::{Field, FieldType, ScalarType},
        inspect::{self, Document},
        metadata::EquipmentModel,
    };

    fn word(bytes: &mut [u8], offset: usize, value: u32) {
        bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn archive(first_id: i16, id_count: u16, records: &[(&str, u32, &[u8])]) -> Vec<u8> {
        let names_offset = 24 + records.len() * 20;
        let names_size: usize = records.iter().map(|(name, _, _)| name.len() + 1).sum();
        let mut bytes = vec![0; names_offset];
        bytes[..4].copy_from_slice(b"mha\x01");
        word(&mut bytes, 4, 24);
        word(&mut bytes, 8, records.len() as u32);
        word(&mut bytes, 12, names_offset as u32);
        word(&mut bytes, 16, names_size as u32);
        bytes[20..22].copy_from_slice(&first_id.to_le_bytes());
        bytes[22..24].copy_from_slice(&id_count.to_le_bytes());
        for (name, _, _) in records {
            bytes.extend_from_slice(name.as_bytes());
            bytes.push(0);
        }
        let mut name_offset = 0;
        for (index, &(name, id, payload)) in records.iter().enumerate() {
            let field = 24 + index * 20;
            let offset = bytes.len();
            for (at, value) in [
                (field, name_offset),
                (field + 4, offset as u32),
                (field + 8, payload.len() as u32),
                (field + 12, payload.len() as u32),
                (field + 16, id),
            ] {
                word(&mut bytes, at, value);
            }
            bytes.extend_from_slice(payload);
            name_offset += name.len() as u32 + 1;
        }
        bytes
    }

    fn field<'a>(document: &'a Document, node: usize, name: &str) -> &'a Field {
        document.nodes[node]
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
    }

    fn change(document: &Document, node: usize, name: &str, input: &str) -> Document {
        let patch = field(document, node, name)
            .write(&document.buffers, input)
            .unwrap()
            .unwrap();
        edit::apply_many(document, &[patch]).unwrap()
    }

    #[test]
    fn nested_ids_expand_as_fields_without_changing_physical_children_or_names() {
        let named = archive(
            -2,
            4,
            &[("wi521.bin", 0xabcd_fffe, b"a"), ("other.bin", 1, b"bc")],
        );
        let mut bytes = vec![0; 12];
        word(&mut bytes, 0, 1);
        word(&mut bytes, 4, 12);
        word(&mut bytes, 8, named.len() as u32);
        bytes.extend_from_slice(&named);
        let document = inspect::inspect("outer.bin", bytes.into());
        let named_node = document.nodes[0].children[0];
        assert_eq!(document.nodes[named_node].kind, Kind::Mha);
        let children = document.nodes[named_node].children.clone();
        assert_eq!(children.len(), 2);
        let key = edit::node_key(&document, children[0]).unwrap();
        let first_id = field(&document, named_node, "first_file_id");
        assert_eq!(first_id.binding.range, 32..34);
        assert_eq!(first_id.binding.format, FieldType::Scalar(ScalarType::I16));
        assert_eq!(first_id.read(&document.buffers).unwrap(), "-2");
        let parsed = MhaArchive::parse(&named, 2).unwrap();
        let name = field(&document, children[0], "原始名称");
        assert_eq!(
            name.binding.range,
            12 + parsed.header.names_offset as usize..12 + parsed.header.names_offset as usize + 9
        );
        assert_eq!(name.binding.bytes(&document.buffers).unwrap(), b"wi521.bin");
        assert_eq!(name.binding.format, FieldType::Bytes);
        assert!(name.writable);
        assert_eq!(
            document
                .metadata()
                .resolve::<EquipmentModel>(children[0])
                .unwrap()
                .value
                .model_id,
            4521
        );

        assert!(document.nodes[named_node].deferred);
        let expanded = inspect::expand(&document, named_node).unwrap();
        assert_eq!(expanded.nodes.len(), document.nodes.len());
        assert_eq!(expanded.nodes[named_node].children, children);
        assert_eq!(edit::locate(&expanded, &key), Some(children[0]));
        assert_eq!(expanded.bytes(children[0]), Some(&b"a"[..]));
        assert_eq!(field(&expanded, named_node, "ID -1 · 槽 1").value, "未分配");
        assert!(
            field(&expanded, named_node, "ID -1 · 槽 1")
                .binding
                .range
                .is_empty()
        );
        let mapped = field(&expanded, named_node, "ID -2 · 槽 0");
        assert!(mapped.value.contains("目录项 0000 · wi521.bin"));
        assert_eq!(mapped.binding.range, 52..54);
        assert!(!mapped.writable);
        assert!(!expanded.nodes[named_node].deferred);
    }

    #[test]
    fn editing_low_and_high_id_words_rebuilds_slots_and_preserves_unrelated_storage() {
        let source = archive(
            500,
            3,
            &[
                ("wi521.bin", 0x1234_01f4, b"old"),
                ("wi522.bin", 0x5678_01f4, b"new"),
                ("empty.bin", 502, b""),
            ],
        );
        let document = inspect::inspect("test.abn", source.clone().into());
        let children = document.nodes[0].children.clone();
        let key = edit::node_key(&document, children[0]).unwrap();
        assert_eq!(
            field(&document, children[0], "ID 索引状态").value,
            "由目录项 0001 覆盖"
        );
        let updated = change(&document, children[0], "file_id", "501");
        assert_eq!(edit::locate(&updated, &key), Some(children[0]));
        let parsed = MhaArchive::parse(&updated.buffers[0], 3).unwrap();
        assert_eq!(parsed.entries[0].file_id, 0x1234_01f5);
        assert_eq!(
            parsed.file_id_index().unwrap().slots,
            [Some(1), Some(0), Some(2)]
        );
        assert_eq!(field(&updated, children[0], "ID 索引状态").value, "生效");
        assert_eq!(
            field(&updated, children[2], "ID 索引状态").value,
            "空资源（size = 0，不读取）"
        );
        let high = field(&updated, children[1], "file_id_high_raw");
        assert_eq!(high.binding.range, 62..64);
        assert_eq!(high.binding.format, FieldType::Scalar(ScalarType::U16));
        let updated = change(&updated, children[1], "file_id_high_raw", "0xabcd");
        let parsed = MhaArchive::parse(&updated.buffers[0], 3).unwrap();
        assert_eq!(parsed.entries[1].file_id, 0xabcd_01f4);
        assert_eq!(
            parsed.file_id_index().unwrap().slots,
            [Some(1), Some(0), Some(2)]
        );
        assert_eq!(parsed.entries[0].name, b"wi521.bin");
        assert_eq!(
            parsed.entries[0]
                .entry
                .payload(&updated.buffers[0])
                .unwrap(),
            b"old"
        );
        assert_eq!(
            parsed.entries[1]
                .entry
                .payload(&updated.buffers[0])
                .unwrap(),
            b"new"
        );
        assert_eq!(&updated.buffers[0][64..], &source[64..]);
    }

    #[test]
    fn invalid_id_metadata_stays_editable_until_the_range_is_repaired() {
        let source = archive(500, 3, &[("member.bin", 502, b"resource")]);
        let document = inspect::inspect("test.abn", source.into());
        let child = document.nodes[0].children[0];
        let invalid = change(&document, 0, "first_file_id", "499");
        assert!(
            invalid.nodes[0]
                .error
                .as_deref()
                .unwrap()
                .contains("outside native ID range")
        );
        assert_eq!(invalid.bytes(child), Some(&b"resource"[..]));
        assert!(!invalid.nodes[0].deferred);
        assert!(field(&invalid, 0, "file_id_count").writable);
        let fixed = change(&invalid, 0, "file_id_count", "4");
        assert!(fixed.nodes[0].error.is_none());
        assert_eq!(fixed.nodes[0].children, [child]);
        assert_eq!(
            MhaArchive::parse(&fixed.buffers[0], 1)
                .unwrap()
                .file_id_index()
                .unwrap()
                .slots,
            [None, None, None, Some(0)]
        );
        let invalid = change(&fixed, 0, "file_id_count", "32768");
        assert!(
            invalid.nodes[0]
                .error
                .as_deref()
                .unwrap()
                .contains("signed 16-bit")
        );
        assert_eq!(invalid.bytes(child), Some(&b"resource"[..]));
        assert!(
            change(&invalid, 0, "file_id_count", "4").nodes[0]
                .error
                .is_none()
        );
    }

    #[test]
    fn malformed_directory_ranges_still_expose_typed_header_fields() {
        let mut source = archive(-7, 8, &[]);
        word(&mut source, 4, u32::MAX);
        let document = inspect::inspect("broken.abn", source.into());
        assert!(document.nodes[0].error.is_some());
        assert!(document.nodes[0].children.is_empty());
        assert_eq!(
            field(&document, 0, "first_file_id")
                .read(&document.buffers)
                .unwrap(),
            "-7"
        );
        assert!(field(&document, 0, "entries_offset").writable);
        assert!(field(&document, 0, "file_id_count").writable);
    }
}
