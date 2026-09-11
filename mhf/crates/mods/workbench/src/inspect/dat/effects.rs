use super::*;
use mhf_resource::{
    dat::EffectRecordKind,
    effect::{AttachmentDefinition, AttachmentGroup, ModelEffectBinding, ModelEffectDefinition},
};

impl Builder {
    pub(super) fn dat_effect_fields(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        bytes: &[u8],
        kind: EffectRecordKind,
        base: usize,
    ) -> Result<(), String> {
        let at = self.document.nodes[node].range.start;
        match kind {
            EffectRecordKind::AttachmentGroup => {
                let record = AttachmentGroup::parse(bytes).map_err(|error| error.to_string())?;
                self.field(node, "附着部位代码", record.part_code, at, 2);
                self.dat_effect_references(node, file, &record.definition_ids, at + 2, 1, base);
            }
            EffectRecordKind::ModelBinding => {
                let record = ModelEffectBinding::parse(bytes).map_err(|error| error.to_string())?;
                self.field(node, "模型绑定部位代码", record.part_code, at, 2);
                self.field(
                    node,
                    "武器种类 ID（仅武器绑定使用）",
                    record.weapon_class,
                    at + 2,
                    2,
                );
                self.field(node, "变体选择值", record.variant, at + 4, 2);
                self.field(node, "模型 ID", record.model_id, at + 6, 2);
                if record.part_code == 0 {
                    self.field(
                        node,
                        "匹配状态",
                        "索引 0 为保留记录；后续部位 0 终止原生匹配",
                        at,
                        2,
                    );
                }
                self.dat_effect_references(node, file, &record.definition_ids, at + 8, 3, base);
            }
            EffectRecordKind::AttachmentDefinition => {
                let record =
                    AttachmentDefinition::parse(bytes).map_err(|error| error.to_string())?;
                for (axis, bits) in record.local_position_bits.into_iter().enumerate() {
                    self.field(
                        node,
                        format!("局部位置 {}", ["X", "Y", "Z"][axis]),
                        format!("{} ({bits:#010X})", f32::from_bits(bits)),
                        at + axis * 4,
                        4,
                    );
                }
                self.field(node, "生效条件 ID", record.activation_condition, at + 12, 1);
                self.field(node, "骨骼节点索引", record.node_index, at + 13, 1);
                self.field(node, "附着模式原值", record.attachment_mode, at + 14, 1);
                self.attachment_animation_fields(node, &record, at);
            }
            EffectRecordKind::ModelDefinition => {
                let record =
                    ModelEffectDefinition::parse(bytes).map_err(|error| error.to_string())?;
                for (axis, bits) in record.translation_delta_bits.into_iter().enumerate() {
                    self.field(
                        node,
                        format!("节点位移增量 {}", ["X", "Y", "Z"][axis]),
                        format!("{} ({bits:#010X})", f32::from_bits(bits)),
                        at + axis * 4,
                        4,
                    );
                }
                self.field(node, "生效条件 ID", record.activation_condition, at + 12, 1);
                self.field(node, "绘制组", record.draw_group, at + 13, 1);
                self.field(node, "组内条目", record.group_entry, at + 14, 1);
                self.field(node, "骨骼节点索引", record.node_index, at + 15, 1);
                self.field(node, "启动延迟原值", record.start_delay, at + 16, 2);
                self.model_effect_animation_fields(node, &record, at);
            }
        }
        Ok(())
    }

    fn attachment_animation_fields(
        &mut self,
        node: usize,
        record: &AttachmentDefinition,
        at: usize,
    ) {
        let float = |bits: u32| format!("{} ({bits:#010X})", f32::from_bits(bits));
        self.field(node, "unknown_0F", record.unknown_0f, at + 0x0f, 1);
        self.field(node, "默认资源 ID", record.resource_id, at + 0x10, 2);
        self.field(node, "启动延迟原值", record.start_delay, at + 0x12, 2);
        self.field(
            node,
            "资源序列 · 起始 ID",
            record.sequence.start_id,
            at + 0x14,
            2,
        );
        self.field(
            node,
            "资源序列 · 结束 ID",
            record.sequence.end_id,
            at + 0x16,
            2,
        );
        self.field(
            node,
            "资源序列 · 重复次数",
            record.sequence.repetitions,
            at + 0x18,
            2,
        );
        self.field(
            node,
            "资源序列 · 切换间隔",
            record.sequence.interval,
            at + 0x1a,
            2,
        );
        for (axis, channel) in record.rotation.iter().enumerate() {
            let name = ["旋转 X", "旋转 Y", "旋转 Z"][axis];
            let offset = at + 0x1c + axis * 8;
            self.field(
                node,
                format!("{name} · 起始角度（度）"),
                channel.start_degrees,
                offset,
                2,
            );
            self.field(
                node,
                format!("{name} · 结束角度（度）"),
                channel.end_degrees,
                offset + 2,
                2,
            );
            self.field(
                node,
                format!("{name} · 重复次数"),
                channel.repetitions,
                offset + 4,
                2,
            );
            self.field(
                node,
                format!("{name} · 周期步数"),
                channel.duration,
                offset + 6,
                2,
            );
        }
        self.field(
            node,
            "统一缩放 · 起始值",
            float(record.scale.start_bits),
            at + 0x34,
            4,
        );
        self.field(
            node,
            "统一缩放 · 结束值",
            float(record.scale.end_bits),
            at + 0x38,
            4,
        );
        self.field(
            node,
            "统一缩放 · 往返标志",
            record.scale.ping_pong,
            at + 0x3c,
            2,
        );
        self.field(
            node,
            "统一缩放 · 重复次数",
            record.scale.repetitions,
            at + 0x3e,
            2,
        );
        self.field(
            node,
            "统一缩放 · 周期步数",
            record.scale.duration,
            at + 0x40,
            2,
        );
        self.field(node, "unknown_42", hex(&record.unknown_42), at + 0x42, 4);
        self.field(
            node,
            "渲染标志",
            format!("{:#04X}", record.render_flags),
            at + 0x46,
            1,
        );
        self.field(
            node,
            "朝向矩阵选择标志",
            hex(&record.orientation_flags),
            at + 0x47,
            3,
        );
        self.field(
            node,
            "颜色 · 起始 RGB",
            format!("{:?}", record.color.start_rgb),
            at + 0x4a,
            3,
        );
        self.field(
            node,
            "颜色 · 结束 RGB",
            format!("{:?}", record.color.end_rgb),
            at + 0x4d,
            3,
        );
        self.field(
            node,
            "颜色 · 往返标志",
            record.color.ping_pong,
            at + 0x50,
            2,
        );
        self.field(
            node,
            "颜色 · 重复次数",
            record.color.repetitions,
            at + 0x52,
            2,
        );
        self.field(node, "颜色 · 周期步数", record.color.duration, at + 0x54, 2);
        self.field(
            node,
            "透明度 · 起始原值",
            record.opacity.start,
            at + 0x56,
            2,
        );
        self.field(node, "透明度 · 结束原值", record.opacity.end, at + 0x58, 2);
        self.field(
            node,
            "透明度 · 重复次数",
            record.opacity.repetitions,
            at + 0x5a,
            2,
        );
        self.field(
            node,
            "透明度 · 周期步数",
            record.opacity.duration,
            at + 0x5c,
            2,
        );
        self.field(
            node,
            "透明度 · 往返标志",
            record.opacity.ping_pong,
            at + 0x5e,
            1,
        );
        self.field(
            node,
            "渲染状态 0x60 原值",
            record.render_state_60,
            at + 0x5f,
            1,
        );
        self.field(node, "unknown_60", hex(&record.unknown_60), at + 0x60, 2);
        self.field(node, "UV · U 周期", record.uv.u_period, at + 0x62, 2);
        self.field(node, "UV · V 周期", record.uv.v_period, at + 0x64, 2);
        self.field(node, "UV · 循环步数", record.uv.cycle_steps, at + 0x66, 2);
        self.field(node, "UV · 重复次数", record.uv.repetitions, at + 0x68, 2);
        self.field(node, "unknown_6A", hex(&record.unknown_6a), at + 0x6a, 2);
        self.field(
            node,
            "视线方向偏移",
            float(record.view_offset_bits),
            at + 0x6c,
            4,
        );
        self.field(node, "拖尾模式原值", record.trail_mode, at + 0x70, 1);
        self.field(
            node,
            "拖尾 RGB",
            format!("{:?}", record.trail_rgb),
            at + 0x71,
            3,
        );
        self.field(
            node,
            "武器显隐选择值",
            record.weapon_visibility,
            at + 0x74,
            1,
        );
        self.field(node, "显隐条件模式", record.visibility_mode, at + 0x75, 1);
        self.field(
            node,
            "显隐条件标志",
            format!("{:#04X}", record.visibility_flags),
            at + 0x76,
            1,
        );
        self.field(node, "unknown_77", hex(&record.unknown_77), at + 0x77, 2);
        self.field(
            node,
            "终态初始化标志",
            record.terminal_initialization,
            at + 0x79,
            1,
        );
        self.field(node, "unknown_7A", hex(&record.unknown_7a), at + 0x7a, 6);
    }

    fn model_effect_animation_fields(
        &mut self,
        node: usize,
        record: &ModelEffectDefinition,
        at: usize,
    ) {
        let float = |bits: u32| format!("{} ({bits:#010X})", f32::from_bits(bits));
        self.field(node, "unknown_12", hex(&record.unknown_12), at + 0x12, 2);
        for (axis, (&flag, channel)) in record
            .rotation_state_flags
            .iter()
            .zip(&record.rotation)
            .enumerate()
        {
            let name = ["旋转 X", "旋转 Y", "旋转 Z"][axis];
            let offset = at + 0x18 + axis * 12;
            self.field(
                node,
                format!("{name} · 状态联动标志"),
                flag,
                at + 0x14 + axis,
                1,
            );
            self.field(
                node,
                format!("{name} · 起始角度（度）"),
                channel.start_degrees,
                offset,
                2,
            );
            self.field(
                node,
                format!("{name} · 结束角度（度）"),
                channel.end_degrees,
                offset + 2,
                2,
            );
            self.field(
                node,
                format!("{name} · 重复次数"),
                channel.repetitions,
                offset + 4,
                2,
            );
            self.field(
                node,
                format!("{name} · 周期步数"),
                channel.duration,
                offset + 6,
                2,
            );
            self.field(
                node,
                format!("{name} · 状态速度倍率"),
                float(channel.state_rate_multiplier_bits),
                offset + 8,
                4,
            );
        }
        self.field(node, "unknown_17", record.unknown_17, at + 0x17, 1);
        for (axis, channel) in record.scale.iter().enumerate() {
            let name = ["缩放 X", "缩放 Y", "缩放 Z"][axis];
            let offset = at + 0x3c + axis * 20;
            self.field(
                node,
                format!("{name} · 起始值"),
                float(channel.start_bits),
                offset,
                4,
            );
            self.field(
                node,
                format!("{name} · 结束值"),
                float(channel.end_bits),
                offset + 4,
                4,
            );
            let (label, value) = if axis == 2 {
                ("状态速度倍率", float(channel.parameter_08_bits))
            } else {
                ("未确认参数", format!("{:#010X}", channel.parameter_08_bits))
            };
            self.field(node, format!("{name} · {label}"), value, offset + 8, 4);
            self.field(
                node,
                format!("{name} · 重复次数"),
                channel.repetitions,
                offset + 12,
                2,
            );
            self.field(
                node,
                format!("{name} · 周期步数"),
                channel.duration,
                offset + 14,
                2,
            );
            let label = if axis == 2 {
                "状态插值模式"
            } else {
                "未确认模式"
            };
            self.field(
                node,
                format!("{name} · {label}"),
                channel.mode,
                offset + 16,
                1,
            );
            self.field(
                node,
                format!("{name} · 往返标志"),
                channel.ping_pong,
                offset + 17,
                1,
            );
            self.field(
                node,
                format!("unknown_{:02X}", 0x4e + axis * 20),
                hex(&channel.unknown_12),
                offset + 18,
                2,
            );
        }
        self.field(
            node,
            "渲染标志",
            format!("{:#04X}", record.render_flags),
            at + 0x78,
            1,
        );
        self.field(node, "unknown_79", hex(&record.unknown_79), at + 0x79, 3);
        self.field(
            node,
            "颜色 · 起始 RGB",
            format!("{:?}", record.color.start_rgb),
            at + 0x7c,
            3,
        );
        self.field(
            node,
            "颜色 · 结束 RGB",
            format!("{:?}", record.color.end_rgb),
            at + 0x7f,
            3,
        );
        self.field(
            node,
            "颜色 · 往返标志",
            record.color.ping_pong,
            at + 0x82,
            2,
        );
        self.field(
            node,
            "颜色 · 重复次数",
            record.color.repetitions,
            at + 0x84,
            2,
        );
        self.field(node, "颜色 · 周期步数", record.color.duration, at + 0x86, 2);
        self.field(
            node,
            "透明度 · 起始原值",
            record.opacity.start,
            at + 0x88,
            2,
        );
        self.field(node, "透明度 · 结束原值", record.opacity.end, at + 0x8a, 2);
        self.field(
            node,
            "透明度 · 重复次数",
            record.opacity.repetitions,
            at + 0x8c,
            2,
        );
        self.field(
            node,
            "透明度 · 周期步数",
            record.opacity.duration,
            at + 0x8e,
            2,
        );
        self.field(
            node,
            "透明度 · 往返标志",
            record.opacity.ping_pong,
            at + 0x90,
            1,
        );
        self.field(
            node,
            "渲染状态 0x60 原值",
            record.render_state_60,
            at + 0x91,
            1,
        );
        self.field(node, "unknown_92", hex(&record.unknown_92), at + 0x92, 2);
        self.field(node, "UV · U 周期", record.uv.u_period, at + 0x94, 2);
        self.field(node, "UV · V 周期", record.uv.v_period, at + 0x96, 2);
        self.field(node, "UV · 循环步数", record.uv.cycle_steps, at + 0x98, 2);
        self.field(node, "UV · 重复次数", record.uv.repetitions, at + 0x9a, 2);
        self.field(
            node,
            "UV · 状态步进参数",
            float(record.uv.state_step_bits),
            at + 0x9c,
            4,
        );
        self.field(node, "UV · 模式原值", record.uv.mode, at + 0xa0, 1);
        self.field(node, "unknown_A1", hex(&record.unknown_a1), at + 0xa1, 7);
        self.field(
            node,
            "武器显隐选择值",
            record.weapon_visibility,
            at + 0xa8,
            1,
        );
        self.field(node, "显隐条件模式", record.visibility_mode, at + 0xa9, 1);
        self.field(
            node,
            "显隐条件标志",
            format!("{:#04X}", record.visibility_flags),
            at + 0xaa,
            1,
        );
        self.field(node, "unknown_AB", record.unknown_ab, at + 0xab, 1);
        self.field(
            node,
            "条件切换标志",
            record.condition_transition,
            at + 0xac,
            1,
        );
        self.field(
            node,
            "终态初始化／额外变换标志",
            record.terminal_transform,
            at + 0xad,
            1,
        );
        self.field(node, "unknown_AE", hex(&record.unknown_ae), at + 0xae, 6);
    }

    fn dat_effect_references(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        ids: &[u16; 8],
        fields_at: usize,
        definition_table: usize,
        base: usize,
    ) {
        let active = ids.iter().position(|&id| id == 0).unwrap_or(ids.len());
        let table = file.table(&dat::EFFECT_TABLES[definition_table]);
        for (slot, &id) in ids.iter().enumerate() {
            let label = if slot < active {
                "特效定义 ID"
            } else {
                "未使用定义槽"
            };
            self.field(node, format!("{label} {slot}"), id, fields_at + slot * 2, 2);
            if slot >= active {
                continue;
            }
            let target = table
                .as_ref()
                .map_err(|error| error.to_string())
                .and_then(|table| {
                    table
                        .record(usize::from(id))
                        .map_err(|error| error.to_string())
                });
            match target {
                Ok((offset, bytes)) => {
                    let buffer = self.document.nodes[node].buffer;
                    let Some(child) = self.child(
                        node,
                        format!("绑定槽 {slot} · 特效定义 {id}"),
                        Kind::DatRecord(dat::DATA_TABLES.len() + definition_table),
                        buffer,
                        base + offset..base + offset + bytes.len(),
                    ) else {
                        return;
                    };
                    self.field(child, "定义 ID", id, fields_at + slot * 2, 2);
                    self.document.nodes[child].deferred = true;
                }
                Err(error) => self.fail(node, format!("特效定义 {id}：{error}")),
            }
        }
    }
}
