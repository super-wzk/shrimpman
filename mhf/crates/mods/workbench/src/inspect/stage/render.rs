//! Render commands locate their target records without duplicating physical nodes.

use super::{Builder, Kind, hex};
use crate::field::{FieldType, ScalarType, formatted, typed};
use mhf_resource::binary::Reader;
use mhf_resource::stage::{
    RenderTables,
    render_tables::{AnimationChannel, AnimationCommand, KeyframeHeader, PointLightSelection},
};

fn channel_name(channel: AnimationChannel) -> &'static str {
    match channel {
        AnimationChannel::DirectionalLight => "方向光动画",
        AnimationChannel::CubeMapLight => "环境立方体光照动画",
        AnimationChannel::GodRay => "光束动画",
        AnimationChannel::HeightFog => "高度雾动画",
        AnimationChannel::DistanceFog => "距离雾动画",
        AnimationChannel::DepthOfField => "景深动画",
        AnimationChannel::Bloom => "辉光动画",
        AnimationChannel::ShadowMap => "阴影动画",
        AnimationChannel::ToneMapping => "色调映射动画",
        AnimationChannel::GaussianBlur => "高斯模糊动画",
        AnimationChannel::PointLight => "点光源动画选择",
    }
}

impl Builder {
    pub(in crate::inspect) fn stage_render_details(
        &mut self,
        node: usize,
        file: &RenderTables<'_>,
        base: usize,
    ) {
        let buffer = self.document.nodes[node].buffer;
        for table in &file.tables {
            let at = base + table.offset;
            let channel = table.animation_channel();
            let name = match channel {
                Some(channel) => channel_name(channel),
                None if table.count_offset == 26 => "动画命令",
                None => "渲染匹配表",
            };
            let Some(parent) = self.child(
                node,
                format!(
                    "{name} · 表 {:02X} · {} 项 × {} 字节",
                    table.count_offset, table.count, table.record_size,
                ),
                Kind::Block,
                buffer,
                at..at + table.records.len(),
            ) else {
                break;
            };
            self.field(
                parent,
                "记录数量",
                table.count,
                base + table.count_offset,
                2,
            );
            for (index, bytes) in table.records().enumerate() {
                let offset = at + index * table.record_size;
                let Some(child) = self.child(
                    parent,
                    format!("记录 {index}"),
                    Kind::Block,
                    buffer,
                    offset..offset + bytes.len(),
                ) else {
                    break;
                };
                let result = match channel {
                    Some(AnimationChannel::PointLight) => {
                        self.render_point_light_selection(child, bytes, offset)
                    }
                    Some(_) => self.render_keyframe(child, bytes, offset),
                    None if table.count_offset == 26 => {
                        self.render_command(child, bytes, offset, file, base)
                    }
                    None => self.render_match_record(child, offset),
                };
                if let Err(error) = result {
                    self.fail(child, error);
                }
            }
        }
        if !file.trailing.is_empty() {
            self.field(
                node,
                "尾部原始字节",
                hex(file.trailing),
                base + file.source.len() - file.trailing.len(),
                file.trailing.len(),
            );
        }
    }

    fn render_command(
        &mut self,
        node: usize,
        bytes: &[u8],
        at: usize,
        file: &RenderTables<'_>,
        base: usize,
    ) -> Result<(), String> {
        let command = AnimationCommand::parse(bytes).map_err(|error| error.to_string())?;
        self.read::<u16>(node, "sequence_id", at)?;
        self.read::<u8>(node, "channel", at + 2)?;
        self.read::<u8>(node, "unknown_03", at + 3)?;
        self.read_as::<u32>(
            node,
            "sequence_flags",
            at + 4,
            FieldType::Flags(ScalarType::U32),
        )?;
        self.read::<i32>(node, "sequence_delay", at + 8)?;
        self.read::<u32>(node, "animation_id", at + 12)?;
        let Some(channel) = AnimationChannel::from_id(command.channel) else {
            let meaning = if command.channel == 0 {
                "序列终止"
            } else {
                "未识别通道，保留原始动画 ID"
            };
            self.field(node, "通道类型", meaning, at + 2, 0);
            return Ok(());
        };
        self.field(node, "通道类型", channel_name(channel), at + 2, 0);
        match file
            .animation_records(channel, command.animation_id)
            .map_err(|error| error.to_string())?
        {
            Some(target) => {
                // This is a locator, not an ownership edge or another editable
                // copy. The target records remain under their original table.
                self.field(
                    node,
                    "目标动画记录",
                    typed(
                        format!(
                            "{} · ID {} · 首记录 {} · {} 项",
                            channel_name(channel),
                            command.animation_id,
                            target.first_record,
                            target.count(),
                        ),
                        FieldType::ReadOnly,
                    ),
                    base + target.offset,
                    target.records.len(),
                );
            }
            None => self.field(
                node,
                "目标动画记录",
                format!(
                    "未找到 {} 的动画 ID {}",
                    channel_name(channel),
                    command.animation_id
                ),
                at + 12,
                0,
            ),
        }
        Ok(())
    }

    fn render_keyframe(&mut self, node: usize, bytes: &[u8], at: usize) -> Result<(), String> {
        let header = KeyframeHeader::parse(bytes).map_err(|error| error.to_string())?;
        self.document.nodes[node].name = format!(
            "{} · 动画 {} · 帧 {}",
            self.document.nodes[node].name, header.animation_id, header.frame,
        );
        self.read::<u16>(node, "animation_id", at)?;
        self.read::<u16>(node, "frame", at + 2)?;
        self.read_as::<u8>(node, "flags", at + 4, FieldType::Flags(ScalarType::U8))?;
        self.read::<[u8; 3]>(node, "unknown_05", at + 5)?;
        for offset in (KeyframeHeader::SIZE..bytes.len()).step_by(4) {
            let word = Reader::with_base(bytes, at)
                .read_at::<u32>(offset)
                .map_err(|error| error.to_string())?;
            let bits = word.value;
            self.field(
                node,
                format!("word_{offset:02X}"),
                formatted(
                    bits,
                    format!(
                        "{bits:#010X} · i32 {} · f32 {}",
                        bits as i32,
                        f32::from_bits(bits)
                    ),
                ),
                word.range.start,
                word.range.len(),
            );
        }
        Ok(())
    }

    fn render_point_light_selection(
        &mut self,
        node: usize,
        bytes: &[u8],
        at: usize,
    ) -> Result<(), String> {
        let selection = PointLightSelection::parse(bytes).map_err(|error| error.to_string())?;
        self.document.nodes[node].name = format!(
            "{} · 动画 {} · 光照动画组 {}",
            self.document.nodes[node].name, selection.animation_id, selection.animation_group_id,
        );
        self.read::<u16>(node, "animation_id", at)?;
        self.read::<u16>(node, "unknown_02", at + 2)?;
        self.read::<u32>(node, "unknown_04", at + 4)?;
        self.read::<u16>(node, "animation_group_id", at + 8)?;
        self.read::<u16>(node, "first_light_id", at + 10)?;
        self.read::<u16>(node, "last_light_id", at + 12)?;
        self.read::<u16>(node, "unknown_0e", at + 14)?;
        Ok(())
    }

    fn render_match_record(&mut self, node: usize, at: usize) -> Result<(), String> {
        self.read::<u8>(node, "kind", at)?;
        self.read::<u8>(node, "unknown_01", at + 1)?;
        self.read::<u16>(node, "unknown_02", at + 2)?;
        self.read::<u16>(node, "match_04", at + 4)?;
        self.read::<u16>(node, "match_06", at + 6)?;
        self.read::<u32>(node, "value", at + 8)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        edit,
        field::Field,
        inspect::{Document, expand, inspect},
    };
    use std::sync::Arc;

    fn render_tables() -> Vec<u8> {
        let mut bytes = vec![0; 32];
        for (offset, value) in [(0, 2u16), (4, 3), (28, 1), (26, 4)] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (id, frame) in [(7u16, 0u16), (7, 30), (8, 60)] {
            let mut record = vec![0; 28];
            record[..2].copy_from_slice(&id.to_le_bytes());
            record[2..4].copy_from_slice(&frame.to_le_bytes());
            record[4..8].copy_from_slice(&[0x81, 0xa5, 0xfe, 0xcc]);
            record[8..12].copy_from_slice(&0x7fc0_1234u32.to_le_bytes());
            bytes.extend(record);
        }
        for value in [9u16, 0x1234, 0xbeef, 0x7fc0, 81, 7, 0xffff, 0xabcd] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        for (channel, id) in [(1u8, 7u32), (11, 9), (1, 777), (0, 0)] {
            bytes.extend_from_slice(&253u16.to_le_bytes());
            bytes.extend_from_slice(&[channel, 0xab]);
            bytes.extend_from_slice(&0x8000_0001u32.to_le_bytes());
            bytes.extend_from_slice(&(-7i32).to_le_bytes());
            bytes.extend_from_slice(&id.to_le_bytes());
        }
        bytes
    }

    fn nested() -> Arc<[u8]> {
        let render = render_tables();
        let mut bytes = [2, 20, 3, 23, render.len() as u32]
            .into_iter()
            .flat_map(u32::to_le_bytes)
            .collect::<Vec<_>>();
        bytes.extend_from_slice(b"abc");
        bytes.extend(render);
        bytes.into()
    }

    fn field<'a>(document: &'a Document, node: usize, name: &str) -> &'a Field {
        document.nodes[node]
            .fields
            .iter()
            .find(|field| field.name == name)
            .unwrap()
    }

    fn render_node(document: &Document) -> usize {
        document
            .nodes
            .iter()
            .position(|node| node.kind == Kind::StageRenderTables)
            .unwrap()
    }

    #[test]
    fn commands_locate_original_records_and_typed_edits_preserve_unrelated_bytes() {
        let source = nested();
        let document = inspect("scene.bin", source.clone());
        let node = render_node(&document);
        assert_eq!(document.nodes[node].range.start, 23);
        assert!(document.nodes[node].deferred);
        let expanded = expand(&document, node).unwrap();
        assert!(Arc::ptr_eq(&source, &expanded.buffers[0]));
        assert_eq!(expanded.buffers.len(), 1);
        assert!(expanded.nodes.iter().all(|node| node.error.is_none()));
        let tables = &expanded.nodes[node].children;
        assert_eq!(tables.len(), 13);
        let commands = &expanded.nodes[tables[12]].children;
        let first_key = expanded.nodes[tables[0]].children[0];
        let target = field(&expanded, commands[0], "目标动画记录");
        assert_eq!(target.binding.buffer, 0);
        assert_eq!(target.binding.range, 55..111);
        assert_eq!(
            target.binding.range.start,
            expanded.nodes[first_key].range.start
        );
        assert_eq!(target.binding.format, FieldType::ReadOnly);
        assert!(!target.writable);
        assert!(target.write(&expanded.buffers, "8").is_err());
        assert!(
            commands
                .iter()
                .all(|&node| expanded.nodes[node].children.is_empty())
        );
        assert!(
            field(&expanded, commands[2], "目标动画记录")
                .value
                .contains("未找到")
        );
        assert_eq!(field(&expanded, commands[3], "通道类型").value, "序列终止");
        assert_eq!(
            field(&expanded, commands[0], "sequence_delay")
                .read(&expanded.buffers)
                .unwrap(),
            "-7"
        );

        let patch = field(&expanded, commands[0], "animation_id")
            .write(&expanded.buffers, "8")
            .unwrap()
            .unwrap();
        let mut expected = source.to_vec();
        expected[patch.binding.range.clone()].copy_from_slice(&patch.after);
        let updated = edit::apply_many(&expanded, &[patch]).unwrap();
        assert_eq!(updated.buffers[0].as_ref(), expected);
        let node = render_node(&updated);
        let tables = &updated.nodes[node].children;
        let command = updated.nodes[tables[12]].children[0];
        let key = updated.nodes[tables[0]].children[2];
        assert_eq!(
            field(&updated, command, "目标动画记录").binding.range,
            111..139
        );
        let frame = field(&updated, key, "frame");
        assert_eq!(frame.binding.range, 113..115);
        assert_eq!(frame.binding.format, FieldType::Scalar(ScalarType::U16));
        let patch = frame.write(&updated.buffers, "65535").unwrap().unwrap();
        expected[patch.binding.range.clone()].copy_from_slice(&patch.after);
        let updated = edit::apply_many(&updated, &[patch]).unwrap();
        assert_eq!(updated.buffers[0].as_ref(), expected);
        assert_eq!(
            &updated.buffers[0][115..123],
            &[0x81, 0xa5, 0xfe, 0xcc, 0x34, 0x12, 0xc0, 0x7f]
        );
    }

    #[test]
    fn point_light_records_keep_their_distinct_fields_and_physical_ownership() {
        let document = inspect("render.bin", render_tables().into());
        let node = render_node(&document);
        let expanded = expand(&document, node).unwrap();
        let tables = &expanded.nodes[node].children;
        let selection = expanded.nodes[tables[1]].children[0];
        assert!(
            expanded.nodes[selection]
                .fields
                .iter()
                .all(|field| field.name != "frame" && field.name != "flags")
        );
        for (name, offset, size, value) in [
            ("unknown_02", 118, 2, "4660"),
            ("unknown_04", 120, 4, "2143338223"),
            ("animation_group_id", 124, 2, "81"),
            ("first_light_id", 126, 2, "7"),
            ("last_light_id", 128, 2, "65535"),
            ("unknown_0e", 130, 2, "43981"),
        ] {
            let field = field(&expanded, selection, name);
            assert_eq!(field.binding.range, offset..offset + size);
            assert_eq!(field.read(&expanded.buffers).unwrap(), value);
            assert!(field.writable);
        }
        let command = expanded.nodes[tables[12]].children[1];
        assert_eq!(
            field(&expanded, command, "目标动画记录").binding.range,
            expanded.nodes[selection].range
        );
        assert!(expanded.nodes[command].children.is_empty());
    }
}
