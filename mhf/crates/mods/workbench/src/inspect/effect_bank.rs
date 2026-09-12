//! Physical effect-bank tables and definition-to-curve references.

use super::{Builder, Kind, hex, summary};
use crate::field::{FieldType, ScalarType, formatted, typed};
use mhf_resource::effect_archive::{CurveKind, Definition56, EffectBank};

#[cfg(test)]
mod tests;

impl Builder {
    pub(super) fn effect_bank_details(
        &mut self,
        node: usize,
        bank: &EffectBank<'_>,
        base: usize,
    ) -> Result<(), String> {
        self.effect_emitters(node, bank, base);
        for kind in [CurveKind::Vector, CurveKind::Color, CurveKind::Integer] {
            self.effect_curve_keys(node, bank, kind, base)?;
        }
        self.effect_definitions_56(node, bank, base)?;
        let count = bank.definitions_140.len();
        let at = base + bank.table_offsets[5];
        if count != 0
            && let Some(child) = self.child(
                node,
                "定义 · 140 字节",
                Kind::Block,
                self.document.nodes[node].buffer,
                at..at + count * 140,
            )
        {
            self.read::<u16>(child, "count", base + 12)?;
            self.field(
                child,
                "records",
                summary(&bank.definitions_140),
                at,
                count * 140,
            );
        }
        if let Some(lookup) = &bank.motion_lookup {
            self.effect_lookup(node, lookup, base);
        }
        self.effect_events(node, &bank.motion_events, base);
        Ok(())
    }

    fn effect_emitters(&mut self, node: usize, bank: &EffectBank<'_>, base: usize) {
        let buffer = self.document.nodes[node].buffer;
        let at = base + bank.table_offsets[0];
        if bank.emitters.is_empty() {
            return;
        }
        let Some(parent) = self.child(
            node,
            "发射器",
            Kind::Block,
            buffer,
            at..at + bank.emitters.len() * 112,
        ) else {
            return;
        };
        for (index, emitter) in bank.emitters.iter().enumerate() {
            let at = at + index * 112;
            let Some(child) = self.child(
                parent,
                format!("发射器 {index} · {}", emitter.emitter_id),
                Kind::Block,
                buffer,
                at..at + 112,
            ) else {
                break;
            };
            for (i, (name, bits)) in [
                ("position", emitter.position_bits),
                ("position_random", emitter.position_random_bits),
                ("rotation", emitter.rotation_bits),
                ("rotation_random", emitter.rotation_random_bits),
                ("scale", emitter.scale_bits),
                ("scale_random", emitter.scale_random_bits),
            ]
            .into_iter()
            .enumerate()
            {
                self.field(
                    child,
                    name,
                    typed(
                        format!("{:?} · {:08X?}", bits.map(f32::from_bits), bits),
                        FieldType::Array(ScalarType::F32),
                    ),
                    at + i * 12,
                    12,
                );
            }
            self.field(
                child,
                "unknown_48",
                formatted(emitter.unknown_48, format!("{:#010X}", emitter.unknown_48)),
                at + 72,
                4,
            );
            for (name, value, offset) in [
                ("definition_id", emitter.definition_id, 76),
                ("trigger_frame", emitter.trigger_frame, 78),
                ("emitter_id", emitter.emitter_id, 80),
                ("unknown_52", emitter.unknown_52, 82),
                ("flags", emitter.flags, 84),
                ("unknown_56", emitter.unknown_56, 86),
                ("unknown_5a", emitter.unknown_5a, 90),
            ] {
                self.field(child, name, value, at + offset, 2);
            }
            self.field(child, "spawn_count", emitter.spawn_count, at + 88, 2);
            self.field(
                child,
                "unknown_5c",
                formatted(emitter.unknown_5c, format!("{:#010X}", emitter.unknown_5c)),
                at + 92,
                4,
            );
            self.field(child, "unknown_60", hex(&emitter.unknown_60), at + 96, 16);
        }
    }

    fn effect_curve_keys(
        &mut self,
        node: usize,
        bank: &EffectBank<'_>,
        kind: CurveKind,
        base: usize,
    ) -> Result<(), String> {
        let table = kind.table_index();
        let count = usize::from(bank.counts[table]);
        if count == 0 {
            return Ok(());
        }
        let at = base + bank.table_offsets[table];
        let stride = kind.record_size();
        let buffer = self.document.nodes[node].buffer;
        let Some(parent) = self.child(
            node,
            format!("{}曲线", curve_name(kind)),
            Kind::Block,
            buffer,
            at..at + count * stride,
        ) else {
            return Ok(());
        };
        self.read::<u16>(parent, "count", base + 2 + table * 2)?;
        for index in 0..count {
            let at = at + index * stride;
            let id = match kind {
                CurveKind::Vector => u16::from(bank.vector_keys[index].curve_id),
                CurveKind::Color => u16::from(bank.color_keys[index].curve_id),
                CurveKind::Integer => bank.integer_keys[index].curve_id,
            };
            let Some(child) = self.child(
                parent,
                format!("键 {index} · 曲线 {id}"),
                Kind::Block,
                buffer,
                at..at + stride,
            ) else {
                break;
            };
            match kind {
                CurveKind::Vector => {
                    let field = self.read::<[f32; 3]>(child, "value", at)?;
                    let bits = bank.vector_keys[index].value_bits;
                    self.document.nodes[child].fields[field].value =
                        format!("{:?} · {:08X?}", bits.map(f32::from_bits), bits);
                    self.read::<i32>(child, "frame", at + 12)?;
                    self.read_as::<u16>(
                        child,
                        "flags",
                        at + 16,
                        FieldType::Flags(ScalarType::U16),
                    )?;
                    self.read::<u8>(child, "curve_id", at + 18)?;
                    self.read::<u8>(child, "unknown_13", at + 19)?;
                    self.read_as::<[u8; 4]>(child, "unknown_14", at + 20, FieldType::Bytes)?;
                }
                CurveKind::Color => {
                    self.read::<u32>(child, "frame", at)?;
                    self.read_as::<u16>(child, "flags", at + 4, FieldType::Flags(ScalarType::U16))?;
                    self.read::<u8>(child, "curve_id", at + 6)?;
                    self.read::<u8>(child, "unknown_07", at + 7)?;
                    self.read_as::<[u8; 4]>(
                        child,
                        "rgba",
                        at + 8,
                        FieldType::Color { alpha: true },
                    )?;
                    self.read_as::<[u8; 4]>(child, "unknown_0c", at + 12, FieldType::Bytes)?;
                }
                CurveKind::Integer => {
                    self.read::<u32>(child, "frame", at)?;
                    self.read_as::<u16>(child, "flags", at + 4, FieldType::Flags(ScalarType::U16))?;
                    self.read::<u16>(child, "curve_id", at + 6)?;
                    self.read::<i32>(child, "value", at + 8)?;
                    self.read_as::<[u8; 4]>(child, "unknown_0c", at + 12, FieldType::Bytes)?;
                }
            }
        }
        Ok(())
    }

    fn effect_definitions_56(
        &mut self,
        node: usize,
        bank: &EffectBank<'_>,
        base: usize,
    ) -> Result<(), String> {
        let count = bank.definitions_56.len();
        if count == 0 {
            return Ok(());
        }
        let at = base + bank.table_offsets[4];
        let buffer = self.document.nodes[node].buffer;
        let Some(parent) = self.child(
            node,
            "定义 · 56 字节",
            Kind::Block,
            buffer,
            at..at + count * Definition56::SIZE,
        ) else {
            return Ok(());
        };
        self.read::<u16>(parent, "count", base + 10)?;
        for (index, definition) in bank.definitions_56.iter().enumerate() {
            let at = at + index * Definition56::SIZE;
            let Some(child) = self.child(
                parent,
                format!("定义 {index} · ID {}", definition.definition_id),
                Kind::Block,
                buffer,
                at..at + Definition56::SIZE,
            ) else {
                break;
            };
            self.read_as::<u32>(child, "flags", at, FieldType::Flags(ScalarType::U32))?;
            for (name, offset) in [
                ("definition_id", 0x04),
                ("unknown_06", 0x06),
                ("unknown_08", 0x08),
                ("integer_curve_0a", 0x0a),
                ("duration_steps", 0x0c),
                ("position_curve_id", 0x0e),
                ("rotation_curve_id", 0x10),
                ("scale_curve_id", 0x12),
                ("color_curve_id", 0x20),
                ("vector_curve_22", 0x22),
                ("vector_curve_26", 0x26),
            ] {
                self.read::<u16>(child, name, at + offset)?;
            }
            self.read_as::<[u8; 12]>(child, "unknown_14", at + 0x14, FieldType::Bytes)?;
            self.read::<i16>(child, "integer_curve_24", at + 0x24)?;
            self.read_as::<[u8; 16]>(child, "unknown_28", at + 0x28, FieldType::Bytes)?;
            for reference in definition.curve_references() {
                let lookup = bank.curve_lookup(reference);
                let (value, range) = if let Some(native) = lookup.native_range() {
                    let start = base + bank.table_offsets[reference.kind.table_index()];
                    let stride = reference.kind.record_size();
                    let difference = if lookup.is_contiguous() {
                        ""
                    } else {
                        "；匹配位置与原生跨度不同"
                    };
                    (
                        format!(
                            "ID {} · 匹配 {} · 原生 {}..{}（不含上界）{difference}",
                            reference.id,
                            summary(&lookup.matching_indices),
                            native.start,
                            native.end
                        ),
                        start + native.start * stride..start + native.end * stride,
                    )
                } else {
                    (
                        format!("ID {} · 无匹配记录", reference.id),
                        at + reference.offset..at + reference.offset,
                    )
                };
                // References locate their real key-table span through a read-only
                // field. The keys keep their physical table as their only parent.
                self.field(
                    child,
                    format!(
                        "+{:02X} {}曲线引用",
                        reference.offset,
                        curve_name(reference.kind)
                    ),
                    value,
                    range.start,
                    range.len(),
                );
            }
        }
        Ok(())
    }
}

fn curve_name(kind: CurveKind) -> &'static str {
    match kind {
        CurveKind::Vector => "三分量",
        CurveKind::Color => "颜色",
        CurveKind::Integer => "整数",
    }
}
