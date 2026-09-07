//! Preserve the native normalization groups and blocked-word dictionaries while
//! comparing Unicode scalars. Their packed legacy keys are resource metadata,
//! and are decoded here; user text never returns to the old byte encoding.

use super::{copy_z, read_z};
use crate::text::utf8::char_columns;
use std::{
    ptr,
    sync::atomic::{AtomicBool, Ordering},
};
use windows::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};

static INVALID_METADATA_REPORTED: AtomicBool = AtomicBool::new(false);

#[derive(Clone, Debug)]
struct Group {
    id: u16,
    characters: Vec<char>,
}

#[derive(Clone, Copy, Debug)]
struct PatternCharacter {
    group: Option<u16>,
    literal: char,
}

#[derive(Debug)]
struct Character {
    offset: usize,
    character: char,
}

#[derive(Debug)]
struct Normalized {
    group: Option<u16>,
    literal: char,
    members: std::ops::Range<usize>,
}

pub(super) unsafe fn apply(base: usize, destination: *mut u8, dictionary: *const u8) -> u32 {
    let Some(source) = (unsafe { read_z(destination, 512) }) else {
        return 0;
    };
    let Ok(source) = std::str::from_utf8(source) else {
        return 0;
    };
    if dictionary.is_null() {
        return 1;
    }
    let source = source.to_owned();
    let group_table = unsafe { ptr::read_volatile((base + 0x0E69FEA8) as *const u32) };
    let metadata = unsafe { read_groups(group_table as *const u32) }
        .and_then(|groups| unsafe { read_patterns(dictionary) }.map(|patterns| (groups, patterns)));
    let (groups, patterns) = match metadata {
        Ok(metadata) => metadata,
        Err(error) => {
            if !INVALID_METADATA_REPORTED.swap(true, Ordering::Relaxed) {
                eprintln!("UTF-8 word filtering rejected invalid native metadata: {error}");
            }
            // Do not admit unchecked text or run the old byte-oriented filter
            // over UTF-8. Preserve the input and report validation failure.
            return 0;
        }
    };
    let (result, valid) = filter(&source, &groups, &patterns);
    // Every replacement fits inside the original scalar, so a caller's fixed
    // UTF-8 field cannot grow while masking a match.
    unsafe { copy_z(destination, source.len() + 1, result.as_bytes()) };
    valid as u32
}

fn decode_key(code: u16) -> Result<Vec<char>, String> {
    if code == 0 {
        return Ok(Vec::new());
    }
    let bytes = code.to_le_bytes();
    let count = if code > 0xFF { 2 } else { 1 };
    let mut wide = [0u16; 2];
    let written =
        unsafe { MultiByteToWideChar(932, MB_ERR_INVALID_CHARS, &bytes[..count], Some(&mut wide)) };
    if written == 0 {
        return Err(format!("invalid packed CP932 key {code:#06X}"));
    }
    char::decode_utf16(wide[..written as usize].iter().copied())
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| format!("invalid packed CP932 key {code:#06X}"))
}

unsafe fn read_groups(table: *const u32) -> Result<Vec<Group>, String> {
    let mut groups = Vec::new();
    if table.is_null() {
        return Ok(groups);
    }
    let mut cursor = 0usize;
    while cursor < 65535 {
        let id = cursor as u16;
        let mut packed = unsafe { ptr::read_unaligned(table.add(cursor)) };
        if packed == 0 {
            return Ok(groups);
        }
        loop {
            let mut characters = decode_key(packed as u16)?;
            if packed > 0xFFFF {
                characters.extend(decode_key((packed >> 16) as u16)?)
            }
            if (1..=2).contains(&characters.len()) {
                groups.push(Group { id, characters })
            }
            cursor += 1;
            if cursor >= 65535 {
                return Err("normalization groups exceed 65535 entries".into());
            }
            packed = unsafe { ptr::read_unaligned(table.add(cursor)) };
            if packed == 0 {
                cursor += 1;
                break;
            }
        }
    }
    Err("normalization groups have no terminator".into())
}

unsafe fn read_patterns(dictionary: *const u8) -> Result<Vec<Vec<PatternCharacter>>, String> {
    let mut result = Vec::new();
    let mut cursor = 0usize;
    // Native records contain a u32 header, count u32 pattern entries, and a
    // terminating u32. Only the low header byte carries the entry count.
    while cursor < 1024 * 1024 {
        let count = unsafe { ptr::read(dictionary.add(cursor)) } as usize;
        if count == 0 {
            return Ok(result);
        }
        let mut pattern = Vec::with_capacity(count);
        for index in 0..count {
            let packed = unsafe {
                ptr::read_unaligned(dictionary.add(cursor + 4 + index * 4) as *const u32)
            };
            let group = (packed >> 16) as u16;
            let literal = if group == u16::MAX {
                decode_key(packed as u16)?.first().copied().unwrap_or('\0')
            } else {
                '\0'
            };
            pattern.push(PatternCharacter {
                group: (group != u16::MAX).then_some(group),
                literal,
            });
        }
        result.push(pattern);
        cursor += (count + 2) * 4;
    }
    Err("word dictionary has no terminator within one MiB".into())
}

fn normalize(characters: &[Character], groups: &[Group]) -> Vec<Normalized> {
    let mut normalized = Vec::new();
    let mut index = 0;
    while index < characters.len() {
        let first = characters[index].character;
        let group = groups.iter().find(|group| {
            group.characters[0] == first
                && (group.characters.len() == 1
                    || characters
                        .get(index + 1)
                        .is_some_and(|second| group.characters[1] == second.character))
        });
        let length = group.map_or(1, |group| group.characters.len());
        normalized.push(Normalized {
            group: group.map(|group| group.id),
            literal: first,
            members: index..index + length,
        });
        index += length;
    }
    normalized
}

fn filter(source: &str, groups: &[Group], patterns: &[Vec<PatternCharacter>]) -> (String, bool) {
    let characters = source
        .char_indices()
        .filter(|(_, c)| !matches!(c, ' ' | '\u{3000}'))
        .map(|(offset, character)| Character { offset, character })
        .collect::<Vec<_>>();
    let normalized = normalize(&characters, groups);
    let mut masked = vec![false; characters.len()];
    for pattern in patterns {
        if pattern.is_empty() || pattern.len() > normalized.len() {
            continue;
        }
        let mut index = 0;
        while index + pattern.len() <= normalized.len() {
            let matching = normalized[index..index + pattern.len()]
                .iter()
                .zip(pattern)
                .all(|(input, expected)| match (input.group, expected.group) {
                    (None, None) => input.literal == expected.literal,
                    (left, right) => left == right,
                });
            if matching {
                for item in &normalized[index..index + pattern.len()] {
                    masked[item.members.clone()].fill(true)
                }
                index += pattern.len();
            } else {
                index += 1
            }
        }
    }
    let valid = !masked.iter().any(|&masked| masked);
    let mut result = String::with_capacity(source.len());
    let mut cursor = 0;
    for (character, masked) in characters.iter().zip(masked) {
        if !masked {
            continue;
        }
        result.push_str(&source[cursor..character.offset]);
        let replacement =
            if character.character.len_utf8() >= 3 && char_columns(character.character) > 1 {
                '＊'
            } else {
                '*'
            };
        result.push(replacement);
        cursor = character.offset + character.character.len_utf8();
    }
    result.push_str(&source[cursor..]);
    (result, valid)
}

#[cfg(test)]
mod tests {
    use super::{Group, PatternCharacter, decode_key, filter};

    #[test]
    fn keeps_normalization_groups_space_skipping_and_masking() {
        assert_eq!(decode_key(0).unwrap(), Vec::<char>::new());
        assert!(decode_key(0x81).is_err());
        let groups = vec![
            Group {
                id: 0,
                characters: decode_key(0x4A83).unwrap(),
            },
            Group {
                id: 0,
                characters: decode_key(0xB6).unwrap(),
            },
            Group {
                id: 3,
                characters: decode_key(0x4B83).unwrap(),
            },
            Group {
                id: 3,
                characters: decode_key(0xDEB6).unwrap(),
            },
        ];
        let pattern = vec![vec![
            PatternCharacter {
                group: Some(0),
                literal: '\0',
            },
            PatternCharacter {
                group: None,
                literal: '中',
            },
        ]];
        assert_eq!(
            filter("カ 中😀", &groups, &pattern),
            ("＊ ＊😀".into(), false)
        );
        assert_eq!(filter("ｶ中", &groups, &pattern), ("*＊".into(), false));
        assert_eq!(filter("ガ中", &groups, &pattern), ("ガ中".into(), true));
    }

    #[test]
    fn supplementary_scalars_are_preserved_and_masks_do_not_grow() {
        let pattern = vec![vec![PatternCharacter {
            group: None,
            literal: '😀',
        }]];
        let (masked, valid) = filter("𠮷😀é", &[], &pattern);
        assert!(!valid);
        assert_eq!(masked, "𠮷＊é");
        assert!(masked.len() <= "𠮷😀é".len());
    }
}
