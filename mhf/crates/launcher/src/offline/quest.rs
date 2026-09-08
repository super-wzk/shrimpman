const CAPACITY: usize = 0x8000;

pub(super) struct Quest {
    pub(super) bytes: Vec<u8>,
    pub(super) id: u16,
    pub(super) properties: usize,
}

#[cfg(feature = "debug")]
pub(super) struct MonsterQuest {
    pub(super) bytes: Vec<u8>,
    pub(super) spawn_offset: usize,
}

impl Quest {
    pub(super) fn test_map() -> Result<Self, String> {
        let mut quest = Self::parse_image(include_bytes!("quests/55921d0.bin"))?;
        #[cfg(feature = "translation")]
        quest.set_texts(crate::translation::offline_quest::TEST_MAP_TEXTS)?;
        #[cfg(all(feature = "unicode", not(feature = "translation")))]
        quest.convert_source_text()?;
        set_start_area(&mut quest.bytes, 460)?;
        Ok(quest)
    }

    /// Append UTF-8 strings before native relocation. Only the eight text
    /// pointers change; all other sections retain their original offsets.
    #[cfg(feature = "unicode")]
    fn set_texts(&mut self, texts: [&str; 8]) -> Result<(), String> {
        let table = self.text_table()?;
        let mut end = self.bytes.len();
        for text in texts {
            if text.as_bytes().contains(&0) {
                return Err("任务文本不能包含 NUL".into());
            }
            end = end
                .checked_add(text.len())
                .and_then(|end| end.checked_add(1))
                .filter(|end| *end <= CAPACITY)
                .ok_or("任务缓冲区没有足够空间容纳 UTF-8 文本")?;
        }
        self.bytes.reserve(end - self.bytes.len());
        for (index, text) in texts.into_iter().enumerate() {
            let offset = self.bytes.len() as u32;
            write_u32(&mut self.bytes, table + index * 4, offset);
            self.bytes.extend_from_slice(text.as_bytes());
            self.bytes.push(0);
        }
        Ok(())
    }

    #[cfg(feature = "unicode")]
    fn text_table(&self) -> Result<usize, String> {
        let table = u32_at(&self.bytes, self.properties + 0x28)? as usize;
        if table == 0
            || table
                .checked_add(8 * 4)
                .is_none_or(|end| end > self.bytes.len())
        {
            return Err("任务缺少完整的文本指针表".into());
        }
        Ok(table)
    }

    /// The original Japanese BIN/JKR ingress is CP932. Decode it once before
    /// native relocation; later quest copies and monster variants stay UTF-8.
    #[cfg(feature = "unicode")]
    fn convert_source_text(&mut self) -> Result<(), String> {
        let table = self.text_table()?;
        let mut texts: [String; 8] = std::array::from_fn(|_| String::new());
        for (index, text) in texts.iter_mut().enumerate() {
            let offset = u32_at(&self.bytes, table + index * 4)? as usize;
            let source = self
                .bytes
                .get(offset..)
                .filter(|_| offset != 0)
                .ok_or("任务文本偏移超出文件范围")?;
            let end = source
                .iter()
                .position(|byte| *byte == 0)
                .ok_or("任务文本缺少 NUL 结尾")?;
            *text = crate::text::resources::decode_source(&source[..end], 932)?;
        }
        self.set_texts(texts.each_ref().map(String::as_str))
    }

    /// Keep the original monster spawns and extend only their resource list.
    /// The debugger creates its own actor from a separate record after loading;
    /// it must not replace a quest target or enter the primary-target registry.
    #[cfg(feature = "debug")]
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

    /// Read an original Japanese quest file. This is the source-encoding
    /// boundary, never a parser for already prepared runtime quest bytes.
    pub(super) fn parse(input: &[u8]) -> Result<Self, String> {
        let quest = Self::parse_image(input)?;
        #[cfg(feature = "unicode")]
        let quest = {
            let mut quest = quest;
            quest.convert_source_text()?;
            quest
        };
        Ok(quest)
    }

    fn parse_image(input: &[u8]) -> Result<Self, String> {
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

#[cfg(any(feature = "debug", feature = "unicode"))]
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

    #[cfg(feature = "unicode")]
    fn text_at(bytes: &[u8], table: usize, index: usize) -> &str {
        std::str::from_utf8(text_bytes_at(bytes, table, index)).unwrap()
    }

    #[cfg(feature = "translation")]
    #[test]
    fn embedded_quest_localizes_all_text_and_preserves_the_camp_and_other_sections() {
        let original = Quest::parse_image(include_bytes!("quests/55921d0.bin")).unwrap();
        let quest = Quest::test_map().unwrap();
        assert_eq!(quest.id, 55921);
        let table = u32_at(&quest.bytes, quest.properties + 0x28).unwrap() as usize;
        assert_eq!(&quest.bytes[..table], &original.bytes[..table]);
        assert_eq!(
            &quest.bytes[table + 32..original.bytes.len()],
            &original.bytes[table + 32..]
        );
        assert_eq!(quest.bytes[93], 0);
        let hunters = u32_at(&quest.bytes, 4).unwrap() as usize;
        for index in 0..4 {
            assert_eq!(u16_at(&quest.bytes, hunters + index * 16).unwrap(), 460);
        }
        for index in 0..8 {
            let offset = u32_at(&quest.bytes, table + index * 4).unwrap() as usize;
            assert!(offset >= original.bytes.len());
            assert!(!text_at(&quest.bytes, table, index).is_empty());
        }
        assert!(text_at(&quest.bytes, table, 0).contains("古迹"));
        assert_eq!(text_at(&quest.bytes, table, 2), "无");
        assert_eq!(text_at(&quest.bytes, table, 3), "无");
        assert!(text_at(&quest.bytes, table, 7).contains("极驱迅龙"));
        assert_eq!(Quest::parse_image(&quest.bytes).unwrap().id, quest.id);

        #[cfg(feature = "debug")]
        {
            let transformed = quest.with_monster(1, 461, [0.0; 3], 0).unwrap();
            assert!(transformed.spawn_offset >= quest.bytes.len());
            assert!(transformed.bytes.len() <= CAPACITY);
            for index in 0..8 {
                assert_eq!(
                    text_at(&transformed.bytes, table, index),
                    text_at(&quest.bytes, table, index)
                );
            }
        }
    }

    #[cfg(not(feature = "unicode"))]
    #[test]
    fn embedded_quest_preserves_original_bytes_without_unicode_text() {
        let original = Quest::parse_image(include_bytes!("quests/55921d0.bin")).unwrap();
        let quest = Quest::test_map().unwrap();
        assert_eq!(quest.id, original.id);
        assert_eq!(quest.bytes.len(), original.bytes.len());
        let table = u32_at(&original.bytes, original.properties + 0x28).unwrap() as usize;
        assert_eq!(
            u32_at(&quest.bytes, quest.properties + 0x28).unwrap() as usize,
            table
        );
        assert_eq!(
            &quest.bytes[table..table + 32],
            &original.bytes[table..table + 32]
        );
        for index in 0..8 {
            assert_eq!(
                text_bytes_at(&quest.bytes, table, index),
                text_bytes_at(&original.bytes, table, index)
            );
        }
        assert_eq!(quest.bytes[93], 0);
        let hunters = u32_at(&quest.bytes, 4).unwrap() as usize;
        for index in 0..4 {
            assert_eq!(u16_at(&quest.bytes, hunters + index * 16).unwrap(), 460);
        }
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn original_bin_and_jkr_ingress_preserve_japanese_text_as_utf8() {
        let original = Quest::parse_image(include_bytes!("quests/55921d0.bin")).unwrap();
        let table = u32_at(&original.bytes, original.properties + 0x28).unwrap() as usize;
        let compressed = Quest::parse(include_bytes!("quests/55921d0.bin")).unwrap();
        let raw = Quest::parse(&original.bytes).unwrap();
        assert_eq!(compressed.bytes, raw.bytes);
        assert_eq!(raw.id, original.id);
        assert!(raw.bytes.len() <= CAPACITY);
        assert_eq!(&raw.bytes[..table], &original.bytes[..table]);
        assert_eq!(
            &raw.bytes[table + 32..original.bytes.len()],
            &original.bytes[table + 32..]
        );
        let expected = [
            "≪古跡・Ｇ★８遷悠クエスト≫\n迅風が空を裂く",
            "ナルガクルガ１頭の討伐",
            "なし",
            "なし",
            "メインターゲットの達成",
            "３回力尽きる\nタイムアップ",
            "ハンターズギルド",
            "古跡で吹き荒れる\n風のごとく駆けまわり、\n光を放つ白い迅竜。\nギルドでは、ヤツの呼称を\n「極み駆けるナルガクルガ」\nとすることにした。\n勇気のあるハンターよ。\nヤツを討伐した暁には、\nギルド秘蔵の防具の素材を\n必ず授けよう。",
        ];
        #[cfg(feature = "debug")]
        let variant = raw.with_monster(1, 461, [0.0; 3], 0).unwrap();
        for (index, expected) in expected.into_iter().enumerate() {
            assert_eq!(text_at(&raw.bytes, table, index), expected);
            #[cfg(feature = "debug")]
            assert_eq!(text_at(&variant.bytes, table, index), expected);
            let offset = u32_at(&raw.bytes, table + index * 4).unwrap() as usize;
            assert!(offset >= original.bytes.len());
            #[cfg(feature = "debug")]
            assert_eq!(
                u32_at(&variant.bytes, table + index * 4).unwrap() as usize,
                offset
            );
        }
    }

    #[cfg(all(feature = "unicode", not(feature = "translation")))]
    #[test]
    fn unicode_only_test_map_preserves_original_japanese_text() {
        let original = Quest::parse(include_bytes!("quests/55921d0.bin")).unwrap();
        let quest = Quest::test_map().unwrap();
        assert_eq!(quest.bytes.len(), original.bytes.len());
        let table = u32_at(&quest.bytes, quest.properties + 0x28).unwrap() as usize;
        for index in 0..8 {
            assert_eq!(
                text_at(&quest.bytes, table, index),
                text_at(&original.bytes, table, index)
            );
        }
        assert_eq!(
            text_at(&quest.bytes, table, 0),
            "≪古跡・Ｇ★８遷悠クエスト≫\n迅風が空を裂く"
        );
        assert_eq!(quest.bytes[93], 0);
        let hunters = u32_at(&quest.bytes, 4).unwrap() as usize;
        for index in 0..4 {
            assert_eq!(u16_at(&quest.bytes, hunters + index * 16).unwrap(), 460);
        }
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn malformed_source_text_and_full_quest_buffers_are_rejected_before_mutation() {
        for invalid in [0, 1, 2, 3] {
            let mut quest = Quest::parse_image(include_bytes!("quests/55921d0.bin")).unwrap();
            let table = quest.text_table().unwrap();
            let end = quest.bytes.len();
            match invalid {
                0 => write_u32(&mut quest.bytes, table + 7 * 4, end as u32),
                1 => {
                    write_u32(&mut quest.bytes, table + 7 * 4, (end - 1) as u32);
                    quest.bytes[end - 1] = b'X';
                }
                2 => {
                    write_u32(&mut quest.bytes, table + 7 * 4, (end - 2) as u32);
                    quest.bytes[end - 2..].copy_from_slice(&[0x81, 0]);
                }
                _ => quest.bytes.resize(CAPACITY, 0),
            }
            let before = quest.bytes.clone();
            assert!(quest.convert_source_text().is_err());
            assert_eq!(quest.bytes, before);
            assert!(Quest::parse(&before).is_err());
        }
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn invalid_text_tables_are_rejected_before_mutation() {
        for table in [0, 3232 - 31, u32::MAX] {
            let mut quest = Quest::parse_image(include_bytes!("quests/55921d0.bin")).unwrap();
            write_u32(&mut quest.bytes, quest.properties + 0x28, table);
            let before = quest.bytes.clone();
            assert!(quest.set_texts(["文本"; 8]).is_err());
            assert_eq!(quest.bytes, before);
        }
    }

    #[cfg(feature = "unicode")]
    #[test]
    fn invalid_text_and_capacity_overflow_are_rejected_before_mutation() {
        let mut quest = Quest::test_map().unwrap();
        let before = quest.bytes.clone();
        let mut texts = ["文本"; 8];
        texts[7] = "截断\0文本";
        assert!(quest.set_texts(texts).is_err());
        assert_eq!(quest.bytes, before);

        quest.bytes.resize(CAPACITY - 1, 0);
        let before = quest.bytes.clone();
        assert!(quest.set_texts(["文本"; 8]).is_err());
        assert_eq!(quest.bytes, before);
    }

    #[test]
    fn literal_and_overlapping_back_reference() {
        assert_eq!(decode_lz(&[0x40, b'A', 0], 4).unwrap(), b"AAAA");
        assert!(decode_lz(&[0x80, 0], 4).is_err());
        assert!(decode_lz(&[0x40, b'A', 0], 3).is_err());
        assert!(decode_lz(&[0, b'A'], 2).is_err());
    }
}
