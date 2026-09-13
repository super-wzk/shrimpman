//! Fields verified against the native 40-byte attack-parameter consumers.
//!
//! Offsets refer to the file record. Native initialization copies the record to
//! effect + 0x68 and then changes several counters and inherited attributes.

use super::FieldLayout;
use crate::binary::ScalarType;

pub(super) const FIELDS: &[FieldLayout] = &[
    FieldLayout {
        name: "启动延迟（原始计数）",
        offset: 0x00,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "有效阶段（原始计数）",
        offset: 0x02,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "基础威力／动作值",
        offset: 0x04,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "命中响应类别",
        offset: 0x06,
        scalar: ScalarType::U16,
    },
    // 11205050 passes these still-unnamed fields to the typed modifier.
    FieldLayout {
        name: "unknown_08",
        offset: 0x08,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "受击方向／响应标志",
        offset: 0x0a,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "受击方向偏移（度）",
        offset: 0x0c,
        scalar: ScalarType::I8,
    },
    FieldLayout {
        name: "unknown_0d",
        offset: 0x0d,
        scalar: ScalarType::I8,
    },
    FieldLayout {
        name: "防御威力",
        offset: 0x0e,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "伤害属性标志",
        offset: 0x0f,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "判定组／内置判定索引",
        offset: 0x10,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "命中音效选择",
        offset: 0x12,
        scalar: ScalarType::U16,
    },
    FieldLayout {
        name: "属性／异常状态标志",
        offset: 0x14,
        scalar: ScalarType::U32,
    },
    FieldLayout {
        name: "属性／异常状态强度",
        offset: 0x18,
        scalar: ScalarType::U32,
    },
    FieldLayout {
        name: "命中特效选择／标志",
        offset: 0x1c,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "命中停顿计数",
        offset: 0x1d,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "太刀气刃槽增量",
        offset: 0x1e,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "重复命中次数／间隔",
        offset: 0x1f,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "下一攻击参数索引",
        offset: 0x20,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "眩晕累积值",
        offset: 0x21,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "倍率阶段 A（原始计数／255）",
        offset: 0x22,
        scalar: ScalarType::U8,
    },
    FieldLayout {
        name: "倍率阶段 B（原始计数）",
        offset: 0x23,
        scalar: ScalarType::U8,
    },
    // 11205050 reads/writes u32; common initialization overwrites this word.
    FieldLayout {
        name: "unknown_24",
        offset: 0x24,
        scalar: ScalarType::U32,
    },
];
