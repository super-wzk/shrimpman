use super::{RecordCount, RecordFormat, TableLayout};
use crate::binary::ScalarType;

#[derive(Clone, Copy, Debug)]
pub struct FieldLayout {
    /// Stable schema identity, independent of the display label.
    pub key: &'static str,
    /// Presentation label; never parsed to construct a resource identity.
    pub name: &'static str,
    pub offset: u16,
    pub scalar: ScalarType,
}

macro_rules! fields {
    ($($offset:literal => $key:literal, $name:literal : $scalar:ident),* $(,)?) => {
        &[$($crate::dat::FieldLayout { key: $key, name: $name, offset: $offset, scalar: $crate::binary::ScalarType::$scalar }),*]
    };
}
pub(crate) use fields;

// All values are stored values: rarity, element damage and cost are not
// converted to menu units. Unestablished fields remain unknown/raw bytes.
const ARMOR: &[FieldLayout] = fields![
    0x00 => "field_00", "男性模型 ID": I16, 0x02 => "field_02", "女性模型 ID": I16,
    0x04 => "field_04", "可装备标志": U8, 0x05 => "field_05", "稀有度原值": U8,
    0x06 => "field_06", "最高等级": U8, 0x0c => "field_0c", "费用原值": U32,
    0x12 => "field_12", "基础防御力": U16,
    0x14 => "field_14", "火耐性": I8, 0x15 => "field_15", "水耐性": I8,
    0x16 => "field_16", "雷耐性": I8, 0x17 => "field_17", "龙耐性": I8, 0x18 => "field_18", "冰耐性": I8,
    0x19 => "field_19", "强化系数索引": U8,
    0x1b => "field_1b", "初始孔数": U8, 0x1c => "field_1c", "最大孔数": U8,
    0x20 => "field_20", "派生装备 ID 1": U16, 0x22 => "field_22", "派生装备 ID 2": U16,
    0x24 => "field_24", "派生装备 ID 3": U16, 0x26 => "field_26", "特效 ID": U16,
    0x28 => "field_28", "强化素材表索引": U16,
    0x2a => "field_2a", "技能 ID 1": U8, 0x2b => "field_2b", "技能点 1": I8,
    0x2c => "field_2c", "技能 ID 2": U8, 0x2d => "field_2d", "技能点 2": I8,
    0x2e => "field_2e", "技能 ID 3": U8, 0x2f => "field_2f", "技能点 3": I8,
    0x30 => "field_30", "技能 ID 4": U8, 0x31 => "field_31", "技能点 4": I8,
    0x32 => "field_32", "技能 ID 5": U8, 0x33 => "field_33", "技能点 5": I8,
    0x34 => "field_34", "防具类型标志": U32,
    0x46 => "field_46", "辿异技能 ID": U16,
];

const MELEE: &[FieldLayout] = fields![
    0x00 => "field_00", "模型 ID": U16, 0x02 => "field_02", "稀有度原值": U8,
    0x03 => "field_03", "武器种类 ID": U8, 0x04 => "field_04", "费用原值": U32,
    0x08 => "field_08", "斩味索引": U8, 0x09 => "field_09", "斩味上限原值": U8,
    0x0a => "field_0a", "基础攻击力": U16, 0x0c => "field_0c", "防御力": U16,
    0x0e => "field_0e", "会心率": I8, 0x0f => "field_0f", "属性 ID": U8,
    0x10 => "field_10", "属性值原值": U8, 0x11 => "field_11", "异常状态 ID": U8,
    0x12 => "field_12", "异常状态值原值": U8, 0x13 => "field_13", "孔数": U8,
    0x18 => "field_18", "附加模型 ID": U16, 0x1a => "field_1a", "装备类型原值": U8,
    0x1c => "field_1c", "长度原值": U32, 0x20 => "field_20", "武器类型标志": U32,
    0x24 => "field_24", "特效 ID": U16, 0x26 => "field_26", "天廊／G50 参数索引": U16,
    0x28 => "field_28", "G 等级原值": U8, 0x30 => "field_30", "辿异技能 ID": U16,
];

const RANGED: &[FieldLayout] = fields![
    0x00 => "field_00", "模型 ID": U16, 0x02 => "field_02", "稀有度原值": U8,
    0x04 => "field_04", "武器种类 ID": U8, 0x06 => "field_06", "装备类型原值": U8,
    0x0c => "field_0c", "武器类型标志": U32, 0x14 => "field_14", "费用原值": U32,
    0x18 => "field_18", "基础攻击力": U16, 0x1a => "field_1a", "防御力": U16,
    0x1c => "field_1c", "反动原值": U8, 0x1d => "field_1d", "孔数": U8,
    0x1e => "field_1e", "会心率": I8, 0x21 => "field_21", "属性 ID": U8,
    0x22 => "field_22", "属性值原值": U8, 0x23 => "field_23", "装填速度原值": U8,
    0x28 => "field_28", "弹药配置原值": U32, 0x2c => "field_2c", "天廊／G50 参数索引": U16,
    0x30 => "field_30", "G 等级原值": U8, 0x36 => "field_36", "辿异技能 ID": U16,
];

const ITEM: &[FieldLayout] = fields![
    0x00 => "field_00", "交互类型原值": U8, 0x01 => "field_01", "使用标志": U8,
    0x02 => "field_02", "稀有度原值": U8, 0x03 => "field_03", "叠放上限": U8,
    0x04 => "field_04", "物品标志": U8, 0x05 => "field_05", "图标 ID": U8,
    0x06 => "field_06", "图标颜色 ID": U8, 0x08 => "field_08", "瓶配置原值": U16,
    0x0c => "field_0c", "买入价格": U32, 0x10 => "field_10", "卖出价格": U32,
    0x14 => "field_14", "分类原值": U16, 0x16 => "field_16", "装饰品 ID": U16,
    0x1c => "field_1c", "装备类型原值": U16,
];

const PRODUCTION: &[FieldLayout] = fields![
    0x00 => "field_00", "装备部位／种类 ID": U8, 0x01 => "field_01", "购买标志原值": U8,
    0x02 => "field_02", "装备 ID": U16,
    0x04 => "field_04", "素材 ID 1": U16, 0x06 => "field_06", "素材数量 1": U16,
    0x0c => "field_0c", "素材 ID 2": U16, 0x0e => "field_0e", "素材数量 2": U16,
    0x14 => "field_14", "素材 ID 3": U16, 0x16 => "field_16", "素材数量 3": U16,
    0x1c => "field_1c", "素材 ID 4": U16, 0x1e => "field_1e", "素材数量 4": U16,
    0x28 => "field_28", "HR 要求原值": U16, 0x2c => "field_2c", "预览标志原值": U8,
];

const UPGRADE: &[FieldLayout] = fields![
    0x00 => "field_00", "素材 ID 1": U16, 0x02 => "field_02", "素材数量 1": U16,
    0x08 => "field_08", "素材 ID 2": U16, 0x0a => "field_0a", "素材数量 2": U16,
    0x10 => "field_10", "素材 ID 3": U16, 0x12 => "field_12", "素材数量 3": U16,
    0x18 => "field_18", "派生武器 ID 1": U16, 0x1a => "field_1a", "派生武器 ID 2": U16,
    0x1c => "field_1c", "派生武器 ID 3": U16, 0x1e => "field_1e", "派生武器 ID 4": U16,
];

const DECORATION: &[FieldLayout] = fields![
    0x00 => "field_00", "物品 ID": U16, 0x02 => "field_02", "配方分类原值": U16,
    0x04 => "field_04", "素材 ID 1": U16, 0x06 => "field_06", "素材数量 1": U8, 0x07 => "field_07", "解锁标志 1": U8,
    0x08 => "field_08", "素材 ID 2": U16, 0x0a => "field_0a", "素材数量 2": U8, 0x0b => "field_0b", "解锁标志 2": U8,
    0x0c => "field_0c", "素材 ID 3": U16, 0x0e => "field_0e", "素材数量 3": U8, 0x0f => "field_0f", "解锁标志 3": U8,
    0x10 => "field_10", "素材 ID 4": U16, 0x12 => "field_12", "素材数量 4": U8, 0x13 => "field_13", "解锁标志 4": U8,
];

// 105625F0 enumerates 40-byte records using count-block +0x238.
// 10564D20 consumes the byte range at +1/+2 as an inclusive random budget.
const RANDOM_RECIPES: &[FieldLayout] = fields![
    0x00 => "field_00", "分类原值": U8,
    0x01 => "field_01", "随机预算下限原值": U8,
    0x02 => "field_02", "随机预算上限原值": U8,
    0x03 => "field_03", "图标颜色索引": U8,
    0x04 => "field_04", "费用原值": U32,
    0x08 => "field_08", "素材 ID 1 原值": I16, 0x0a => "field_0a", "素材数量 1 原值": I16,
    0x0c => "field_0c", "素材 ID 2 原值": I16, 0x0e => "field_0e", "素材数量 2 原值": I16,
    0x10 => "field_10", "素材 ID 3 原值": I16, 0x12 => "field_12", "素材数量 3 原值": I16,
    0x14 => "field_14", "素材 ID 4 原值": I16, 0x16 => "field_16", "素材数量 4 原值": I16,
    0x18 => "field_18", "素材 ID 5 原值": I16, 0x1a => "field_1a", "素材数量 5 原值": I16,
    0x1c => "field_1c", "素材 ID 6 原值": I16, 0x1e => "field_1e", "素材数量 6 原值": I16,
    0x20 => "field_20", "素材 ID 7 原值": I16, 0x22 => "field_22", "素材数量 7 原值": I16,
    0x24 => "field_24", "素材 ID 8 原值": I16, 0x26 => "field_26", "素材数量 8 原值": I16,
];

// 10562F70 and 105675B0 use the recipe index to select a parallel 64-byte
// record. 10564750 reads eight candidates with stored weights and ranges.
const RANDOM_CANDIDATES: &[FieldLayout] = fields![
    0x00 => "field_00", "候选 ID 1": U16, 0x02 => "field_02", "选取权重 1 原值": U8,
    0x03 => "field_03", "选取标志 1 原值": U8,
    0x04 => "field_04", "数值下限 1 原值": I16, 0x06 => "field_06", "数值上限 1 原值": I16,
    0x08 => "field_08", "候选 ID 2": U16, 0x0a => "field_0a", "选取权重 2 原值": U8,
    0x0b => "field_0b", "选取标志 2 原值": U8,
    0x0c => "field_0c", "数值下限 2 原值": I16, 0x0e => "field_0e", "数值上限 2 原值": I16,
    0x10 => "field_10", "候选 ID 3": U16, 0x12 => "field_12", "选取权重 3 原值": U8,
    0x13 => "field_13", "选取标志 3 原值": U8,
    0x14 => "field_14", "数值下限 3 原值": I16, 0x16 => "field_16", "数值上限 3 原值": I16,
    0x18 => "field_18", "候选 ID 4": U16, 0x1a => "field_1a", "选取权重 4 原值": U8,
    0x1b => "field_1b", "选取标志 4 原值": U8,
    0x1c => "field_1c", "数值下限 4 原值": I16, 0x1e => "field_1e", "数值上限 4 原值": I16,
    0x20 => "field_20", "候选 ID 5": U16, 0x22 => "field_22", "选取权重 5 原值": U8,
    0x23 => "field_23", "选取标志 5 原值": U8,
    0x24 => "field_24", "数值下限 5 原值": I16, 0x26 => "field_26", "数值上限 5 原值": I16,
    0x28 => "field_28", "候选 ID 6": U16, 0x2a => "field_2a", "选取权重 6 原值": U8,
    0x2b => "field_2b", "选取标志 6 原值": U8,
    0x2c => "field_2c", "数值下限 6 原值": I16, 0x2e => "field_2e", "数值上限 6 原值": I16,
    0x30 => "field_30", "候选 ID 7": U16, 0x32 => "field_32", "选取权重 7 原值": U8,
    0x33 => "field_33", "选取标志 7 原值": U8,
    0x34 => "field_34", "数值下限 7 原值": I16, 0x36 => "field_36", "数值上限 7 原值": I16,
    0x38 => "field_38", "候选 ID 8": U16, 0x3a => "field_3a", "选取权重 8 原值": U8,
    0x3b => "field_3b", "选取标志 8 原值": U8,
    0x3c => "field_3c", "数值下限 8 原值": I16, 0x3e => "field_3e", "数值上限 8 原值": I16,
];

const MOTION_EVENT_GROUP_KEYS: &[FieldLayout] = fields![
    0x00 => "field_00", "匹配编号 0": U16,
    0x02 => "field_02", "匹配编号 1": U16,
    0x04 => "field_04", "匹配编号 2": U16,
    0x06 => "field_06", "匹配编号 3": U16,
];

const fn sentinel(root: &'static [u32], stride: u16, width: u8, value: u32) -> RecordCount {
    RecordCount::Sentinel {
        root,
        stride,
        offset: 0,
        width,
        value,
    }
}

const fn table(
    id: &'static str,
    label: &'static str,
    root: &'static [u32],
    stride: u16,
    records: RecordCount,
    fields: &'static [FieldLayout],
    names: Option<u32>,
) -> TableLayout {
    TableLayout {
        id,
        label,
        root,
        stride,
        records,
        first_record: 0,
        format: RecordFormat::Fields(fields),
        directory: None,
        names,
    }
}

macro_rules! terminated_table {
    ($id:literal, $label:literal, $root:literal, $stride:literal, $width:literal, $end:literal, $fields:ident, $names:expr) => {
        table(
            $id,
            $label,
            &[$root],
            $stride,
            sentinel(&[$root], $stride, $width, $end),
            $fields,
            $names,
        )
    };
}

pub static DATA_TABLES: &[TableLayout] = &[
    terminated_table!(
        "head_armor",
        "头部防具",
        0x50,
        72,
        2,
        65535,
        ARMOR,
        Some(0x64)
    ),
    terminated_table!(
        "body_armor",
        "身体防具",
        0x54,
        72,
        2,
        65535,
        ARMOR,
        Some(0x68)
    ),
    terminated_table!(
        "arm_armor",
        "手臂防具",
        0x58,
        72,
        2,
        65535,
        ARMOR,
        Some(0x6c)
    ),
    terminated_table!(
        "waist_armor",
        "腰部防具",
        0x5c,
        72,
        2,
        65535,
        ARMOR,
        Some(0x70)
    ),
    terminated_table!(
        "leg_armor",
        "腿部防具",
        0x60,
        72,
        2,
        65535,
        ARMOR,
        Some(0x74)
    ),
    terminated_table!(
        "melee_weapons",
        "近战武器",
        0x7c,
        52,
        2,
        65535,
        MELEE,
        Some(0x88)
    ),
    terminated_table!(
        "ranged_weapons",
        "远程武器",
        0x80,
        60,
        2,
        65535,
        RANGED,
        Some(0x84)
    ),
    table(
        "items",
        "物品",
        &[0xfc],
        36,
        RecordCount::U16(&[0x10, 8]),
        ITEM,
        Some(0x100),
    ),
    table(
        "melee_upgrades",
        "近战武器强化",
        &[0x3c],
        36,
        sentinel(&[0x7c], 52, 2, 65535),
        UPGRADE,
        Some(0x88),
    ),
    table(
        "ranged_upgrades",
        "远程武器强化",
        &[0x40],
        36,
        sentinel(&[0x80], 60, 2, 65535),
        UPGRADE,
        Some(0x84),
    ),
    terminated_table!(
        "armor_forging",
        "防具生产",
        0x34,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "weapon_forging",
        "武器生产",
        0x38,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "g_weapon_forging",
        "G 级武器生产",
        0x5f0,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "g_armor_forging",
        "G 级防具生产",
        0x5f4,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "special_weapon_forging",
        "特殊武器生产",
        0x7ac,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "premium_armor_forging",
        "付费防具生产",
        0x7b0,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "tower_weapon_forging",
        "天廊武器生产",
        0x940,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "tower_armor_forging",
        "天廊防具生产",
        0x998,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "transmog_forging",
        "外装生产",
        0xab8,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "transmog_forging_2",
        "外装生产 2",
        0xabc,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "zenith_weapon_forging",
        "辿异武器生产",
        0xac0,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "zenith_armor_forging",
        "辿异防具生产",
        0xac4,
        56,
        1,
        255,
        PRODUCTION,
        None
    ),
    terminated_table!(
        "decoration_recipes",
        "装饰品配方",
        0x44,
        20,
        4,
        0,
        DECORATION,
        None
    ),
    terminated_table!(
        "tower_sigil_recipes",
        "天廊印配方",
        0x944,
        20,
        4,
        0,
        DECORATION,
        None
    ),
    terminated_table!(
        "g_decoration_recipes",
        "G 级装饰品配方",
        0xb48,
        20,
        4,
        0,
        DECORATION,
        None
    ),
    table(
        "random_recipes",
        "随机参数配方",
        &[0x620],
        40,
        RecordCount::U16(&[0x10, 0x238]),
        RANDOM_RECIPES,
        None,
    ),
    table(
        "random_candidates",
        "随机参数候选",
        &[0x624],
        64,
        RecordCount::U16(&[0x10, 0x238]),
        RANDOM_CANDIDATES,
        None,
    ),
    table(
        "motion_event_group_keys",
        "动作事件扩展分组匹配",
        &[664 * 4],
        8,
        RecordCount::U32(&[665 * 4]),
        MOTION_EVENT_GROUP_KEYS,
        None,
    ),
];
