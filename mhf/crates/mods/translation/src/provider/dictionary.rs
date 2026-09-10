use super::{TRANSLATION_ENTRY_SIZE, TRANSLATION_GROUPS};
use crate::{Key, KeyKind};
use std::cmp::Ordering;

pub(crate) struct CompiledLocale {
    id: &'static str,
    entries_offset: usize,
    entries_len: usize,
}

impl CompiledLocale {
    pub(crate) const fn new(id: &'static str, entries_offset: usize, entries_len: usize) -> Self {
        Self {
            id,
            entries_offset,
            entries_len,
        }
    }
}

pub(crate) struct CompiledDictionary {
    bytes: &'static [u8],
    locales: &'static [CompiledLocale],
}

impl CompiledDictionary {
    pub(crate) const fn new(bytes: &'static [u8], locales: &'static [CompiledLocale]) -> Self {
        Self { bytes, locales }
    }

    pub(crate) fn locale(&'static self, id: &str) -> Option<RuntimeLocale> {
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

    pub(crate) fn locale_ids(&self) -> impl Iterator<Item = &'static str> + '_ {
        self.locales.iter().map(|locale| locale.id)
    }
}

/// Every record is already NUL-terminated UTF-8 in the executable's static data.
pub(crate) struct RuntimeLocale {
    bytes: &'static [u8],
    entries: &'static [u8],
}

impl RuntimeLocale {
    pub(crate) fn translation(&self, key: Key<'_>) -> Option<&'static [u8]> {
        let key = BinaryTranslationKey::from_key(key)?;
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

impl BinaryTranslationKey {
    fn from_key(key: Key<'_>) -> Option<Self> {
        Some(match key.kind() {
            KeyKind::Resource => {
                let index = TRANSLATION_GROUPS
                    .binary_search_by(|(resource, group, _)| {
                        (*resource, *group).cmp(&(key.resource_id(), key.group_id()))
                    })
                    .ok()?;
                Self {
                    kind: 1,
                    primary: TRANSLATION_GROUPS[index].2,
                    secondary: key.record_id(),
                    part: key.part(),
                }
            }
            KeyKind::Stage => Self {
                kind: 0,
                primary: u32::from(key.stage_id()),
                secondary: u32::from(key.section()) << 16 | u32::from(key.record()),
                part: 0,
            },
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn generated_locales_contain_only_nul_terminated_utf8_records() {
        let dictionary = &crate::provider::TRANSLATION_DICTIONARY;
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
