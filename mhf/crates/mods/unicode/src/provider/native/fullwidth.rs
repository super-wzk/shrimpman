//! The native fullwidth helper has no size argument. These capacities come from
//! its 547 call sites in the supported client, including split stack variables
//! and the three callers that forward a 256-byte destination through 1088E700.

use std::{borrow::Cow, ptr};

use crate::provider::utf8::truncate;

mod callers;

pub(super) unsafe fn enabled(base: usize) -> bool {
    let language = unsafe { ptr::read_volatile((base + 0x0E866C5C) as *const u32) } as usize;
    language != 0 && unsafe { ptr::read_volatile((language + 7600) as *const u32) } != 1
}

pub(super) unsafe fn validate(base: usize, size: usize) -> Result<(), String> {
    for &(call, _) in callers::CALLERS {
        let call = call as usize;
        if call + 5 > size
            || unsafe { ptr::read((base + call) as *const u8) } != 0xE8
            || unsafe { ptr::read_unaligned((base + call + 1) as *const i32) }
                != (0x014D_F610i32 - call as i32 - 5)
        {
            return Err(format!("unsupported fullwidth caller at RVA {call:#010X}"));
        }
    }
    Ok(())
}

pub(super) fn capacity(return_rva: usize) -> Option<usize> {
    let call = u32::try_from(return_rva.checked_sub(5)?).ok()?;
    callers::CALLERS
        .binary_search_by_key(&call, |&(rva, _)| rva)
        .ok()
        .map(|index| callers::CALLERS[index].1 as usize)
}

/// Matches the Japanese PAC's ASCII presentation table, including its yen sign.
pub(super) fn character(value: char) -> char {
    match value {
        '\u{0001}'..=' ' => '\u{3000}',
        '\\' => '￥',
        '!'..='~' => char::from_u32(value as u32 + 0xFEE0).unwrap_or(value),
        '\u{007F}' => '・',
        _ => value,
    }
}

pub(super) fn convert(source: &str, capacity: usize, enabled: bool) -> Cow<'_, str> {
    let available = capacity.saturating_sub(1);
    if enabled {
        let mut expanded = String::with_capacity(source.len().min(available));
        for character in source.chars().map(character) {
            if character.len_utf8() > available - expanded.len() {
                // Preserve the complete narrow value when its fullwidth form
                // cannot fit in a small native field.
                return Cow::Borrowed(truncate(source, available));
            }
            expanded.push(character);
        }
        return Cow::Owned(expanded);
    }
    Cow::Borrowed(truncate(source, available))
}

#[cfg(test)]
mod tests {
    use super::{capacity, convert};

    #[test]
    fn conversion_preserves_original_padding_and_unicode() {
        assert_eq!(convert(" 12\\中🙂", 64, true), "　１２￥中🙂");
        assert_eq!(convert(" 12中", 64, false), " 12中");
        assert_eq!(convert("1234", 12, true), "1234");
        assert_eq!(convert("中🙂文", 8, false), "中🙂");
        assert_eq!(convert("中", 0, true), "");
    }

    #[test]
    fn caller_inventory_is_unique_and_covers_split_stack_buffers() {
        assert_eq!(super::callers::CALLERS.len(), 547);
        assert!(
            super::callers::CALLERS
                .windows(2)
                .all(|pair| pair[0].0 < pair[1].0)
        );
        assert_eq!(capacity(0x0081_12BF + 5), Some(16));
        assert_eq!(capacity(0x0093_CB50 + 5), Some(32));
        assert_eq!(capacity(0x0088_E8F5 + 5), Some(256));
        assert_eq!(capacity(0), None);
    }
}
