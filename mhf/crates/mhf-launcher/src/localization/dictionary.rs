use super::{TRANSLATION_ENTRY_SIZE, TranslationKey};
use std::cmp::Ordering;

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

    pub(super) fn locale(&'static self, id: &str) -> Option<RuntimeLocale> {
        let locale = self
            .locales
            .iter()
            .find(|locale| locale.id.eq_ignore_ascii_case(id))?;
        let start = locale.entries_offset;
        let end = start + locale.entries_len * TRANSLATION_ENTRY_SIZE;
        Some(RuntimeLocale {
            bytes: self.bytes,
            entries: &self.bytes[start..end],
        })
    }

    pub(super) fn locale_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.locales.iter().map(|locale| locale.id)
    }
}

/// Every record is already NUL-terminated UTF-8 in the executable's static data.
pub(super) struct RuntimeLocale {
    bytes: &'static [u8],
    entries: &'static [u8],
}

impl RuntimeLocale {
    pub(super) fn translation(&self, key: TranslationKey) -> Option<&'static [u8]> {
        let key = BinaryTranslationKey::from(key);
        let mut start = 0;
        let mut end = self.entries.len() / TRANSLATION_ENTRY_SIZE;
        while start < end {
            let index = start + (end - start) / 2;
            let offset = index * TRANSLATION_ENTRY_SIZE;
            let entry = &self.entries[offset..offset + TRANSLATION_ENTRY_SIZE];
            let entry_key = BinaryTranslationKey {
                kind: entry[0],
                primary: read_u32(entry, 4),
                secondary: read_u32(entry, 8),
                part: u16::from_le_bytes([entry[2], entry[3]]),
            };
            match entry_key.cmp(&key) {
                Ordering::Less => start = index + 1,
                Ordering::Greater => end = index,
                Ordering::Equal => {
                    let offset = read_u32(entry, 12) as usize;
                    let length = read_u32(entry, 16) as usize;
                    return Some(&self.bytes[offset..offset + length]);
                }
            }
        }
        None
    }
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
        match key {
            TranslationKey::Resource {
                translation_group,
                record_id,
                part,
                ..
            } => Self {
                kind: 1,
                primary: translation_group,
                secondary: record_id,
                part,
            },
            TranslationKey::Stage {
                stage,
                section,
                record,
            } => Self {
                kind: 0,
                primary: u32::from(stage),
                secondary: u32::from(section) << 16 | u32::from(record),
                part: 0,
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{CompiledDictionary, CompiledLocale};
    use crate::localization::TranslationKey;

    static TEST_BYTES: &[u8] = &[
        0, 0, 0, 0, 125, 0, 0, 0, 42, 0, 23, 0, 40, 0, 0, 0, 9, 0, 0, 0, 1, 0, 0, 0, 7, 0, 0, 0, 1,
        0, 0, 0, 49, 0, 0, 0, 14, 0, 0, 0, b's', b't', b'a', b'g', b'e', 0xEF, 0x83, 0xA7, 0, b't',
        b'r', b'a', b'n', b's', b'l', b'a', b't', b'e', b'd', 0xE4, 0xB8, 0xAD, 0,
    ];
    static TEST_LOCALES: [CompiledLocale; 1] = [CompiledLocale::new("test", 0, 2)];
    static TEST_DICTIONARY: CompiledDictionary = CompiledDictionary::new(TEST_BYTES, &TEST_LOCALES);

    fn resource_key(record_id: u32) -> TranslationKey {
        TranslationKey::Resource {
            resource_id: "mhfdat",
            group_id: "table",
            translation_group: 7,
            record_id,
            part: 0,
        }
    }

    #[test]
    fn looks_up_nul_terminated_utf8_without_copying_the_compiled_records() {
        let locale = TEST_DICTIONARY.locale("TEST").unwrap();

        assert!(locale.translation(resource_key(0)).is_none());
        let translated = locale.translation(resource_key(1)).unwrap();
        assert_eq!(translated, "translated中\0".as_bytes());
        assert_eq!(translated.as_ptr(), TEST_BYTES[49..].as_ptr());

        let stage = locale
            .translation(TranslationKey::Stage {
                stage: 125,
                section: 0x17,
                record: 0x2A,
            })
            .unwrap();
        assert_eq!(stage, "stage\u{F0E7}\0".as_bytes());
        assert_eq!(stage.as_ptr(), TEST_BYTES[40..].as_ptr());
    }

    #[test]
    fn generated_locales_contain_only_nul_terminated_utf8_records() {
        let dictionary = &crate::localization::TRANSLATION_DICTIONARY;
        for locale in dictionary.locales {
            let runtime = dictionary.locale(locale.id).unwrap();
            for entry in runtime
                .entries
                .as_chunks::<{ super::TRANSLATION_ENTRY_SIZE }>()
                .0
            {
                let offset = super::read_u32(entry, 12) as usize;
                let length = super::read_u32(entry, 16) as usize;
                let record = &runtime.bytes[offset..offset + length];
                assert_eq!(record.last(), Some(&0), "locale {:?}", locale.id);
                assert!(
                    std::str::from_utf8(record).is_ok(),
                    "locale {:?}",
                    locale.id
                );
            }
        }
    }
}
