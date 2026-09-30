//! Display labels for resource formats other than EMD.
//! Internal field names remain stable for edits, drafts and byte bindings.

use crate::inspect::Kind;
use std::borrow::Cow;

use super::is_decimal;

pub(super) fn label(kind: Kind, name: &str) -> Option<Cow<'_, str>> {
    let precise = match (kind, name) {
        (Kind::Jkr, "encoding") => Some("压缩编码"),
        (Kind::Channel, "encoding") => Some("关键帧编码"),
        (Kind::Archive | Kind::Momo | Kind::Mha | Kind::Txb | Kind::EffectArchive, "count") => {
            Some("目录项数")
        }
        (Kind::GroupedMaterials, "count") => Some("材质组数"),
        (Kind::Motion, "count") => Some("轨道数"),
        (Kind::Track, "count") => Some("通道数"),
        (Kind::Dds, "reserved_1") => Some("保留字段组"),
        (Kind::Dds, "reserved_2") => Some("保留字段"),
        (Kind::Dds, "flags") => Some("纹理头标志"),
        // resource/docs/effect-archives.md: the bank header has nine independent
        // counts; table 8's target layout is unknown, not a reserved constant.
        (Kind::EffectBank, "count_0") => Some("发射器数"),
        (Kind::EffectBank, "count_1") => Some("向量曲线关键帧数"),
        (Kind::EffectBank, "count_2") => Some("颜色曲线关键帧数"),
        (Kind::EffectBank, "count_3") => Some("整数曲线关键帧数"),
        (Kind::EffectBank, "count_4") => Some("56 字节定义数"),
        (Kind::EffectBank, "count_5") => Some("140 字节定义数"),
        (Kind::EffectBank, "count_6") => Some("动作查找槽数"),
        (Kind::EffectBank, "count_7") => Some("动作事件数"),
        (Kind::EffectBank, "count_8") => Some("数据表 08 计数"),
        // resource/docs/attack-parameters.md, 40-byte attack parameters:
        // +04 participates in damage calculation; counters remain stored units.
        (Kind::SdtAttack, "基础威力／动作值") => Some("基础威力"),
        (Kind::SdtAttack, "判定组／内置判定索引") => Some("判定索引（分组／内置）"),
        (Kind::SdtAttack, "命中音效选择") => Some("命中音效选择值"),
        (Kind::SdtAttack, "命中特效选择／标志") => Some("命中特效选择与标志"),
        (Kind::SdtAttack, "倍率阶段 A（原始计数／255）") => {
            Some("倍率阶段 A 原始计数（255 为特殊值）")
        }
        (Kind::SdtAuxiliary, "攻击分支类型原值（3 走特殊处理）") => {
            Some("攻击分支类型（3：特殊分支）")
        }
        (Kind::SdtAuxiliary, "按部位命中累积量原值") => Some("按部位命中累积量"),
        (Kind::SdtAuxiliary, "攻击行为标志原值") => Some("攻击行为标志"),
        // The modifier targets an attack-record offset. Its own stored value
        // is at +04, so retaining the target record name avoids offset confusion.
        // See attack-parameters.md, category-100 modifier banks / 11205050.
        (Kind::SdtExtra, "unknown_08修正值（变体 2）") => {
            Some("攻击参数 +0x08 修正值（变体 2）")
        }
        (Kind::SdtExtra, "unknown_0d修正值（变体 2）") => {
            Some("攻击参数 +0x0D 修正值（变体 2）")
        }
        (Kind::SdtExtra, "unknown_24修正值（变体 2）") => {
            Some("攻击参数 +0x24 修正值（变体 2）")
        }
        (Kind::SdtExtra, "倍率阶段 A修正值（变体 2）") => {
            Some("倍率阶段 A 修正值（变体 2）")
        }
        (Kind::SdtExtra, "倍率阶段 B修正值（变体 2）") => {
            Some("倍率阶段 B 修正值（变体 2）")
        }
        (Kind::SdtExtra, "辅助参数 +0x08修正值（变体 2）") => {
            Some("辅助参数 +0x08 修正值（变体 2）")
        }
        _ => None,
    };
    if let Some(label) = precise {
        return Some(Cow::Borrowed(label));
    }

    // These keys have unique meanings, or deliberately retain a generic name
    // because Block is shared by unrelated record types. Material colors and
    // placement vectors retain offsets; the parsers do not establish more
    // specific rendering/transform roles. duration_steps is an age threshold,
    // not seconds (resource/docs/effect-archives.md).
    let text = match name {
        "key_index" => "密钥索引",
        "filename_checksum" => "文件名校验值",
        "payload_size" => "载荷长度",
        "CRC32" => "CRC32 校验值",
        "seed" => "种子",
        "version" => "版本",
        "encoding" => "编码方式",
        "data_offset" => "数据偏移",
        "decoded_size" => "解码后长度",
        "count" => "数量",
        "offset" => "偏移",
        "size" => "字节长度",
        "additional_count" => "附加资源数",
        "resource_id" => "资源 ID",
        "width" => "宽度",
        "height" => "高度",
        "bit_depth" => "位深",
        "color_type" => "颜色类型",
        "compression_method" => "压缩方式",
        "filter_method" => "滤波方式",
        "interlace_method" => "交错方式",
        "pixel_format.size" => "像素格式结构长度",
        "pixel_format.flags" => "像素格式标志",
        "length" => "数据长度",
        "kind" => "类型",
        "flags" => "标志位",
        "pitch_or_linear_size" => "行距／线性长度",
        "depth" => "深度",
        "mip_map_count" => "多级纹理层数",
        "rgb_bit_count" => "像素位数",
        "r_bit_mask" => "红色位掩码",
        "g_bit_mask" => "绿色位掩码",
        "b_bit_mask" => "蓝色位掩码",
        "a_bit_mask" => "透明度位掩码",
        "caps" => "能力标志",
        "caps_2" => "能力标志 2",
        "caps_3" => "能力标志 3",
        "caps_4" => "能力标志 4",
        "four_cc" => "四字符编码",
        "dxgi_format" => "DXGI 格式",
        "resource_dimension" => "资源维度",
        "misc_flag" => "附加标志",
        "array_size" => "数组元素数",
        "misc_flags_2" => "附加标志 2",
        "parameter_30" => "浮点参数 +0x30",
        "texture_indices" => "贴图索引",
        "image_id" => "图像 ID",
        "trailing" => "尾部原始字节",
        "indices" => "索引列表",
        "node_id" => "节点 ID",
        "parent_index" => "父节点索引",
        "first_child_index" => "首个子节点索引",
        "next_sibling_index" => "下一个同级节点索引",
        "scale" => "缩放",
        "rotation" => "旋转",
        "translation" => "平移",
        "lookup_count" => "动作查找槽数",
        "event_count" => "事件数",
        "step_count" => "步骤数",
        "steps_offset" => "步骤表偏移",
        "transition_count" => "派生条件数",
        "transitions_offset" => "派生条件表偏移",
        "events_offset" => "事件表偏移",
        "records_offset" => "记录表偏移",
        "priority" => "优先级原值",
        "input" => "输入原值",
        "selection" => "选择原值",
        "step" => "步骤",
        "timing" => "时机原值",
        "phase" => "阶段原值",
        "operation" => "操作码",
        "argument" => "参数原值",
        "start" => "起始动作 ID",
        "end（不含）" => "结束动作 ID（不含）",
        "event_indices" => "事件索引",
        "position" => "位置",
        "motion_id" => "动作 ID",
        "frame" => "帧",
        "node_index" => "节点索引",
        "emitter_id" => "发射器 ID",
        "version_marker" => "格式标记",
        "offsets_offset" => "偏移目录位置",
        "motion_offset" => "动作数据偏移",
        "metadata_present" => "附加数据存在标记",
        "metadata" => "附加数据原值",
        "native_key_count" => "关键帧计数",
        "motion_tag（动画分组）" => "动画分组标记",
        "byte_size" => "块字节长度",
        "target_id" => "目标 ID",
        "render_mode" => "渲染模式",
        "sequence_id" => "序列 ID",
        "channel" => "通道",
        "sequence_flags" => "序列标志",
        "sequence_delay" => "序列延迟",
        "animation_id" => "动画 ID",
        "animation_group_id" => "光照动画组 ID",
        "first_light_id" => "首个光源 ID",
        "last_light_id" => "末个光源 ID",
        "match_04" => "匹配值 +0x04",
        "match_06" => "匹配值 +0x06",
        "value" => "数值",
        "control" => "控制字",
        "records" => "记录数据",
        "position_random" => "位置随机量",
        "rotation_random" => "旋转随机量",
        "scale_random" => "缩放随机量",
        "definition_id" => "定义 ID",
        "trigger_frame" => "触发帧",
        "spawn_count" => "生成数量",
        "curve_id" => "曲线 ID",
        "rgba" => "颜色（含透明度）",
        "duration_steps" => "持续计数阈值",
        "position_curve_id" => "位置曲线 ID",
        "rotation_curve_id" => "旋转曲线 ID",
        "scale_curve_id" => "缩放曲线 ID",
        "color_curve_id" => "颜色曲线 ID",
        "entries_offset" => "目录偏移",
        "names_offset" => "名称区偏移",
        "names_size" => "名称区长度",
        "first_file_id" => "起始文件 ID",
        "file_id_count" => "文件 ID 槽数",
        "name_offset" => "名称偏移",
        "padded_size" => "含填充长度",
        "file_id" => "文件 ID",
        "file_id_high_raw" => "文件 ID 高位原值",
        "header_size" => "头部长度",
        "categories_offset" => "任务分类目录偏移",
        "counts_offset" => "计数表偏移",
        "category_count" => "分类数量",
        "quest_id_limit" => "任务 ID 上界",
        "slot_count" => "槽数",
        "slots_offset" => "槽表偏移",
        "slot_offset" => "槽引用偏移",
        "text_table_offset" => "文本目录偏移",
        "quest_id" => "任务 ID",
        "header.unknown" => "头部未知字节",
        "u32 words" => "32 位字数据",
        "reserved_09" => "保留字节 +0x09",
        "unknown_tail" => "尾部未知字节",
        "color_00" => "颜色参数 +0x00",
        "color_10" => "颜色参数 +0x10",
        "color_20" => "颜色参数 +0x20",
        "color_02" => "颜色参数 +0x02",
        "vector_00" => "向量参数 +0x00",
        "vector_0c" => "向量参数 +0x0C",
        "vector_20" => "向量参数 +0x20",
        "integer_curve_0a" => "整数曲线 ID +0x0A",
        "integer_curve_24" => "整数曲线 ID +0x24",
        "vector_curve_22" => "向量曲线 ID +0x22",
        "vector_curve_26" => "向量曲线 ID +0x26",
        "short_14" => "16 位原始值 +0x14",
        "short_16" => "16 位原始值 +0x16",
        _ => return dynamic_label(name),
    };
    Some(Cow::Borrowed(text))
}

fn dynamic_label(name: &str) -> Option<Cow<'_, str>> {
    for (prefix, label) in [
        ("扩展 word_", "扩展原始值"),
        ("扩展 color_", "扩展颜色参数"),
    ] {
        if let Some(offset) = name
            .strip_prefix(prefix)
            .and_then(|value| usize::from_str_radix(value, 16).ok())
        {
            return Some(Cow::Owned(format!("{label} +0x{offset:02X}")));
        }
    }
    if let Some(index) = name
        .strip_prefix("text_")
        .and_then(|rest| rest.strip_suffix("_offset"))
        && is_decimal(index)
    {
        return Some(Cow::Owned(format!("文本 {index} 偏移")));
    }
    if let Some((index, field)) = name
        .strip_prefix('[')
        .and_then(|rest| rest.split_once("]."))
        && is_decimal(index)
    {
        let label = match field {
            "kind" => "类型",
            "resource_id" => "资源 ID",
            _ => return None,
        };
        return Some(Cow::Owned(format!("资源 {index} {label}")));
    }
    if let Some(index) = name
        .strip_prefix("成员 ")
        .and_then(|rest| rest.strip_suffix(" kind"))
        && is_decimal(index)
    {
        return Some(Cow::Owned(format!("成员 {index} 类型")));
    }
    None
}
