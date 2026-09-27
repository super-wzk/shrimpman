use std::borrow::Cow;

use super::is_decimal;

pub(in crate::inspect) fn table_label(name: &str) -> Cow<'_, str> {
    let fixed = match name {
        "armor_descriptions" => "防具说明",
        "head_armor_names" => "头部防具名称",
        "body_armor_names" => "躯干防具名称",
        "arm_armor_names" => "腕部防具名称",
        "waist_armor_names" => "腰部防具名称",
        "leg_armor_names" => "腿部防具名称",
        "melee_weapon_names" => "近战武器名称",
        "ranged_weapon_names" => "远程武器名称",
        "melee_weapon_descriptions" => "近战武器说明",
        "ranged_weapon_descriptions" => "远程武器说明",
        "item_names" => "物品名称",
        "item_messages" => "物品提示文本",
        "item_descriptions" => "物品说明",
        "item_acquisition_hints" => "物品获取提示",
        "hunter_guide_sections" => "猎人指南章节",
        "hunter_basics" => "猎人基础知识",
        "training_tutorials" => "训练教程",
        "dojo_rules" => "道场规则",
        "dojo_rules_legacy" => "道场规则（旧版）",
        "recruitment_messages" => "招募文本",
        "recruitment_conditions" => "招募条件",
        "room_renovations" => "房间改装",
        "poogie_food_descriptions" => "噗吱猪食物说明",
        "caregiver_wares" => "照料员商品",
        "caravan_courses" => "商队路线",
        "special_locations" => "特殊地点",
        "objective_suffixes" => "目标后缀文本",
        "guuku_hat_names" => "咕咕鸭帽子名称",
        "guuku_poncho_names" => "咕咕鸭披风名称",
        "guuku_mask_names" => "咕咕鸭面具名称",
        "guuku_bag_names" => "咕咕鸭背包名称",
        "guuku_cactus_names" => "咕咕鸭仙人掌名称",
        "guuku_hat_descriptions" => "咕咕鸭帽子说明",
        "guuku_poncho_descriptions" => "咕咕鸭披风说明",
        "guuku_mask_descriptions" => "咕咕鸭面具说明",
        "guuku_bag_descriptions" => "咕咕鸭背包说明",
        "guuku_cactus_descriptions" => "咕咕鸭仙人掌说明",
        "guuku_accessory_series" => "咕咕鸭饰品系列",
        "guuku_care_actions" => "咕咕鸭照料动作",
        "guuku_care_messages" => "咕咕鸭照料提示",
        "guuku_personalities" => "咕咕鸭性格",
        "guuku_bed_names" => "咕咕鸭床铺名称",
        "guuku_water_names" => "咕咕鸭水景名称",
        "guuku_floor_names" => "咕咕鸭地板名称",
        "guuku_bed_descriptions" => "咕咕鸭床铺说明",
        "guuku_water_descriptions" => "咕咕鸭水景说明",
        "guuku_floor_descriptions" => "咕咕鸭地板说明",
        "guuku_furnishing_series" => "咕咕鸭家具系列",
        _ => return indexed_label(name),
    };
    Cow::Borrowed(fixed)
}

fn indexed_label(name: &str) -> Cow<'_, str> {
    for (prefix, description) in [
        ("table_", "文本表"),
        ("training_tutorial_", "训练教程"),
        ("training_quest_dialogue_", "训练任务对话"),
        ("caravan_course_group_", "商队路线组"),
    ] {
        if let Some(index) = name.strip_prefix(prefix).filter(|value| is_decimal(value)) {
            return Cow::Owned(format!("{description} {index}"));
        }
    }
    if let Some(section) = name.strip_prefix("hunter_guide_section_") {
        if let Some((section, entry)) = section.split_once("_entry_") {
            if is_decimal(section) && is_decimal(entry) {
                return Cow::Owned(format!("猎人指南章节 {section} · 条目 {entry}"));
            }
        } else if is_decimal(section) {
            return Cow::Owned(format!("猎人指南章节 {section}"));
        }
    }
    Cow::Borrowed(name)
}
