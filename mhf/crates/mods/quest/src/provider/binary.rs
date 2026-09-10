use crate::api::MonsterSpawn;

const CAPACITY: usize = 0x8000;

pub(super) struct Quest {
    pub(super) bytes: Vec<u8>,
    pub(super) id: u16,
    #[cfg(windows)]
    pub(super) properties: usize,
}

pub(super) struct MonsterQuest {
    pub(super) bytes: Vec<u8>,
    pub(super) spawn_offset: usize,
}

impl Quest {
    /// Keep original monster spawns while preparing resources and a species variant.
    /// The debugger creates its own actor from a separate record after loading;
    /// it must not replace a quest target or enter the primary-target registry.
    pub(super) fn with_monster(&self, spawn: MonsterSpawn) -> Result<MonsterQuest, String> {
        let MonsterSpawn {
            species,
            variant,
            area,
            position,
            yaw,
        } = spawn;
        if species == 0 || species >= 177 || !position.iter().all(|v| v.is_finite()) {
            return Err("怪物种类或出生位置无效".into());
        }
        if variant > 16 {
            return Err("怪物变种无效".into());
        }
        let original = &self.bytes;
        let section = u32_at(original, 24)? as usize;
        if section == 0
            || section
                .checked_add(16)
                .is_none_or(|end| end > original.len())
        {
            return Err("任务缺少怪物资源段".into());
        }
        let original_ids = u32_at(original, section + 8)? as usize;
        if original_ids == 0 || original_ids == u32::MAX as usize {
            return Err("任务缺少怪物资源列表".into());
        }
        let mut species_ids = Vec::new();
        for index in 0..6 {
            let id = u32_at(original, original_ids + index * 4)?;
            if id == 0 || id == u32::MAX {
                break;
            }
            species_ids.push(id);
        }
        if !species_ids.contains(&u32::from(species)) {
            if species_ids.len() == 6 {
                return Err("任务的 6 个怪物资源槽已满，无法额外载入此种类".into());
            }
            species_ids.push(u32::from(species));
        }
        let properties = u32_at(original, 0)? as usize;
        // Native 1087CB30 indexes variants by the resource-species slot. The
        // leading byte at +0x90 is the reward mode, not a monster variant.
        let variant_offsets = [0x91, 0x92, 0xb6, 0xb7, 0xb8];
        let variant_slots = if u32_at(original, properties + 0x98)? & 0x2000 != 0 {
            5
        } else {
            2
        };
        for (index, &id) in species_ids.iter().enumerate().skip(variant_slots) {
            if id == u32::from(species) && variant != 0 {
                return Err(format!(
                    "任务的第 {} 个怪物资源槽不支持设置变种，请更换怪物种类较少的任务",
                    index + 1
                ));
            }
        }
        let ids = (original.len() + 3) & !3;
        let spawn_offset = ids + 32;
        let end = spawn_offset + 60;
        if end > CAPACITY {
            return Err("任务缓冲区没有足够空间容纳变身数据".into());
        }
        let mut bytes = Vec::with_capacity(end);
        bytes.extend_from_slice(original);
        for (&id, &offset) in species_ids.iter().zip(&variant_offsets[..variant_slots]) {
            if id == u32::from(species) {
                bytes[properties + offset] = variant;
            }
        }
        bytes.resize(end, 0);
        write_u32(&mut bytes, section + 8, ids as u32);
        bytes[ids..spawn_offset].fill(u8::MAX);
        for (index, id) in species_ids.into_iter().enumerate() {
            write_u32(&mut bytes, ids + index * 4, id);
        }
        write_u16(&mut bytes, spawn_offset, u16::from(species));
        bytes[spawn_offset + 4] = 1;
        write_u16(&mut bytes, spawn_offset + 8, area);
        write_u32(&mut bytes, spawn_offset + 28, u32::from(yaw));
        for (index, coordinate) in position.into_iter().enumerate() {
            write_u32(
                &mut bytes,
                spawn_offset + 32 + index * 4,
                coordinate.to_bits(),
            );
        }
        write_u16(&mut bytes, spawn_offset + 48, 100);
        write_u16(&mut bytes, spawn_offset + 50, u16::MAX);
        bytes[spawn_offset + 52] = u8::MAX;
        bytes[spawn_offset + 56] = u8::MAX;
        // +53 remains zero: 10AAA420 must not register this actor as a main
        // objective or overwrite quest +156/+3036 with the controlled instance.

        // Use a fixed hunter spawn in the same area so the new monster is visible
        // immediately. The actor is positioned after the native area initializer.
        set_start_area(&mut bytes[..original.len()], area)?;
        Ok(MonsterQuest {
            bytes,
            spawn_offset,
        })
    }

    /// Parse a BIN or JKR image while preserving its original text encoding.
    pub(super) fn parse(input: &[u8]) -> Result<Self, String> {
        let bytes = if input.starts_with(b"JKR\x1a") {
            if input.len() < 16 || u16_at(input, 6)? != 3 {
                return Err("调试任务目前支持原始 BIN 或 JKR 类型 3 压缩文件".into());
            }
            let offset = u32_at(input, 8)? as usize;
            let size = u32_at(input, 12)? as usize;
            if offset < 16 || offset >= input.len() || !(0x86..=CAPACITY).contains(&size) {
                return Err("任务压缩头的长度或偏移无效".into());
            }
            decode_lz(&input[offset..], size)?
        } else {
            if !(0x86..=CAPACITY).contains(&input.len()) {
                return Err("任务文件长度超出客户端任务缓冲区".into());
            }
            input.to_vec()
        };
        let properties = u32_at(&bytes, 0)? as usize;
        if properties
            .checked_add(320)
            .is_none_or(|end| end > bytes.len())
        {
            return Err("任务属性记录超出文件范围".into());
        }
        let id = u16_at(&bytes, properties + 46)?;
        if id < 40000 {
            return Err("当前离线入口支持编号 40000 以上的活动任务".into());
        }
        // The fourth pointer carries a flag in its high bit; native relocation clears it.
        for offset in (0..68).step_by(4) {
            let pointer = u32_at(&bytes, offset)? & 0x7fff_ffff;
            if pointer as usize >= bytes.len() {
                return Err(format!("任务段偏移越界：{offset:#x} -> {pointer:#x}"));
            }
        }
        Ok(Self {
            bytes,
            id,
            #[cfg(windows)]
            properties,
        })
    }
}

fn set_start_area(bytes: &mut [u8], area: u16) -> Result<(), String> {
    let hunters = u32_at(bytes, 4)? as usize;
    if hunters.checked_add(64).is_none_or(|end| end > bytes.len()) {
        return Err("任务缺少完整的猎人出生记录".into());
    }
    bytes[93] = 0;
    for index in 0..4 {
        write_u16(bytes, hunters + index * 16, area);
    }
    Ok(())
}

fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

fn u16_at(bytes: &[u8], offset: usize) -> Result<u16, String> {
    bytes
        .get(offset..offset + 2)
        .map(|v| u16::from_le_bytes(v.try_into().unwrap()))
        .ok_or_else(|| "任务文件被截断".into())
}

fn u32_at(bytes: &[u8], offset: usize) -> Result<u32, String> {
    bytes
        .get(offset..offset + 4)
        .map(|v| u32::from_le_bytes(v.try_into().unwrap()))
        .ok_or_else(|| "任务文件被截断".into())
}

struct Bits<'a> {
    input: &'a [u8],
    position: usize,
    flag: u8,
    remaining: u8,
}

impl Bits<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let value = *self.input.get(self.position).ok_or("压缩任务文件被截断")?;
        self.position += 1;
        Ok(value)
    }
    fn bit(&mut self) -> Result<usize, String> {
        if self.remaining == 0 {
            self.flag = self.byte()?;
            self.remaining = 8;
        }
        self.remaining -= 1;
        Ok(usize::from((self.flag >> self.remaining) & 1))
    }
}

fn decode_lz(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let mut reader = Bits {
        input,
        position: 0,
        flag: 0,
        remaining: 0,
    };
    let mut output = Vec::with_capacity(size);
    while output.len() < size {
        if reader.bit()? == 0 {
            output.push(reader.byte()?);
            continue;
        }
        let (offset, length) = if reader.bit()? == 0 {
            let length = (reader.bit()? << 1) | reader.bit()?;
            (usize::from(reader.byte()?), length + 3)
        } else {
            let high = usize::from(reader.byte()?);
            let low = usize::from(reader.byte()?);
            let offset = ((high & 31) << 8) | low;
            let length = high >> 5;
            if length != 0 {
                (offset, length + 2)
            } else if reader.bit()? == 0 {
                let mut length = 0;
                for _ in 0..4 {
                    length = length * 2 + reader.bit()?;
                }
                (offset, length + 10)
            } else {
                let length = usize::from(reader.byte()?);
                if length == 255 {
                    if output.len() + offset + 27 > size {
                        return Err("任务解压长度越界".into());
                    }
                    for _ in 0..offset + 27 {
                        output.push(reader.byte()?);
                    }
                    continue;
                }
                (offset, length + 26)
            }
        };
        if offset >= output.len() || output.len() + length > size {
            return Err("任务压缩回溯引用无效".into());
        }
        for _ in 0..length {
            output.push(output[output.len() - offset - 1]);
        }
    }
    Ok(output)
}

#[cfg(test)]
mod tests {
    use super::super::fixtures::monster_spawn;
    use super::*;

    fn text_bytes_at(bytes: &[u8], table: usize, index: usize) -> &[u8] {
        let offset = u32_at(bytes, table + index * 4).unwrap() as usize;
        let bytes = &bytes[offset..];
        let end = bytes.iter().position(|byte| *byte == 0).unwrap();
        &bytes[..end]
    }

    fn quest_with_species(species: &[u32], extended: bool) -> Quest {
        let mut bytes = super::super::fixtures::quest_bytes();
        let properties = u32_at(&bytes, 0).unwrap() as usize;
        let section = u32_at(&bytes, 24).unwrap() as usize;
        let ids = u32_at(&bytes, section + 8).unwrap() as usize;
        for (index, &id) in species.iter().chain([u32::MAX].iter()).enumerate() {
            write_u32(&mut bytes, ids + index * 4, id);
        }
        if extended {
            write_u32(&mut bytes, properties + 0x98, 0x2000);
        }
        Quest::parse(&bytes).unwrap()
    }

    #[test]
    fn monster_variant_uses_resource_slot_and_preserves_other_quest_data() {
        let mut quest = quest_with_species(&[163], false);
        let properties = u32_at(&quest.bytes, 0).unwrap() as usize;
        let section = u32_at(&quest.bytes, 24).unwrap() as usize;
        quest.bytes[properties + 0x90..properties + 0x93].copy_from_slice(&[7, 10, 12]);
        quest.bytes[properties + 0xb6..properties + 0xb9].copy_from_slice(&[13, 14, 15]);
        quest.bytes[properties + 0x97..properties + 0x9b]
            .copy_from_slice(&[0x08, 0x20, 0x03, 0x7f]);
        write_u32(&mut quest.bytes, section + 12, 0x2c0);
        quest.bytes[0x2c0..0x2fc].fill(0xa5);
        let original = quest.bytes.clone();

        let replacement = quest
            .with_monster(MonsterSpawn {
                position: [1.0, 2.0, 3.0],
                yaw: 4,
                ..monster_spawn(15, 16)
            })
            .unwrap();
        let mut expected = original.clone();
        let ids = (original.len() + 3) & !3;
        write_u32(&mut expected, section + 8, ids as u32);
        expected[properties + 0x92] = 16;
        set_start_area(&mut expected, 461).unwrap();
        assert_eq!(&replacement.bytes[..original.len()], expected);
        assert_eq!(u32_at(&replacement.bytes, ids).unwrap(), 163);
        assert_eq!(u32_at(&replacement.bytes, ids + 4).unwrap(), 15);
        assert_eq!(u32_at(&replacement.bytes, ids + 8).unwrap(), u32::MAX);
        assert_eq!(
            u16_at(&replacement.bytes, replacement.spawn_offset).unwrap(),
            15
        );
        assert_eq!(quest.bytes, original);

        let normal = quest.with_monster(monster_spawn(163, 0)).unwrap();
        assert_eq!(normal.bytes[properties + 0x91], 0);
        assert_eq!(normal.bytes[properties + 0x92], 12);
    }

    #[test]
    fn duplicate_species_receive_the_same_variant() {
        let quest = quest_with_species(&[15, 15], false);
        let properties = u32_at(&quest.bytes, 0).unwrap() as usize;
        for variant in [0, 1, 11, 16] {
            let replacement = quest.with_monster(monster_spawn(15, variant)).unwrap();
            assert_eq!(replacement.bytes[properties + 0x91], variant);
            assert_eq!(replacement.bytes[properties + 0x92], variant);
        }
        let mixed_slots = quest_with_species(&[15, 1, 15], false);
        assert!(mixed_slots.with_monster(monster_spawn(15, 1)).is_err());
    }

    #[test]
    fn variant_slots_follow_the_original_interception_flag() {
        let offsets = [0x91, 0x92, 0xb6, 0xb7, 0xb8];
        for extended in [false, true] {
            let slot_count = if extended { 5 } else { 2 };
            for index in 0..6 {
                let species = [1, 11, 15, 17, 21, 48];
                let selected = species[index] as u8;
                let quest = quest_with_species(&species, extended);
                let properties = u32_at(&quest.bytes, 0).unwrap() as usize;
                let result = quest.with_monster(monster_spawn(selected, 16));
                if index < slot_count {
                    assert_eq!(result.unwrap().bytes[properties + offsets[index]], 16);
                } else {
                    assert!(result.is_err());
                }
                assert!(quest.with_monster(monster_spawn(selected, 0)).is_ok());

                // Newly appended species obey the same slot limit.
                let quest = quest_with_species(&species[..index], extended);
                assert_eq!(
                    quest.with_monster(monster_spawn(80, 1)).is_ok(),
                    index < slot_count
                );
                assert!(quest.with_monster(monster_spawn(80, 0)).is_ok());
            }
        }
    }

    #[test]
    fn unsupported_variants_and_full_resource_lists_are_rejected() {
        let quest = quest_with_species(&[1, 11, 15, 17, 21, 48], true);
        for variant in [0, 1, 16] {
            assert!(quest.with_monster(monster_spawn(80, variant)).is_err());
        }
        for variant in [17, u8::MAX] {
            assert!(quest.with_monster(monster_spawn(1, variant)).is_err());
        }
    }

    #[test]
    fn parsing_and_monster_variants_preserve_caller_text() {
        let bytes = super::super::fixtures::quest_bytes();
        let quest = Quest::parse(&bytes).unwrap();
        assert_eq!(quest.bytes, bytes);
        let properties = u32_at(&bytes, 0).unwrap() as usize;
        let table = u32_at(&bytes, properties + 0x28).unwrap() as usize;
        let variant = quest.with_monster(monster_spawn(1, 1)).unwrap();
        assert!(variant.spawn_offset >= bytes.len());
        assert!(variant.bytes.len() <= CAPACITY);
        for index in 0..8 {
            assert_eq!(
                text_bytes_at(&variant.bytes, table, index),
                text_bytes_at(&bytes, table, index)
            );
        }
    }

    #[test]
    fn malformed_images_and_monster_buffer_overflow_are_rejected() {
        assert!(Quest::parse(&[0; 0x85]).is_err());
        assert!(Quest::parse(&vec![0; CAPACITY + 1]).is_err());
        let original = Quest::parse(&super::super::fixtures::quest_bytes()).unwrap();
        for offset in [0, 64] {
            let mut broken = original.bytes.clone();
            write_u32(&mut broken, offset, u32::MAX);
            assert!(Quest::parse(&broken).is_err());
        }
        // Appended monster data must not complete a truncated hunter record.
        let mut incomplete_hunters = original.bytes.clone();
        let hunters = incomplete_hunters.len() - 32;
        write_u32(&mut incomplete_hunters, 4, hunters as u32);
        let quest = Quest::parse(&incomplete_hunters).unwrap();
        assert!(quest.with_monster(monster_spawn(1, 0)).is_err());

        let mut full = original;
        full.bytes.resize(CAPACITY, 0);
        assert!(full.with_monster(monster_spawn(1, 0)).is_err());
    }

    #[test]
    fn literal_and_overlapping_back_reference() {
        assert_eq!(decode_lz(&[0x40, b'A', 0], 4).unwrap(), b"AAAA");
        assert!(decode_lz(&[0x80, 0], 4).is_err());
        assert!(decode_lz(&[0x40, b'A', 0], 3).is_err());
        assert!(decode_lz(&[0, b'A'], 2).is_err());
    }
}
