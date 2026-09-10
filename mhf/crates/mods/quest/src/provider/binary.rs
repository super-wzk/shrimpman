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
    /// Keep the original monster spawns and extend only their resource list.
    /// The debugger creates its own actor from a separate record after loading;
    /// it must not replace a quest target or enter the primary-target registry.
    pub(super) fn with_monster(
        &self,
        species: u8,
        area: u16,
        position: [f32; 3],
        yaw: u16,
    ) -> Result<MonsterQuest, String> {
        if species == 0 || species >= 177 || !position.iter().all(|v| v.is_finite()) {
            return Err("怪物种类或出生位置无效".into());
        }
        let mut bytes = self.bytes.clone();
        let section = u32_at(&bytes, 24)? as usize;
        if section == 0 || section.checked_add(16).is_none_or(|end| end > bytes.len()) {
            return Err("任务缺少怪物资源段".into());
        }
        let original_ids = u32_at(&bytes, section + 8)? as usize;
        if original_ids == 0 || original_ids == u32::MAX as usize {
            return Err("任务缺少怪物资源列表".into());
        }
        let mut species_ids = Vec::new();
        for index in 0..6 {
            let id = u32_at(&bytes, original_ids + index * 4)?;
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
        let ids = (bytes.len() + 3) & !3;
        let spawn = ids + 32;
        let end = spawn + 60;
        if end > CAPACITY {
            return Err("任务缓冲区没有足够空间容纳变身数据".into());
        }
        bytes.resize(end, 0);
        write_u32(&mut bytes, section + 8, ids as u32);
        for offset in (ids..ids + 32).step_by(4) {
            write_u32(&mut bytes, offset, u32::MAX);
        }
        for (index, id) in species_ids.into_iter().enumerate() {
            write_u32(&mut bytes, ids + index * 4, id);
        }
        write_u16(&mut bytes, spawn, u16::from(species));
        bytes[spawn + 4] = 1;
        write_u16(&mut bytes, spawn + 8, area);
        write_u32(&mut bytes, spawn + 28, u32::from(yaw));
        for (index, coordinate) in position.into_iter().enumerate() {
            write_u32(&mut bytes, spawn + 32 + index * 4, coordinate.to_bits());
        }
        write_u16(&mut bytes, spawn + 48, 100);
        write_u16(&mut bytes, spawn + 50, u16::MAX);
        bytes[spawn + 52] = u8::MAX;
        bytes[spawn + 56] = u8::MAX;
        // +53 remains zero: 10AAA420 must not register this actor as a main
        // objective or overwrite quest +156/+3036 with the controlled instance.

        // Use a fixed hunter spawn in the same area so the new monster is visible
        // immediately. The actor is positioned after the native area initializer.
        set_start_area(&mut bytes[..self.bytes.len()], area)?;
        Ok(MonsterQuest {
            bytes,
            spawn_offset: spawn,
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
    use super::*;

    fn text_bytes_at(bytes: &[u8], table: usize, index: usize) -> &[u8] {
        let offset = u32_at(bytes, table + index * 4).unwrap() as usize;
        let bytes = &bytes[offset..];
        let end = bytes.iter().position(|byte| *byte == 0).unwrap();
        &bytes[..end]
    }

    #[test]
    fn parsing_and_monster_variants_preserve_caller_text() {
        let bytes = super::super::fixtures::quest_bytes();
        let quest = Quest::parse(&bytes).unwrap();
        assert_eq!(quest.bytes, bytes);
        let properties = u32_at(&bytes, 0).unwrap() as usize;
        let table = u32_at(&bytes, properties + 0x28).unwrap() as usize;
        let variant = quest.with_monster(1, 461, [0.0; 3], 0).unwrap();
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
        let mut full = original;
        full.bytes.resize(CAPACITY, 0);
        assert!(full.with_monster(1, 461, [0.0; 3], 0).is_err());
    }

    #[test]
    fn literal_and_overlapping_back_reference() {
        assert_eq!(decode_lz(&[0x40, b'A', 0], 4).unwrap(), b"AAAA");
        assert!(decode_lz(&[0x80, 0], 4).is_err());
        assert!(decode_lz(&[0x40, b'A', 0], 3).is_err());
        assert!(decode_lz(&[0, b'A'], 2).is_err());
    }
}
