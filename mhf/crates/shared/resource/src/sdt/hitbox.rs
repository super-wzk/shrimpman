use super::FieldLayout;
use crate::binary::ScalarType;

macro_rules! fields {
    ($($offset:literal => $name:literal : $scalar:ident),* $(,)?) => {
        &[$(FieldLayout { name: $name, offset: $offset, scalar: ScalarType::$scalar }),*]
    };
}

// 108C5510 dispatches the first u16 to a node transform or a special mode.
// 10A93DD0 / 10A94290 / 10A94410 / 10A94650 consume only shape 0 or 1.
// Unconsumed bytes stay outside the field layout and remain visible as raw data.
const HEADER: &[FieldLayout] = fields![
    0x00 => "节点索引 / 判定模式": U16,
];
const SPHERE: &[FieldLayout] = fields![
    0x00 => "节点索引 / 判定模式": U16,
    0x02 => "形状（0 球体 / 1 胶囊）": U16,
    0x08 => "判定标志原值": U32,
    0x0c => "半径原值": F32,
    0x10 => "局部中心 X": F32,
    0x14 => "局部中心 Y": F32,
    0x18 => "局部中心 Z": F32,
];
const CAPSULE: &[FieldLayout] = fields![
    0x00 => "节点索引 / 判定模式": U16,
    0x02 => "形状（0 球体 / 1 胶囊）": U16,
    0x08 => "判定标志原值": U32,
    0x0c => "半径原值": F32,
    0x10 => "局部起点 X": F32,
    0x14 => "局部起点 Y": F32,
    0x18 => "局部起点 Z": F32,
    0x1c => "局部终点 X": F32,
    0x20 => "局部终点 Y": F32,
    0x24 => "局部终点 Z": F32,
];
// 10A94360 takes both endpoints from the attack instance. Only mode 119
// additionally translates those endpoints by the stored, scaled local vector.
const SWEPT_OFFSET: &[FieldLayout] = fields![
    0x00 => "动态端点判定模式": U16,
    0x08 => "判定标志原值": U32,
    0x0c => "半径原值": F32,
    0x10 => "端点偏移 X": F32,
    0x14 => "端点偏移 Y": F32,
    0x18 => "端点偏移 Z": F32,
];
const SWEPT: &[FieldLayout] = fields![
    0x00 => "动态端点判定模式": U16,
    0x08 => "判定标志原值": U32,
    0x0c => "半径原值": F32,
];
// 108AF5A0 tests owner +0xb5c against the mask and advances by
// 40 * (1 + signed_skip_count) when any mask bit is present.
const CONDITIONAL_SKIP: &[FieldLayout] = fields![
    0x00 => "条件跳过指令（125）": U16,
    0x02 => "持有者状态条件掩码": U16,
    0x04 => "满足条件时跳过记录数": I16,
];

pub(super) fn fields(bytes: &[u8]) -> &'static [FieldLayout] {
    let Some(header) = bytes.get(..4) else {
        return &[];
    };
    let mode = u16::from_le_bytes([header[0], header[1]]);
    let shape = u16::from_le_bytes([header[2], header[3]]);
    match mode {
        125 => CONDITIONAL_SKIP,
        119 => SWEPT_OFFSET,
        122 | 123 | 126 => SWEPT,
        0..=117 | 120 | 121 | 124 | 127 => match shape {
            0 => SPHERE,
            1 => CAPSULE,
            _ => HEADER,
        },
        _ => HEADER,
    }
}
