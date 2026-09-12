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
                self.read::<u16>(node, "附着部位代码", at)?;
                self.dat_effect_references(node, file, &record.definition_ids, at + 2, 1, base)?;
            }
            EffectRecordKind::ModelBinding => {
                let record = ModelEffectBinding::parse(bytes).map_err(|error| error.to_string())?;
                self.read::<u16>(node, "模型绑定部位代码", at)?;
                self.read::<u16>(node, "武器种类 ID（仅武器绑定使用）", at + 2)?;
                self.read::<u16>(node, "变体选择值", at + 4)?;
                self.read::<u16>(node, "模型 ID", at + 6)?;
                if record.part_code == 0 {
                    self.field(
                        node,
                        "匹配状态",
                        "索引 0 为保留记录；后续部位 0 终止原生匹配",
                        at,
                        0,
                    );
                }
                self.dat_effect_references(node, file, &record.definition_ids, at + 8, 3, base)?;
            }
            EffectRecordKind::AttachmentDefinition => {
                if bytes.len() != AttachmentDefinition::SIZE {
                    return Err("附着特效定义记录长度不正确".into());
                }
                self.read::<[f32; 3]>(node, "局部位置 XYZ", at)?;
                self.read_fields::<u8>(
                    node,
                    at,
                    &[
                        ("生效条件 ID", 12),
                        ("骨骼节点索引", 13),
                        ("附着模式原值", 14),
                    ],
                )?;
                self.attachment_animation_fields(node, at)?;
            }
            EffectRecordKind::ModelDefinition => {
                if bytes.len() != ModelEffectDefinition::SIZE {
                    return Err("模型特效定义记录长度不正确".into());
                }
                self.read::<[f32; 3]>(node, "节点位移增量 XYZ", at)?;
                self.read_fields::<u8>(
                    node,
                    at,
                    &[
                        ("生效条件 ID", 12),
                        ("绘制组", 13),
                        ("组内条目", 14),
                        ("骨骼节点索引", 15),
                    ],
                )?;
                self.read::<u16>(node, "启动延迟原值", at + 16)?;
                self.model_effect_animation_fields(node, at)?;
            }
        }
        Ok(())
    }

    fn read_fields<T: mhf_resource::binary::BinaryValue + std::fmt::Debug>(
        &mut self,
        node: usize,
        base: usize,
        fields: &[(&str, usize)],
    ) -> Result<(), String> {
        for &(name, offset) in fields {
            self.read::<T>(node, name, base + offset)?;
        }
        Ok(())
    }

    fn attachment_animation_fields(&mut self, node: usize, at: usize) -> Result<(), String> {
        self.read::<u8>(node, "unknown_0F", at + 0x0f)?;
        self.read_fields::<u16>(node, at, &[("默认资源 ID", 0x10), ("启动延迟原值", 0x12)])?;
        self.read_fields::<i16>(
            node,
            at,
            &[
                ("资源序列 · 起始 ID", 0x14),
                ("资源序列 · 结束 ID", 0x16),
                ("资源序列 · 重复次数", 0x18),
                ("资源序列 · 切换间隔", 0x1a),
            ],
        )?;
        for (axis, name) in ["旋转 X", "旋转 Y", "旋转 Z"].into_iter().enumerate() {
            self.effect_rotation_fields(node, name, at + 0x1c + axis * 8)?;
        }
        self.read_fields::<f32>(
            node,
            at,
            &[("统一缩放 · 起始值", 0x34), ("统一缩放 · 结束值", 0x38)],
        )?;
        self.read::<u16>(node, "统一缩放 · 往返标志", at + 0x3c)?;
        self.read::<i16>(node, "统一缩放 · 重复次数", at + 0x3e)?;
        self.read::<u16>(node, "统一缩放 · 周期步数", at + 0x40)?;
        self.read_as::<[u8; 4]>(node, "unknown_42", at + 0x42, FieldType::Bytes)?;
        self.read_as::<u8>(
            node,
            "渲染标志",
            at + 0x46,
            FieldType::Flags(ScalarType::U8),
        )?;
        self.read_as::<[u8; 3]>(node, "朝向矩阵选择标志", at + 0x47, FieldType::Bytes)?;
        self.effect_color_opacity_uv_fields(node, at, 0x4a, 0x56, 0x62)?;
        self.read_as::<[u8; 2]>(node, "unknown_6A", at + 0x6a, FieldType::Bytes)?;
        self.read::<f32>(node, "视线方向偏移", at + 0x6c)?;
        self.read::<u8>(node, "拖尾模式原值", at + 0x70)?;
        self.read_as::<[u8; 3]>(
            node,
            "拖尾 RGB",
            at + 0x71,
            FieldType::Color { alpha: false },
        )?;
        self.read_fields::<u8>(
            node,
            at,
            &[("武器显隐选择值", 0x74), ("显隐条件模式", 0x75)],
        )?;
        self.read_as::<u8>(
            node,
            "显隐条件标志",
            at + 0x76,
            FieldType::Flags(ScalarType::U8),
        )?;
        self.read_as::<[u8; 2]>(node, "unknown_77", at + 0x77, FieldType::Bytes)?;
        self.read::<u8>(node, "终态初始化标志", at + 0x79)?;
        self.read_as::<[u8; 6]>(node, "unknown_7A", at + 0x7a, FieldType::Bytes)
    }

    fn effect_rotation_fields(&mut self, node: usize, name: &str, at: usize) -> Result<(), String> {
        for (label, offset) in [
            ("起始角度（度）", 0),
            ("结束角度（度）", 2),
            ("重复次数", 4),
        ] {
            self.read::<i16>(node, format!("{name} · {label}"), at + offset)?;
        }
        self.read::<u16>(node, format!("{name} · 周期步数"), at + 6)?;
        Ok(())
    }

    fn model_effect_animation_fields(&mut self, node: usize, at: usize) -> Result<(), String> {
        self.read_as::<[u8; 2]>(node, "unknown_12", at + 0x12, FieldType::Bytes)?;
        for (axis, name) in ["旋转 X", "旋转 Y", "旋转 Z"].into_iter().enumerate() {
            let offset = at + 0x18 + axis * 12;
            self.read::<u8>(node, format!("{name} · 状态联动标志"), at + 0x14 + axis)?;
            self.effect_rotation_fields(node, name, offset)?;
            self.read::<f32>(node, format!("{name} · 状态速度倍率"), offset + 8)?;
        }
        self.read::<u8>(node, "unknown_17", at + 0x17)?;
        for (axis, name) in ["缩放 X", "缩放 Y", "缩放 Z"].into_iter().enumerate() {
            let offset = at + 0x3c + axis * 20;
            self.read::<f32>(node, format!("{name} · 起始值"), offset)?;
            self.read::<f32>(node, format!("{name} · 结束值"), offset + 4)?;
            if axis == 2 {
                self.read::<f32>(node, format!("{name} · 状态速度倍率"), offset + 8)?;
            } else {
                self.read::<u32>(node, format!("{name} · 未确认参数"), offset + 8)?;
            }
            self.read::<i16>(node, format!("{name} · 重复次数"), offset + 12)?;
            self.read::<u16>(node, format!("{name} · 周期步数"), offset + 14)?;
            let mode = if axis == 2 {
                "状态插值模式"
            } else {
                "未确认模式"
            };
            self.read::<u8>(node, format!("{name} · {mode}"), offset + 16)?;
            self.read::<u8>(node, format!("{name} · 往返标志"), offset + 17)?;
            self.read_as::<[u8; 2]>(
                node,
                format!("unknown_{:02X}", 0x4e + axis * 20),
                offset + 18,
                FieldType::Bytes,
            )?;
        }
        self.read_as::<u8>(
            node,
            "渲染标志",
            at + 0x78,
            FieldType::Flags(ScalarType::U8),
        )?;
        self.read_as::<[u8; 3]>(node, "unknown_79", at + 0x79, FieldType::Bytes)?;
        self.effect_color_opacity_uv_fields(node, at, 0x7c, 0x88, 0x94)?;
        self.read::<f32>(node, "UV · 状态步进参数", at + 0x9c)?;
        self.read::<u8>(node, "UV · 模式原值", at + 0xa0)?;
        self.read_as::<[u8; 7]>(node, "unknown_A1", at + 0xa1, FieldType::Bytes)?;
        self.read_fields::<u8>(
            node,
            at,
            &[("武器显隐选择值", 0xa8), ("显隐条件模式", 0xa9)],
        )?;
        self.read_as::<u8>(
            node,
            "显隐条件标志",
            at + 0xaa,
            FieldType::Flags(ScalarType::U8),
        )?;
        self.read_fields::<u8>(
            node,
            at,
            &[
                ("unknown_AB", 0xab),
                ("条件切换标志", 0xac),
                ("终态初始化／额外变换标志", 0xad),
            ],
        )?;
        self.read_as::<[u8; 6]>(node, "unknown_AE", at + 0xae, FieldType::Bytes)
    }

    fn effect_color_opacity_uv_fields(
        &mut self,
        node: usize,
        at: usize,
        color: usize,
        opacity: usize,
        uv: usize,
    ) -> Result<(), String> {
        self.read_as::<[u8; 3]>(
            node,
            "颜色 · 起始 RGB",
            at + color,
            FieldType::Color { alpha: false },
        )?;
        self.read_as::<[u8; 3]>(
            node,
            "颜色 · 结束 RGB",
            at + color + 3,
            FieldType::Color { alpha: false },
        )?;
        self.read::<u16>(node, "颜色 · 往返标志", at + color + 6)?;
        self.read::<i16>(node, "颜色 · 重复次数", at + color + 8)?;
        self.read::<u16>(node, "颜色 · 周期步数", at + color + 10)?;
        self.read::<u16>(node, "透明度 · 起始原值", at + opacity)?;
        self.read::<u16>(node, "透明度 · 结束原值", at + opacity + 2)?;
        self.read::<i16>(node, "透明度 · 重复次数", at + opacity + 4)?;
        self.read::<u16>(node, "透明度 · 周期步数", at + opacity + 6)?;
        self.read::<u8>(node, "透明度 · 往返标志", at + opacity + 8)?;
        self.read::<u8>(node, "渲染状态 0x60 原值", at + opacity + 9)?;
        self.read_as::<[u8; 2]>(
            node,
            format!("unknown_{:02X}", opacity + 10),
            at + opacity + 10,
            FieldType::Bytes,
        )?;
        self.read::<i16>(node, "UV · U 周期", at + uv)?;
        self.read::<i16>(node, "UV · V 周期", at + uv + 2)?;
        self.read::<u16>(node, "UV · 循环步数", at + uv + 4)?;
        self.read::<i16>(node, "UV · 重复次数", at + uv + 6)?;
        Ok(())
    }

    fn dat_effect_references(
        &mut self,
        node: usize,
        file: &Dat<'_>,
        ids: &[u16; 8],
        fields_at: usize,
        definition_table: usize,
        base: usize,
    ) -> Result<(), String> {
        let active = ids.iter().position(|&id| id == 0).unwrap_or(ids.len());
        let table = file.table(&dat::EFFECT_TABLES[definition_table]);
        for (slot, &id) in ids.iter().enumerate() {
            let label = if slot < active {
                "特效定义 ID"
            } else {
                "未使用定义槽"
            };
            self.read::<u16>(node, format!("{label} {slot}"), fields_at + slot * 2)?;
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
                        return Ok(());
                    };
                    self.read::<u16>(child, "定义 ID", fields_at + slot * 2)?;
                    self.document.nodes[child].deferred = true;
                }
                Err(error) => self.fail(node, format!("特效定义 {id}：{error}")),
            }
        }
        Ok(())
    }
}
