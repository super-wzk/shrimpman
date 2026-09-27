use std::borrow::Cow;

use crate::inspect::Kind;
use mhf_resource::emd::{ROOT_LABELS, RecordKind};

pub(super) fn label(kind: Kind, name: &str) -> Option<Cow<'_, str>> {
    if !matches!(
        kind,
        Kind::Emd
            | Kind::EmdGroup
            | Kind::EmdSpecies(_)
            | Kind::EmdTable(..)
            | Kind::EmdGlobalTable(_)
            | Kind::EmdRecord(_)
            | Kind::EmdSpeciesTable(..)
            | Kind::EmdAiScript(_)
            | Kind::EmdAiInstruction
    ) {
        return None;
    }

    // root 22's final two multipliers feed health and part-value calculations;
    // root 13 uses the same field keys for a different six-value profile.
    if kind == Kind::EmdRecord(RecordKind::SpeciesModifiers) {
        match name {
            "multiplier_14" => return Some(Cow::Borrowed("生命修正倍率")),
            "multiplier_18" => return Some(Cow::Borrowed("部位耐久修正倍率")),
            _ => {}
        }
    }

    let fixed = match name {
        "species_slot_count" => "物种槽位数",
        "species_id" => "物种编号",
        "root_offset" => "数据表偏移",
        "offset" => "目标偏移",
        "target_offset" => "链接目标偏移",
        "group_offset" => "记录组偏移",
        "record_count" => "记录数",
        "table_07_count" => "物种检索记录数",
        "table_09_count" => "AI 脚本数",
        "table_13_count" => "物种与配置修正记录数",
        "table_14_count" => "三键倍率记录数",
        "table_16_count" => "记录组数",
        "table_17_count" => "物种条件脚本数",
        "table_18_count" => "物种数值记录数",
        "table_19_count" => "物种关联记录数",
        "table_22_count" => "物种修正记录数",
        "scaling_records_offset" => "缩放记录偏移",
        "parameter_banks_offset" => "参数组偏移",
        "parameter_directory_200_offset" => "参数链接目录偏移",
        "initial_actor_3392" => "初始状态值（+0xBC）",
        "initial_actor_2924" => "部位伤害处理方式",
        "initial_actor_834" => "动画通道数",
        "initial_actor_2184" => "状态初始阈值（+0x00）",
        "initial_actor_2176" => "状态初始阈值（+0x0E）",
        "initial_actor_2676" => "眩晕初始阈值",
        "initial_actor_2154" => "状态初始阈值（+0x22）",
        "initial_actor_2168" => "状态初始阈值（+0x2C）",
        "initial_actor_3400" => "状态初始阈值（+0x36）",
        "initial_actor_3414" => "初始状态值（+0x3C）",
        "other_part_recovery_ratio" => "其他部位恢复倍率",
        "request_timer_limit" => "追踪与警戒保持时间",
        "timer_3214_base" => "行为计时基数",
        "display_category" => "显示分类",
        "classification_key" => "分类键",
        "probability_rows_offset" => "概率阈值表偏移",
        "health_multiplier" => "生命倍率",
        "weight" => "权重",
        "value" => "数值（+0x01）",
        "anger_threshold" => "怒气阈值",
        "anger_duration" => "怒态持续时间",
        "actor_2848_value" => "怒态动作速度倍率",
        "actor_2200_multiplier" => "怒态攻击倍率",
        "actor_2204_multiplier" => "怒态承伤倍率",
        "actor_8_value" => "状态值（+0x28）",
        "actor_2154_increment" => "状态阈值增量（目标 +0x22）",
        "actor_2228_limit" => "状态阈值递增次数上限（目标 +0x22）",
        "actor_3389_key" => "状态条件值（+0x01）",
        "actor_2394_key" => "状态条件值（+0x04）",
        "profile_selector_negative_is_wildcard" => "配置条件（负值通配）",
        "selector" => "选择条件（+0x02）",
        "multiplier" => "倍率",
        "anchor_bone_index" => "锚点骨骼索引",
        "anchor_offset_x" => "锚点局部偏移 X",
        "anchor_offset_y" => "锚点局部偏移 Y",
        "anchor_offset_z" => "锚点局部偏移 Z",
        "action_rule_count" => "动作规则数",
        "action_rules_offset" => "动作规则表偏移",
        "result_nonzero" => "规则结果（非零为真）",
        "action_group" => "动作组",
        "action_id" => "动作编号",
        "flags" => "标志位",
        _ => return indexed_label(kind, name),
    };
    Some(Cow::Borrowed(fixed))
}

fn indexed_label(kind: Kind, name: &str) -> Option<Cow<'_, str>> {
    if let Some(index) = index(name, "table_", "_offset") {
        return Some(Cow::Owned(format!(
            "{}偏移（+0x{:02X}）",
            table_name(index)?,
            index * 4
        )));
    }
    if let Some(index) = index(name, "table_", "_count") {
        return Some(Cow::Owned(format!("{}记录数", table_name(index)?)));
    }
    if let Some(index) = index(name, "table_", "") {
        return table_name(index);
    }
    for (prefix, suffix, description) in [
        ("profile_", "_offset", "配置"),
        ("anger_profile_", "_offset", "怒态配置"),
    ] {
        if let Some(index) = index(name, prefix, suffix) {
            return Some(Cow::Owned(format!("{description} {index:02} 偏移")));
        }
    }
    if let Some(index) = index(name, "health_threshold_ratio_profile_", "") {
        return Some(Cow::Owned(format!("配置 {index:02} 生命阈值倍率")));
    }
    if let Some(index) = index(name, "health_base_", "") {
        return Some(Cow::Owned(format!("生命基值 {index}")));
    }
    if let Some(index) = index(name, "anger_gain_health_bucket_", "") {
        return Some(Cow::Owned(format!("生命档位 {index} 怒气增长倍率")));
    }
    for (suffix, description) in [
        ("_initial_value", "耐久基数"),
        ("_health_ratio", "生命倍率"),
        ("_mapped_index", "映射索引"),
        ("_threshold", "反应阈值"),
        ("_threshold_increment", "阈值递增基数"),
        ("_response_kind", "反应类型"),
    ] {
        if let Some(index) = index(name, "part_", suffix) {
            return Some(Cow::Owned(format!("部位 {index} {description}")));
        }
    }
    if let Some(index) = index(name, "threshold_", "") {
        return Some(Cow::Owned(format!("列 {index} 阈值")));
    }
    if kind == Kind::EmdRecord(RecordKind::SpeciesValues)
        && let Some(index) = index(name, "value_", "")
    {
        return Some(Cow::Owned(format!(
            "数值（+0x{:02X}）",
            index.checked_mul(2)?
        )));
    }
    for (prefix, description) in [
        ("value_", "数值"),
        ("key_", "条件值"),
        ("multiplier_", "修正倍率"),
    ] {
        if let Some(offset) = name
            .strip_prefix(prefix)
            .and_then(|value| usize::from_str_radix(value, 16).ok())
        {
            return Some(Cow::Owned(format!("{description}（+0x{offset:02X}）")));
        }
    }
    None
}

fn index(name: &str, prefix: &str, suffix: &str) -> Option<usize> {
    name.strip_prefix(prefix)?
        .strip_suffix(suffix)?
        .parse()
        .ok()
}

fn table_name(index: usize) -> Option<Cow<'static, str>> {
    let name = ROOT_LABELS.get(index)?;
    if matches!(index, 8 | 20 | 23) {
        Some(Cow::Owned(format!("数据表 {index:02}")))
    } else {
        Some(Cow::Borrowed(name))
    }
}
