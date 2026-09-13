//! Category-specific 32-byte parameter banks accessed by 109A2C60.
//! An identical offset can be a float, integer, or a narrower value in another
//! category/record. Do not impose one eight-word semantic layout on every bank.

use super::FieldLayout;
use crate::binary::ScalarType;

macro_rules! fields {
    ($($offset:literal => $name:literal : $scalar:ident),* $(,)?) => {
        &[$(FieldLayout { name: $name, offset: $offset, scalar: ScalarType::$scalar }),*]
    };
}

// Category 140 has a state-accumulation controller (113F1AA0..113F5FF0),
// not an attack table. State IDs remain numeric: no UI/game names are assumed.
// 113F29F0 copies +0x10's low WORD to the active state lifetime, decremented
// by 113F3180. 113F38B0 / 113F3CC0 use +0x14 as the periodic damage interval.
const PERIODIC_STATE: &[FieldLayout] = fields![
    0x10 => "状态持续计数原值": I32,
    0x14 => "周期扣血间隔原值": I32,
];
const STATE_LIFETIME: &[FieldLayout] = fields![
    0x10 => "状态持续计数原值（低 16 位）": I16,
];
const STATE_1: &[FieldLayout] = fields![
    0x10 => "状态持续计数原值（低 16 位）": I16,
    0x14 => "状态 1 生效参数": I32,
];
const STATE_3: &[FieldLayout] = fields![
    0x10 => "状态持续计数原值": I32,
    0x14 => "周期扣血间隔原值": I32,
    0x18 => "目标积累量增量（低 16 位）": U16,
    0x1c => "目标积累更新间隔": I32,
];
const STATE_4: &[FieldLayout] = fields![
    0x10 => "状态持续计数原值（低 16 位）": I16,
    0x18 => "目标积累量增量（低 16 位）": U16,
    0x1c => "目标积累更新间隔": I32,
];
const STATE_5: &[FieldLayout] = fields![
    0x04 => "状态结束时附加扣血比例": F32,
    0x10 => "状态持续计数原值（低 16 位）": I16,
];
// 113F1D50 polls the global tick counter using record 6, then 113F20F0
// adds the selected increment to the controller gauge and clamps at +0x1c.
const GAUGE: &[FieldLayout] = fields![
    0x10 => "计量增长轮询间隔（低 16 位）": I16,
    0x14 => "条件成立时计量增量": I32,
    0x18 => "一般条件计量增量": I32,
    0x1c => "计量上限／就绪阈值": I32,
];
// 113F2280 and 113F21D0 convert two different accumulated inputs to gauge.
const GAUGE_CONVERSION: &[FieldLayout] = fields![
    0x10 => "累计输入转换阈值": I32,
    0x14 => "每阈值计量增量": I32,
];
// 113F4D00 selects 11 + holder.weapon_kind. 113F4D40 divides this amount
// among the nonzero elemental channels before adding state accumulation.
const WEAPON_ACCUMULATION: &[FieldLayout] = fields![
    0x10 => "该武器种类的命中积累基数": I32,
];
const PARTY_ACCUMULATION: &[FieldLayout] = fields![
    0x10 => "1 人启用时状态积累百分数": I32,
    0x14 => "2 人启用时状态积累百分数": I32,
    0x18 => "3 人启用时状态积累百分数": I32,
    0x1c => "4 人启用时状态积累百分数": I32,
];
const STATE_BASE: &[FieldLayout] = fields![
    0x00 => "基于目标生命值的伤害比例": F32,
    0x10 => "状态触发积累阈值": I32,
];
const STATE_REPEAT: &[FieldLayout] = fields![
    0x00 => "重复触发伤害衰减底数": F32,
    0x10 => "每次触发追加的积累阈值": I32,
];
// 113F5520 searches [100,500) by monster ID and state ID. State 2 also
// compares target action bytes +21/+20, then 113F48E0 checks the stored frame.
const MONSTER_STATE_KEYS: &[FieldLayout] = fields![
    0x10 => "怪物 ID 匹配键": I32,
    0x14 => "状态类别匹配键": I32,
];
const MONSTER_STATE_0: &[FieldLayout] = fields![
    0x10 => "怪物 ID 匹配键": I32,
    0x14 => "状态类别匹配键": I32,
    0x18 => "切换动作大类（低 16 位）": U16,
    0x1c => "切换动作小类（低 16 位）": U16,
];
const MONSTER_STATE_2: &[FieldLayout] = fields![
    0x00 => "动画帧判定值": F32,
    0x10 => "怪物 ID 匹配键": I32,
    0x14 => "状态类别匹配键": I32,
    0x18 => "动作大类／切换目标": I32,
    0x1c => "动作小类／切换目标": I32,
];
// Use the assembly of 113F51A0: the current IDB prototype omits the XMM0
// return and its decompilation drops these reads. Its case order is 0,2,1,3
// for the first four floats; cases 4/5 read the following signed DWORDs.
const MONSTER_COEFFICIENTS: &[FieldLayout] = fields![
    0x00 => "状态 0 伤害系数百分数原值": F32,
    0x04 => "状态 2 伤害系数百分数原值": F32,
    0x08 => "状态 1 伤害系数百分数原值": F32,
    0x0c => "状态 3 伤害系数百分数原值": F32,
    0x10 => "状态 4 伤害系数百分数": I32,
    0x14 => "状态 5 伤害系数百分数": I32,
    // 113F32B0 -> 11234480 -> 1122F740 applies this to the visual XYZ scale.
    0x18 => "状态特效缩放百分数（0 使用 1 倍）": I32,
    0x1c => "怪物 ID 匹配键": I32,
];

pub(super) fn fields(kind: u16, index: usize, bytes: &[u8], bank: &[u8]) -> &'static [FieldLayout] {
    if let Some(fields) = super::weapon_parameters::fields(kind, index, bank)
        .or_else(|| super::effect_parameters::fields(kind, index))
    {
        return fields;
    }
    match (kind, index) {
        (140, 0) => PERIODIC_STATE,
        (140, 1) => STATE_1,
        (140, 2) => STATE_LIFETIME,
        (140, 3) => STATE_3,
        (140, 4) => STATE_4,
        (140, 5) => STATE_5,
        (140, 6) => GAUGE,
        (140, 8 | 9) => GAUGE_CONVERSION,
        (140, 11..=24) => WEAPON_ACCUMULATION,
        (140, 29) => PARTY_ACCUMULATION,
        (140, 30) => STATE_BASE,
        (140, 31) => STATE_REPEAT,
        (140, 100..=499) => match bytes.get(20..24) {
            Some([0, 0, 0, 0]) => MONSTER_STATE_0,
            Some([2, 0, 0, 0]) => MONSTER_STATE_2,
            _ => MONSTER_STATE_KEYS,
        },
        (140, 500..=799) => MONSTER_COEFFICIENTS,
        _ => &[],
    }
}
