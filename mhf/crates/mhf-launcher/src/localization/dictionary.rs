use super::{TRANSLATION_ENTRY_SIZE, TranslationKey};
use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
};
use unicode_width::UnicodeWidthChar;

pub(super) struct CompiledLocale {
    id: &'static str,
    entries_offset: usize,
    entries_len: usize,
}

impl CompiledLocale {
    pub(super) const fn new(id: &'static str, entries_offset: usize, entries_len: usize) -> Self {
        Self {
            id,
            entries_offset,
            entries_len,
        }
    }
}

pub(super) struct CompiledDictionary {
    bytes: &'static [u8],
    locales: &'static [CompiledLocale],
}

impl CompiledDictionary {
    pub(super) const fn new(bytes: &'static [u8], locales: &'static [CompiledLocale]) -> Self {
        Self { bytes, locales }
    }

    pub(super) fn locale(&self, id: &str) -> Option<&'static CompiledLocale> {
        self.locales
            .iter()
            .find(|locale| locale.id.eq_ignore_ascii_case(id))
    }

    pub(super) fn locale_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.locales.iter().map(|locale| locale.id)
    }

    pub(super) fn encode_locale(&'static self, locale: &'static CompiledLocale) -> RuntimeLocale {
        let entries = self.entries(locale);
        let characters = (0..locale.entries_len)
            .flat_map(|index| self.text(entries, index).chars())
            .filter(|character| !character.is_ascii())
            .collect::<BTreeSet<_>>();
        let glyphs = assign_virtual_glyphs(locale.id, characters);

        let mut record_offsets = Vec::with_capacity(locale.entries_len);
        let mut records = Vec::new();
        for index in 0..locale.entries_len {
            record_offsets
                .push(u32::try_from(records.len()).expect("locale text must fit in 4 GiB"));
            encode_record(self.text(entries, index), &glyphs, &mut records);
        }

        let mut reverse_glyphs = glyphs
            .into_iter()
            .map(|(character, code)| (code.value(), character))
            .collect::<Vec<_>>();
        reverse_glyphs.sort_unstable_by_key(|(code, _)| *code);

        RuntimeLocale {
            entries,
            record_offsets: record_offsets.into_boxed_slice(),
            records: records.into_boxed_slice(),
            glyphs: reverse_glyphs.into_boxed_slice(),
        }
    }

    fn entries(&self, locale: &CompiledLocale) -> &'static [u8] {
        let start = locale.entries_offset;
        let end = start + locale.entries_len * TRANSLATION_ENTRY_SIZE;
        &self.bytes[start..end]
    }

    fn text(&self, entries: &'static [u8], index: usize) -> &'static str {
        let entry = entry(entries, index);
        let record_offset = read_u32(entry, 12) as usize;
        let record_len = read_u32(entry, 16) as usize;
        let record = &self.bytes[record_offset..record_offset + record_len];
        // SAFETY: build.rs writes these bytes directly from Rust Strings.
        unsafe { std::str::from_utf8_unchecked(record) }
    }
}

pub(super) struct RuntimeLocale {
    entries: &'static [u8],
    record_offsets: Box<[u32]>,
    records: Box<[u8]>,
    glyphs: Box<[(u16, char)]>,
}

impl RuntimeLocale {
    pub(super) fn translation(&self, key: TranslationKey) -> Option<&[u8]> {
        let key = BinaryTranslationKey::from(key);
        let mut start = 0;
        let mut end = self.record_offsets.len();
        while start < end {
            let index = start + (end - start) / 2;
            match self.key(index).cmp(&key) {
                Ordering::Less => start = index + 1,
                Ordering::Greater => end = index,
                Ordering::Equal => return Some(self.record(index)),
            }
        }
        None
    }

    pub(super) fn stage_translations(&self) -> impl Iterator<Item = RuntimeStageTranslation<'_>> {
        (0..self.record_offsets.len()).map_while(move |index| {
            let key = self.key(index);
            (key.kind == 0).then(|| RuntimeStageTranslation {
                stage: key.primary as u16,
                section: (key.secondary >> 16) as u16,
                record: key.secondary as u16,
                replacement: self.record(index),
            })
        })
    }

    pub(super) fn virtual_character(&self, code: u16) -> Option<char> {
        self.glyphs
            .binary_search_by_key(&code, |(entry_code, _)| *entry_code)
            .ok()
            .map(|index| self.glyphs[index].1)
    }

    fn record(&self, index: usize) -> &[u8] {
        let start = self.record_offsets[index] as usize;
        let end = self
            .record_offsets
            .get(index + 1)
            .map_or(self.records.len(), |offset| *offset as usize);
        &self.records[start..end]
    }

    fn key(&self, index: usize) -> BinaryTranslationKey {
        let entry = entry(self.entries, index);
        BinaryTranslationKey {
            kind: entry[0],
            part: read_u16(entry, 2),
            primary: read_u32(entry, 4),
            secondary: read_u32(entry, 8),
        }
    }
}

#[derive(Clone, Copy)]
enum GlyphWidth {
    Narrow,
    Wide,
}

#[derive(Clone, Copy)]
enum VirtualGlyphCode {
    Narrow(u8),
    Wide(u16),
}

impl VirtualGlyphCode {
    fn value(self) -> u16 {
        match self {
            Self::Narrow(code) => u16::from(code),
            Self::Wide(code) => code,
        }
    }
}

fn assign_virtual_glyphs(
    locale: &str,
    characters: BTreeSet<char>,
) -> BTreeMap<char, VirtualGlyphCode> {
    let mut narrow_characters = Vec::new();
    let mut wide_characters = Vec::new();
    for character in characters {
        match glyph_width(character) {
            GlyphWidth::Narrow => narrow_characters.push(character),
            GlyphWidth::Wide => wide_characters.push(character),
        }
    }

    let narrow_codes = narrow_virtual_glyph_codes();
    assert!(
        narrow_characters.len() <= narrow_codes.len(),
        "locale {locale:?} uses {} half-width non-ASCII characters, but only {} single-byte virtual glyph codes are available",
        narrow_characters.len(),
        narrow_codes.len()
    );
    let wide_codes = wide_virtual_glyph_codes();
    assert!(
        wide_characters.len() <= wide_codes.len(),
        "locale {locale:?} uses {} full-width non-ASCII characters, but only {} double-byte virtual glyph codes are available",
        wide_characters.len(),
        wide_codes.len()
    );

    narrow_characters
        .into_iter()
        .zip(narrow_codes)
        .map(|(character, code)| (character, VirtualGlyphCode::Narrow(code)))
        .chain(
            wide_characters
                .into_iter()
                .zip(wide_codes)
                .map(|(character, code)| (character, VirtualGlyphCode::Wide(code))),
        )
        .collect()
}

/// Classify a character as half-width or full-width using the Unicode East Asian
/// Width property, treating Ambiguous characters (including Private Use Area
/// icons) as full-width per UAX #11 CJK context rules.
fn glyph_width(character: char) -> GlyphWidth {
    match UnicodeWidthChar::width_cjk(character) {
        Some(2) => GlyphWidth::Wide,
        _ => GlyphWidth::Narrow,
    }
}

fn narrow_virtual_glyph_codes() -> Vec<u8> {
    (0xA0..=0xDF).collect()
}

fn wide_virtual_glyph_codes() -> Vec<u16> {
    let mut codes = Vec::new();
    // The game's inline tokenizer treats 0x81-0x9F and 0xE0-0xFC as lead
    // bytes and reads two bytes for those. 0x80, 0xFD-0xFE are single-byte.
    // Invalid trail bytes avoid collisions with real characters.
    for lead in (0x81u16..=0x9F).chain(0xE0..=0xFC) {
        for trail in 1u16..=0xFF {
            let invalid_trail = trail <= 0x3F || trail == 0x7F || trail == 0xFF;
            if invalid_trail && trail != u16::from(b'%') {
                codes.push((lead << 8) | trail);
            }
        }
    }
    codes
}

fn encode_record(text: &str, glyphs: &BTreeMap<char, VirtualGlyphCode>, record: &mut Vec<u8>) {
    for character in text.chars() {
        match character {
            character if character.is_ascii() => record.push(character as u8),
            _ => match glyphs
                .get(&character)
                .expect("every non-ASCII character has a virtual glyph")
            {
                VirtualGlyphCode::Narrow(code) => record.push(*code),
                VirtualGlyphCode::Wide(code) => record.extend_from_slice(&code.to_be_bytes()),
            },
        }
    }
    record.push(0);
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes([bytes[offset], bytes[offset + 1]])
}

fn entry(entries: &[u8], index: usize) -> &[u8] {
    let offset = index * TRANSLATION_ENTRY_SIZE;
    &entries[offset..offset + TRANSLATION_ENTRY_SIZE]
}

fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes([
        bytes[offset],
        bytes[offset + 1],
        bytes[offset + 2],
        bytes[offset + 3],
    ])
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct BinaryTranslationKey {
    kind: u8,
    primary: u32,
    secondary: u32,
    part: u16,
}

impl From<TranslationKey> for BinaryTranslationKey {
    fn from(key: TranslationKey) -> Self {
        Self {
            kind: 1,
            primary: key.translation_group,
            secondary: key.record_id,
            part: key.part,
        }
    }
}

pub(super) struct RuntimeStageTranslation<'a> {
    pub(super) stage: u16,
    pub(super) section: u16,
    pub(super) record: u16,
    pub(super) replacement: &'a [u8],
}

#[cfg(test)]
mod tests {
    use super::{CompiledDictionary, CompiledLocale, GlyphWidth, glyph_width};
    use crate::localization::TranslationKey;

    static TEST_BYTES: &[u8] = &[
        0, 0, 0, 0, 125, 0, 0, 0, 42, 0, 23, 0, 40, 0, 0, 0, 8, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1,
        0, 0, 0, 48, 0, 0, 0, 13, 0, 0, 0, b's', b't', b'a', b'g', b'e', 0xEF, 0x83, 0xA7, b't',
        b'r', b'a', b'n', b's', b'l', b'a', b't', b'e', b'd', 0xE4, 0xB8, 0xAD,
    ];
    static TEST_LOCALES: [CompiledLocale; 1] = [CompiledLocale::new("test", 0, 2)];
    static TEST_DICTIONARY: CompiledDictionary = CompiledDictionary::new(TEST_BYTES, &TEST_LOCALES);

    fn resource_key(record_id: u32) -> TranslationKey {
        TranslationKey {
            resource_id: "mhfdat",
            group_id: "table",
            translation_group: 7,
            record_id,
            part: 0,
        }
    }

    #[test]
    fn encodes_the_selected_locale() {
        let source = TEST_DICTIONARY.locale("TEST").unwrap();
        let locale = TEST_DICTIONARY.encode_locale(source);

        assert!(locale.translation(resource_key(0)).is_none());
        assert_eq!(
            locale.translation(resource_key(1)),
            Some(b"translated\x81\x01\0".as_slice())
        );

        let stage = locale.stage_translations().next().unwrap();
        assert_eq!(
            (stage.stage, stage.section, stage.record),
            (125, 0x17, 0x2A)
        );
        assert_eq!(stage.replacement, b"stage\x81\x02\0");

        assert_eq!(locale.virtual_character(0x8101), Some('中'));
        assert_eq!(locale.virtual_character(0x8102), Some('\u{F0E7}'));
        assert_eq!(locale.virtual_character(0x8103), None);
    }

    #[test]
    fn classifies_cjk_and_pua_as_wide() {
        // CJK ideographs are unambiguously wide.
        assert!(matches!(glyph_width('中'), GlyphWidth::Wide));
        assert!(matches!(glyph_width('あ'), GlyphWidth::Wide));
        // Private Use Area icons (Nerd Font, Font Awesome) are Ambiguous in
        // UAX #11 and must be treated as wide in CJK contexts.
        assert!(matches!(glyph_width('\u{F0E7}'), GlyphWidth::Wide));
        assert!(matches!(glyph_width('\u{F118}'), GlyphWidth::Wide));
        assert!(matches!(glyph_width('\u{E0B0}'), GlyphWidth::Wide));
        // Latin and half-width katakana stay narrow.
        assert!(matches!(glyph_width('M'), GlyphWidth::Narrow));
        assert!(matches!(glyph_width('ｵ'), GlyphWidth::Narrow));
        // Control characters have no width and fall back to narrow.
        assert!(matches!(glyph_width('\0'), GlyphWidth::Narrow));
    }
}
