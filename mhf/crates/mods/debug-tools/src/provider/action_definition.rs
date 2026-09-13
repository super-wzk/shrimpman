//! The selected weapon move's DAT[389] animation steps and event commands.
//! 10A67790 consumes steps; 10A68510 dispatches events to weapon callbacks.

pub(super) struct ActionDefinition {
    pub action: super::Action,
    pub motion_style: Option<u8>,
    pub data: Result<Definition, String>,
}

/// 108FD780 selects a resource bank by thousands, then a directory and slot.
/// 1089F8C0 selects wNN.mot, with style-specific files for weapon 11.
pub(super) fn motion_location(weapon: u8, style: Option<u8>, id: u16) -> String {
    let resource = match id / 1000 {
        0 => "共通动作库".into(),
        1 if weapon == 11 => match style {
            Some(0) => "motion/w11.mot".into(),
            Some(1) => "motion/w11ten.mot".into(),
            Some(2) => "motion/w11ran.mot".into(),
            Some(3) => "motion/w11goku.mot".into(),
            _ => "穿龙棍动作库（型未确定）".into(),
        },
        1 if weapon < 14 => format!("motion/w{weapon:02}.mot"),
        2 => "场景动作库".into(),
        3 => "motion/plface_m-pc.mot".into(),
        4 => "motion/plface_f-pc.mot".into(),
        bank => return format!("动画 {id}（未解析资源库 {bank}）"),
    };
    format!(
        "{resource} · 目录记录 {} / 槽位 {}",
        id / 100 % 10,
        id % 100
    )
}

pub(super) struct Definition {
    pub steps: Vec<ActionStep>,
    pub events: Vec<ActionEvent>,
}

pub(super) struct ActionStep(pub [u16; 6]);

pub(super) struct ActionEvent {
    pub step: u16,
    pub timing: u8,
    pub phase: i8,
    pub frame: u16,
    pub count: u16,
    pub operation: u16,
    pub argument: u16,
}

impl ActionEvent {
    pub fn label(&self, weapon: u8) -> String {
        let category = match (weapon, self.operation) {
            (0, 16 | 17)
            | (1 | 5, 8)
            | (2, 10)
            | (3 | 4, 12)
            | (6, 11)
            | (7, 17)
            | (8, 9)
            | (9, 10 | 11)
            | (10, 9) => Some(0),
            (11, 4 | 9 | 10) => Some(100),
            (12, 4) => Some(106),
            (12, 5) => Some(107),
            (13, 3) => Some(120),
            _ => None,
        };
        if let Some(category) = category {
            format!(
                "生成攻击 · mhfsdt.bin / 类别 {category} / 攻击参数 / 记录 {:05}",
                self.argument
            )
        } else {
            format!("操作 {} · 参数 {}", self.operation, self.argument)
        }
    }

    pub fn when(&self) -> String {
        match self.timing {
            1 => "进入步骤时".into(),
            2 => "步骤结束后".into(),
            _ if self.phase > 0 => format!("动作阶段 {}", self.phase),
            _ => format!("帧条件 {} · 计数 {}", self.frame, self.count),
        }
    }
}

pub(super) fn read(bytes: &[u8], base: u32, action: super::Action) -> Result<Definition, String> {
    if action.group != 1 || action.weapon >= 14 {
        return Err("此动作不使用 DAT[389] 武器事件目录，尚未建立静态攻击引用".into());
    }
    let dword = |bytes: &[u8], offset: usize| -> Result<u32, String> {
        bytes
            .get(offset..offset + 4)
            .map(|value| u32::from_le_bytes(value.try_into().unwrap()))
            .ok_or_else(|| "动作定义字段越界".into())
    };
    let table = |pointer: u32, count: u32, stride: usize| -> Result<&[u8], String> {
        if count > 4096 {
            return Err("招式定义记录数量异常".into());
        }
        if count == 0 {
            return Ok(&[]);
        }
        let start = pointer
            .checked_sub(base)
            .ok_or("动作定义引用不在 DAT 资源内")? as usize;
        let end = (count as usize)
            .checked_mul(stride)
            .and_then(|len| start.checked_add(len))
            .ok_or("动作定义数量溢出")?;
        bytes
            .get(start..end)
            .ok_or_else(|| "动作定义引用超出 DAT 资源".into())
    };
    let directory = table(dword(bytes, 389 * 4)?, 14, 8)?;
    let entry = &directory[usize::from(action.weapon) * 8..];
    let count = dword(entry, 0)?;
    if count > 256 || u32::from(action.id) >= count {
        return Err("动作编号超出武器事件目录".into());
    }
    let records = table(dword(entry, 4)?, count, 24)?;
    let record = &records[usize::from(action.id) * 24..];
    let steps = table(dword(record, 4)?, dword(record, 0)?, 12)?;
    let events = table(dword(record, 20)?, dword(record, 16)?, 12)?;
    Ok(Definition {
        steps: steps
            .as_chunks::<12>()
            .0
            .iter()
            .map(|bytes| {
                ActionStep(std::array::from_fn(|i| {
                    u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]])
                }))
            })
            .collect(),
        events: events
            .as_chunks::<12>()
            .0
            .iter()
            .map(|bytes| ActionEvent {
                step: u16::from_le_bytes([bytes[0], bytes[1]]),
                timing: bytes[2],
                phase: bytes[3] as i8,
                frame: u16::from_le_bytes([bytes[4], bytes[5]]),
                count: u16::from_le_bytes([bytes[6], bytes[7]]),
                operation: u16::from_le_bytes([bytes[8], bytes[9]]),
                argument: u16::from_le_bytes([bytes[10], bytes[11]]),
            })
            .collect(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (Vec<u8>, super::super::Action) {
        let mut bytes = vec![0_u8; 0xa00];
        let base = 0x2000_0000_u32;
        for (at, value) in [
            (389 * 4, base + 0x700),
            (0x700 + 7 * 8, 2),
            (0x704 + 7 * 8, base + 0x800),
            (0x818, 2),
            (0x81c, base + 0x900),
            (0x828, 2),
            (0x82c, base + 0x940),
        ] {
            bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
        }
        for (at, words) in [
            (0x900, [3_u16, 47, 0, 4, 0, 8]),
            (0x90c, [4, 49, 2, 4, 12, 3]),
            (0x940, [0, 1, 0, 0, 17, 8]),
            (0x94c, [1, 0, 12, 3, 19, 7]),
        ] {
            for (index, word) in words.iter().enumerate() {
                bytes[at + index * 2..at + index * 2 + 2].copy_from_slice(&word.to_le_bytes());
            }
        }
        (
            bytes,
            super::super::Action {
                weapon: 7,
                group: 1,
                id: 1,
            },
        )
    }

    #[test]
    fn selected_move_keeps_animation_steps_and_their_event_arguments() {
        let (bytes, action) = fixture();
        let definition = read(&bytes, 0x2000_0000, action).unwrap();
        assert_eq!(definition.steps.len(), 2);
        assert_eq!(definition.steps[0].0, [3, 47, 0, 4, 0, 8]);
        assert_eq!(definition.steps[1].0[1], 49);
        assert_eq!(definition.events.len(), 2);
        assert_eq!(definition.events[0].step, 0);
        assert_eq!(
            definition.events[0].label(7),
            "生成攻击 · mhfsdt.bin / 类别 0 / 攻击参数 / 记录 00008"
        );
        assert_eq!(definition.events[1].step, 1);
        assert_eq!(definition.events[1].label(7), "操作 19 · 参数 7");
        assert_eq!(definition.events[1].when(), "帧条件 12 · 计数 3");
    }

    #[test]
    fn invalid_native_references_and_unavailable_moves_are_rejected() {
        let (mut bytes, mut action) = fixture();
        assert!(read(&bytes[..0x94f], 0x2000_0000, action).is_err());
        bytes[0x82c..0x830].copy_from_slice(&0x1fff_ffff_u32.to_le_bytes());
        assert!(read(&bytes, 0x2000_0000, action).is_err());
        action.id = 2;
        assert!(read(&bytes, 0x2000_0000, action).is_err());
        action.group = 0;
        assert!(read(&bytes, 0x2000_0000, action).is_err());
    }

    #[test]
    fn motion_ids_resolve_the_bank_before_the_directory_and_preserve_style() {
        assert_eq!(
            motion_location(7, Some(0), 1405),
            "motion/w07.mot · 目录记录 4 / 槽位 5"
        );
        assert_eq!(
            motion_location(11, Some(3), 1405),
            "motion/w11goku.mot · 目录记录 4 / 槽位 5"
        );
        assert_eq!(
            motion_location(11, None, 1405),
            "穿龙棍动作库（型未确定） · 目录记录 4 / 槽位 5"
        );
        assert_eq!(
            motion_location(7, Some(0), 3405),
            "motion/plface_m-pc.mot · 目录记录 4 / 槽位 5"
        );
        assert_eq!(
            motion_location(7, Some(0), 5405),
            "动画 5405（未解析资源库 5）"
        );
    }
}
