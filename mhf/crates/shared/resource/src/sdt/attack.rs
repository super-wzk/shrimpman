//! Fields verified against the native 40-byte attack-parameter consumers.
//!
//! Offsets refer to the file record. Native initialization copies the record to
//! effect + 0x68 and then changes several counters and inherited attributes.

use crate::dat::{FieldLayout, fields};

pub(super) const FIELDS: &[FieldLayout] = fields![
    0x00 => "startup_count", "启动延迟（原始计数）": U16,
    0x02 => "active_count", "有效阶段（原始计数）": U16,
    0x04 => "power", "基础威力／动作值": U16,
    0x06 => "field_06", "命中响应类别": U16,
    // 11205050 passes these still-unnamed fields to the typed modifier.
    0x08 => "unknown_08", "unknown_08": U16,
    0x0a => "field_0a", "受击方向／响应标志": U16,
    0x0c => "field_0c", "受击方向偏移（度）": I8,
    0x0d => "unknown_0d", "unknown_0d": I8,
    0x0e => "field_0e", "防御威力": U8,
    0x0f => "field_0f", "伤害属性标志": U8,
    0x10 => "field_10", "判定组／内置判定索引": U16,
    0x12 => "field_12", "命中音效选择": U16,
    0x14 => "field_14", "属性／异常状态标志": U32,
    0x18 => "field_18", "属性／异常状态强度": U32,
    0x1c => "field_1c", "命中特效选择／标志": U8,
    0x1d => "field_1d", "命中停顿计数": U8,
    0x1e => "field_1e", "太刀气刃槽增量": U8,
    0x1f => "field_1f", "重复命中次数／间隔": U8,
    0x20 => "field_20", "下一攻击参数索引": U8,
    0x21 => "field_21", "眩晕累积值": U8,
    0x22 => "field_22", "倍率阶段 A（原始计数／255）": U8,
    0x23 => "field_23", "倍率阶段 B（原始计数）": U8,
    // 11205050 reads/writes u32; common initialization overwrites this word.
    0x24 => "unknown_24", "unknown_24": U32,
];
