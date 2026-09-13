use super::FieldLayout;
use crate::binary::ScalarType;

macro_rules! fields {
    ($($offset:literal => $name:literal : $scalar:ident),* $(,)?) => {
        &[$(FieldLayout { name: $name, offset: $offset, scalar: ScalarType::$scalar }),*]
    };
}

// Auxiliary entries are four DWORDs, but their meanings depend on the effect
// kind. 11322200 and 10AFD0F0 use the attack count as an index bound; this does
// not prove each directory owns that many physically independent entries.
// Unknown kinds intentionally retain all 16 bytes as raw data.
const KIND_100: &[FieldLayout] = &[
    FieldLayout {
        name: "攻击分支类型原值（3 走特殊处理）",
        offset: 0,
        scalar: ScalarType::I32,
    },
    FieldLayout {
        // 108BBF19..108BBF82 adds this to the target's indexed accumulation.
        name: "按部位命中累积量原值",
        offset: 4,
        scalar: ScalarType::I32,
    },
    FieldLayout {
        // 1120526F applies the same integer modifier machinery as +4.
        name: "可缩放参数 +0x08",
        offset: 8,
        scalar: ScalarType::I32,
    },
    FieldLayout {
        // 11204841..11204880 and 11204924..11204955 inspect bits 0..3.
        name: "攻击行为标志原值",
        offset: 12,
        scalar: ScalarType::U32,
    },
];

// 10316120 reads the copied auxiliary DWORD at work +48 for kinds 106/107,
// adds it to attack +0x89, and clamps that stun accumulation byte to 255.
const STUN_INCREMENT: &[FieldLayout] = fields![
    0x00 => "眩晕累积增量": I32,
];

// 102049BD copies these to work +0x3c..+0x48. Mode 45's initializer
// 101FF360 creates velocity and acceleration; 10201D80 integrates both.
const PROJECTILE_160_45: &[FieldLayout] = fields![
    0x00 => "初始竖直速度基数": I32,
    0x04 => "初始前向速度基数": I32,
    0x08 => "竖直加速度": I32,
    0x0c => "速度共用随机扰动范围": I32,
];
// 10200170 / 10200360 compare horizontal distance to the local player
// with these two thresholds, then adjust the rendered opacity/color.
const DISTANCE_BAND_160: &[FieldLayout] = fields![
    0x08 => "显示距离下限": I32,
    0x0c => "显示距离上限": I32,
];
// 10202830 / 10202990 multiply the threshold by the parent effect's scale.
const SCALED_DISTANCE_160: &[FieldLayout] = fields![
    0x08 => "显示距离阈值（乘父实例缩放）": I32,
];
const HIT_INTERVAL_160_47: &[FieldLayout] = fields![
    // 1020280A uses global tick % this value to refresh the collision ID.
    0x00 => "命中标识刷新间隔计数": I32,
];

pub(super) fn fields(kind: u16, index: usize) -> &'static [FieldLayout] {
    match (kind, index) {
        (100, _) => KIND_100,
        (106 | 107, _) => STUN_INCREMENT,
        (160, 16..=18) => DISTANCE_BAND_160,
        (160, 45) => PROJECTILE_160_45,
        (160, 47) => HIT_INTERVAL_160_47,
        (160, 48..=50) => SCALED_DISTANCE_160,
        _ => &[],
    }
}
