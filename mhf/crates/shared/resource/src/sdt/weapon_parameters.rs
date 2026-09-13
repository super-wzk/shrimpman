//! Category-specific SDT parameter consumers; unrecognized records stay raw.

use super::FieldLayout;
use crate::binary::ScalarType;

macro_rules! field {
    ($offset:literal => $name:literal : $scalar:ident) => {
        FieldLayout {
            name: $name,
            offset: $offset,
            scalar: ScalarType::$scalar,
        }
    };
}

pub(super) fn fields(kind: u16, index: usize, bank: &[u8]) -> Option<&'static [FieldLayout]> {
    match kind {
        100 => weapon_fields(index, bank),
        999 => status_fields(index),
        _ => None,
    }
}

fn weapon_fields(index: usize, bank: &[u8]) -> Option<&'static [FieldLayout]> {
    match index {
        // 11205050: auxiliary kind selects a bank; holder variant 2 applies it.
        1..=25 => Some(&ATTACK_MODIFIERS[index - 1]),
        27..=51 => Some(&ATTACK_MODIFIERS[index - 27]),
        53..=77 => Some(&ATTACK_MODIFIERS[index - 53]),
        85..=87 => Some(CHAIN_INCREMENTS),
        95 => Some(PART_ACCUMULATION_TIMER),
        98 => Some(SKILL_POWER_OVERRIDE),
        103..=105 => Some(POWER_12_14_27_28),
        107..=109 => Some(STUN_12_14_27_28),
        111 | 113 => Some(POWER_47_48_53_54),
        112 | 114 => Some(POWER_68_69_70),
        116..=118 => Some(PART_ACCUMULATION_MODIFIER),
        120 | 122 => Some(POWER_32_33_34_35),
        121 | 123 => Some(POWER_64_65),
        125 => Some(SHARPNESS_RANGE),
        129 => Some(PART_ACCUMULATION_RANGE),
        136 => Some(WEAPON_VALUE_RANGE),
        147..=149 => Some(POWER_105_106_107_114),
        150..=152 => Some(STUN_105_106_107_114),
        // Fixed native selectors above take precedence if edited data aliases
        // one of their records. Range-driven layouts follow the current bank.
        _ if in_bank_range(bank, 125, index) => Some(SHARPNESS_SLOTS),
        _ if in_bank_range(bank, 129, index) || in_bank_range(bank, 136, index) => {
            Some(CHAIN_PERCENTAGES)
        }
        _ => None,
    }
}

fn in_bank_range(bank: &[u8], selector: usize, index: usize) -> bool {
    let offset = selector * 32 + 16;
    let Some(bounds) = bank.get(offset..offset + 8) else {
        return false;
    };
    let start = i32::from_le_bytes(bounds[..4].try_into().expect("four-byte bound"));
    let end = i32::from_le_bytes(bounds[4..].try_into().expect("four-byte bound"));
    let (Ok(start), Ok(end)) = (usize::try_from(start), usize::try_from(end)) else {
        return false;
    };
    (selector != 125 || start != 0 && end != 0)
        && start <= end
        && end < bank.len() / 32
        && (start..=end).contains(&index)
}

fn status_fields(index: usize) -> Option<&'static [FieldLayout]> {
    match index {
        11 | 14 | 15 => Some(ELEMENT_STATUS_THRESHOLDS),
        12 => Some(ELEMENT_STATUS_RECOVERY),
        13 => Some(ELEMENT_STATUS_TIMERS),
        17 => Some(PERIODIC_STATUS),
        18 => Some(PERIODIC_STATUS_COST),
        19 | 22 => Some(SEVERE_ELEMENT_STATUS),
        20 => Some(SEVERE_STATUS_RECOVERY),
        21 => Some(SEVERE_STATUS_PHASES),
        23 => Some(SEVERE_STATUS_DAMAGE),
        24 => Some(PERIODIC_DAMAGE_WITH_VARIANT),
        25 | 26 | 32 => Some(PERIODIC_DAMAGE),
        31 => Some(STATUS_PROTECTION),
        33 => Some(PROGRESS_STATUS_TIMERS),
        34 => Some(PROGRESS_STATUS_THRESHOLDS),
        35 => Some(PROGRESS_STATUS_RECOVERY),
        36 => Some(PROGRESS_STATUS_HEALTH),
        46 => Some(SEVERE_STATUS_BREAKS_A),
        47 => Some(SEVERE_STATUS_BREAKS_B),
        _ => None,
    }
}

const fn variant_modifier(name: &'static str) -> [FieldLayout; 2] {
    [
        FieldLayout {
            name,
            offset: 0x04,
            scalar: ScalarType::F32,
        },
        field!(0x14 => "修正操作（1替换／2相加／3相乘）" : I32),
    ]
}

const ATTACK_MODIFIERS: [[FieldLayout; 2]; 25] = [
    variant_modifier("启动延迟修正值（变体 2）"),
    variant_modifier("有效阶段修正值（变体 2）"),
    variant_modifier("基础威力修正值（变体 2）"),
    variant_modifier("命中响应类别修正值（变体 2）"),
    variant_modifier("unknown_08修正值（变体 2）"),
    variant_modifier("方向／响应标志修正值（变体 2）"),
    variant_modifier("受击方向角度修正值（变体 2）"),
    variant_modifier("unknown_0d修正值（变体 2）"),
    variant_modifier("防御威力修正值（变体 2）"),
    variant_modifier("伤害属性标志修正值（变体 2）"),
    variant_modifier("判定组索引修正值（变体 2）"),
    variant_modifier("命中音效选择修正值（变体 2）"),
    variant_modifier("属性／异常标志修正值（变体 2）"),
    variant_modifier("属性／异常强度修正值（变体 2）"),
    variant_modifier("命中特效标志修正值（变体 2）"),
    variant_modifier("命中停顿修正值（变体 2）"),
    variant_modifier("气刃槽增量修正值（变体 2）"),
    variant_modifier("重复命中配置修正值（变体 2）"),
    variant_modifier("下一攻击索引修正值（变体 2）"),
    variant_modifier("眩晕累积值修正值（变体 2）"),
    variant_modifier("倍率阶段 A修正值（变体 2）"),
    variant_modifier("倍率阶段 B修正值（变体 2）"),
    variant_modifier("unknown_24修正值（变体 2）"),
    variant_modifier("按部位命中累积量修正值（变体 2）"),
    variant_modifier("辅助参数 +0x08修正值（变体 2）"),
];

const POWER_12_14_27_28: &[FieldLayout] = &[
    field!(0x00 => "攻击 12 的基础威力修正值" : F32),
    field!(0x04 => "攻击 14 的基础威力修正值" : F32),
    field!(0x08 => "攻击 27 的基础威力修正值" : F32),
    field!(0x0c => "攻击 28 的基础威力修正值" : F32),
    field!(0x10 => "攻击 12 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 14 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 27 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 28 的操作（1替换／2相加／3相乘）" : I32),
];

const STUN_12_14_27_28: &[FieldLayout] = &[
    field!(0x00 => "攻击 12 的眩晕修正值" : F32),
    field!(0x04 => "攻击 14 的眩晕修正值" : F32),
    field!(0x08 => "攻击 27 的眩晕修正值" : F32),
    field!(0x0c => "攻击 28 的眩晕修正值" : F32),
    field!(0x10 => "攻击 12 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 14 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 27 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 28 的操作（1替换／2相加／3相乘）" : I32),
];

const POWER_47_48_53_54: &[FieldLayout] = &[
    field!(0x00 => "攻击 47 的基础威力修正值" : F32),
    field!(0x04 => "攻击 48 的基础威力修正值" : F32),
    field!(0x08 => "攻击 53 的基础威力修正值" : F32),
    field!(0x0c => "攻击 54 的基础威力修正值" : F32),
    field!(0x10 => "攻击 47 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 48 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 53 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 54 的操作（1替换／2相加／3相乘）" : I32),
];

const POWER_68_69_70: &[FieldLayout] = &[
    field!(0x00 => "攻击 68 的基础威力修正值" : F32),
    field!(0x04 => "攻击 69 的基础威力修正值" : F32),
    field!(0x08 => "攻击 70 的基础威力修正值" : F32),
    field!(0x10 => "攻击 68 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 69 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 70 的操作（1替换／2相加／3相乘）" : I32),
];

const POWER_32_33_34_35: &[FieldLayout] = &[
    field!(0x00 => "攻击 32 的基础威力修正值" : F32),
    field!(0x04 => "攻击 33 的基础威力修正值" : F32),
    field!(0x08 => "攻击 34 的基础威力修正值" : F32),
    field!(0x0c => "攻击 35 的基础威力修正值" : F32),
    field!(0x10 => "攻击 32 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 33 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 34 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 35 的操作（1替换／2相加／3相乘）" : I32),
];

const POWER_64_65: &[FieldLayout] = &[
    field!(0x00 => "攻击 64 的基础威力修正值" : F32),
    field!(0x04 => "攻击 65 的基础威力修正值" : F32),
    field!(0x10 => "攻击 64 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 65 的操作（1替换／2相加／3相乘）" : I32),
];

const POWER_105_106_107_114: &[FieldLayout] = &[
    field!(0x00 => "攻击 105 的基础威力修正值" : F32),
    field!(0x04 => "攻击 106 的基础威力修正值" : F32),
    field!(0x08 => "攻击 107 的基础威力修正值" : F32),
    field!(0x0c => "攻击 114 的基础威力修正值" : F32),
    field!(0x10 => "攻击 105 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 106 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 107 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 114 的操作（1替换／2相加／3相乘）" : I32),
];

const STUN_105_106_107_114: &[FieldLayout] = &[
    field!(0x00 => "攻击 105 的眩晕修正值" : F32),
    field!(0x04 => "攻击 106 的眩晕修正值" : F32),
    field!(0x08 => "攻击 107 的眩晕修正值" : F32),
    field!(0x0c => "攻击 114 的眩晕修正值" : F32),
    field!(0x10 => "攻击 105 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x14 => "攻击 106 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x18 => "攻击 107 的操作（1替换／2相加／3相乘）" : I32),
    field!(0x1c => "攻击 114 的操作（1替换／2相加／3相乘）" : I32),
];

const PART_ACCUMULATION_MODIFIER: &[FieldLayout] = &[
    field!(0x00 => "按部位累积量修正值" : F32),
    field!(0x10 => "修正操作（1替换／2相加／3相乘）" : I32),
];

const CHAIN_INCREMENTS: &[FieldLayout] = &[
    field!(0x10 => "每连段基础威力增量" : I32),
    field!(0x14 => "每连段眩晕增量" : I32),
    field!(0x18 => "每连段部位累积量增量" : I32),
    field!(0x1c => "每连段辅助参数 +0x08 增量" : I32),
];

const PART_ACCUMULATION_TIMER: &[FieldLayout] = &[field!(0x10 => "部位积累触发后持续计数" : I32)];

const SKILL_POWER_OVERRIDE: &[FieldLayout] = &[
    field!(0x10 => "技能 219/359：攻击 4 基础威力" : U16),
    field!(0x14 => "技能 219/359：攻击 5 基础威力" : U16),
    field!(0x18 => "技能 219/359：攻击 96/97 基础威力" : U16),
];

const SHARPNESS_RANGE: &[FieldLayout] = &[
    field!(0x10 => "斩味修正表起始记录" : I32),
    field!(0x14 => "斩味修正表末尾记录（含）" : I32),
];

const PART_ACCUMULATION_RANGE: &[FieldLayout] = &[
    field!(0x10 => "部位累积连段倍率表起始记录" : I32),
    field!(0x14 => "部位累积连段倍率表末尾记录（含）" : I32),
];

const WEAPON_VALUE_RANGE: &[FieldLayout] = &[
    field!(0x10 => "武器数值连段倍率表起始记录" : I32),
    field!(0x14 => "武器数值连段倍率表末尾记录（含）" : I32),
];

const SHARPNESS_SLOTS: &[FieldLayout] = &[
    field!(0x00 => "攻击索引槽 0" : F32),
    field!(0x04 => "攻击索引槽 1" : F32),
    field!(0x08 => "攻击索引槽 2" : F32),
    field!(0x0c => "攻击索引槽 3" : F32),
    field!(0x10 => "攻击索引槽 0 的斩味增量" : I32),
    field!(0x14 => "攻击索引槽 1 的斩味增量" : I32),
    field!(0x18 => "攻击索引槽 2 的斩味增量" : I32),
    field!(0x1c => "攻击索引槽 3 的斩味增量" : I32),
];

const CHAIN_PERCENTAGES: &[FieldLayout] = &[
    field!(0x00 => "连段倍率槽 0（百分数）" : F32),
    field!(0x04 => "连段倍率槽 1（百分数）" : F32),
    field!(0x08 => "连段倍率槽 2（百分数）" : F32),
    field!(0x0c => "连段倍率槽 3（百分数）" : F32),
    field!(0x10 => "连段倍率槽 4（百分数）" : I32),
    field!(0x14 => "连段倍率槽 5（百分数）" : I32),
    field!(0x18 => "连段倍率槽 6（百分数）" : I32),
    field!(0x1c => "连段倍率槽 7（百分数）" : I32),
];

const ELEMENT_STATUS_THRESHOLDS: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x04 => "强弱异常分界抗性" : F32),
    field!(0x10 => "强异常初始计数" : I32),
    field!(0x14 => "弱异常初始计数" : I32),
];

const ELEMENT_STATUS_RECOVERY: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x04 => "强弱异常分界抗性" : F32),
    field!(0x08 => "弱异常数值恢复倍率" : F32),
    field!(0x0c => "强异常数值恢复倍率" : F32),
    field!(0x10 => "强异常初始计数" : I32),
    field!(0x14 => "弱异常初始计数" : I32),
];

const ELEMENT_STATUS_TIMERS: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x04 => "强弱异常分界抗性" : F32),
    field!(0x10 => "强异常初始计数" : I32),
    field!(0x14 => "弱异常初始计数" : I32),
];

const PERIODIC_STATUS: &[FieldLayout] = &[
    field!(0x10 => "状态持续计数" : I32),
    field!(0x14 => "状态辅助值（写玩家扩展 +1810）" : U16),
    field!(0x18 => "周期扣血开始延迟" : I32),
    field!(0x1c => "扣血间隔计数" : I32),
];

const PERIODIC_STATUS_COST: &[FieldLayout] = &[
    field!(0x10 => "斩味变化间隔计数" : I32),
    field!(0x14 => "每次斩味增量" : I16),
    field!(0x18 => "周期处理间隔" : I32),
    field!(0x1c => "周期处理开始延迟" : I32),
];

const SEVERE_ELEMENT_STATUS: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x10 => "异常初始计数" : I32),
];

const SEVERE_STATUS_RECOVERY: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x08 => "异常期间数值恢复倍率" : F32),
    field!(0x10 => "异常初始计数" : I32),
];

const SEVERE_STATUS_PHASES: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x10 => "异常初始计数" : I32),
    field!(0x1c => "异常结束后等待计数" : U16),
];

const SEVERE_STATUS_DAMAGE: &[FieldLayout] = &[
    field!(0x00 => "异常抗性阈值" : F32),
    field!(0x04 => "状态触发时生命增量" : F32),
    field!(0x10 => "异常初始计数" : I32),
];

const PERIODIC_DAMAGE_WITH_VARIANT: &[FieldLayout] = &[
    field!(0x14 => "扣血间隔计数" : U16),
    field!(0x1c => "变体间隔倍率" : U16),
];

const PERIODIC_DAMAGE: &[FieldLayout] = &[field!(0x14 => "扣血间隔计数" : U16)];

const STATUS_PROTECTION: &[FieldLayout] = &[
    field!(0x00 => "状态期间输入数值倍率" : F32),
    field!(0x10 => "状态特效刷新间隔" : I32),
];

const PROGRESS_STATUS_TIMERS: &[FieldLayout] = &[
    field!(0x10 => "初始状态倒计时" : I32),
    field!(0x14 => "状态 2 倒计时" : U16),
    field!(0x18 => "状态 1 倒计时" : U16),
];

const PROGRESS_STATUS_THRESHOLDS: &[FieldLayout] = &[
    field!(0x10 => "累积目标值 A" : I32),
    field!(0x14 => "累积目标值 B" : I32),
];

const PROGRESS_STATUS_RECOVERY: &[FieldLayout] = &[
    field!(0x00 => "倒计时恢复比例" : F32),
    field!(0x10 => "状态 2 第一阶段会心增量" : I16),
    field!(0x14 => "状态 2 第二阶段会心增量" : I16),
];

const PROGRESS_STATUS_HEALTH: &[FieldLayout] = &[
    field!(0x10 => "状态 1 扣血间隔" : I32),
    field!(0x18 => "状态 2 第二阶段回复间隔" : I32),
    field!(0x1c => "每次回复上限" : I32),
];

const SEVERE_STATUS_BREAKS_A: &[FieldLayout] = &[
    field!(0x10 => "阈值大于 0 时触发参数" : U16),
    field!(0x14 => "阈值低于 300 时触发参数" : U16),
    field!(0x18 => "阈值低于 225 时触发参数" : U16),
];

const SEVERE_STATUS_BREAKS_B: &[FieldLayout] = &[
    field!(0x10 => "阈值低于 150 时触发参数" : U16),
    field!(0x14 => "阈值低于 75 时触发参数" : U16),
];
