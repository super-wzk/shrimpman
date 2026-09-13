use super::FieldLayout;
use crate::binary::ScalarType;

macro_rules! fields {
    ($($offset:literal => $name:literal : $scalar:ident),* $(,)?) => {
        &[$(FieldLayout { name: $name, offset: $offset, scalar: ScalarType::$scalar }),*]
    };
}

// 101E7090 / 101E7190 / 101E7280 / 101E7370 select records 0..3.
// 101E6D60 rotates XYZ by the owner's orientation and adds its position;
// the fourth float extends the other collision anchor along local forward.
const LOCAL_ANCHORS_107: &[FieldLayout] = fields![
    0x00 => "持有者局部生成偏移 X": F32,
    0x04 => "持有者局部生成偏移 Y": F32,
    0x08 => "持有者局部生成偏移 Z": F32,
    0x0c => "另一判定点的前向附加偏移": F32,
];
// 11235670 and 11235850 write effect +0xec and initialize a forward
// motion vector from float 1 * 2.0, rotated by the effect's orientation.
const PROJECTILE_141_1: &[FieldLayout] = fields![
    0x00 => "实例缩放": F32,
    0x04 => "初始前向运动量基数（乘 2）": F32,
];
const PROJECTILE_141_2: &[FieldLayout] = fields![
    0x00 => "实例缩放": F32,
    0x04 => "初始前向运动量基数（乘 2）": F32,
    // 11235D40 uses record 2 +8 when initializing effect record 11.
    0x08 => "后续实例 11 的缩放": F32,
];
const SCALE_141: &[FieldLayout] = fields![
    // 11235A30 / 11235A80 read records 5 / 6 respectively.
    0x00 => "实例缩放": F32,
];
const TRAIL_141_9: &[FieldLayout] = fields![
    // 11235AD0 initializes the motion vector and scale.
    0x00 => "初始前向运动量": F32,
    0x04 => "实例缩放": F32,
    // 11237BC0 spawns the trail at position + motion * float 2.
    0x08 => "尾迹生成位置的运动量倍数": F32,
    0x10 => "尾迹生成间隔计数": I32,
];
const DELAY_141_11: &[FieldLayout] = fields![
    // 11237DE0 creates kind 141 record 86 when the lifetime has decreased
    // by this count from its initial value.
    0x10 => "后续实例 86 的生成延迟计数": I32,
];

pub(super) fn fields(kind: u16, index: usize) -> Option<&'static [FieldLayout]> {
    Some(match (kind, index) {
        (107, 0..=3) => LOCAL_ANCHORS_107,
        (141, 1) => PROJECTILE_141_1,
        (141, 2) => PROJECTILE_141_2,
        (141, 5 | 6) => SCALE_141,
        (141, 9) => TRAIL_141_9,
        (141, 11) => DELAY_141_11,
        _ => return None,
    })
}
