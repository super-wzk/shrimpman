use super::tokens;
use crate::provider::utf8::{decode_first, truncate};

pub(super) fn escape(source: &[u8], capacity: usize, percent: bool) -> Vec<u8> {
    let mut result = Vec::with_capacity(source.len().min(capacity));
    let mut offset = 0;
    while offset < source.len() {
        let (character, count) = decode_first(&source[offset..]).unwrap_or(('\u{FFFD}', 1));
        let mut utf8 = [0; 4];
        let encoded = character.encode_utf8(&mut utf8).as_bytes();
        let replacement: &[u8] = match (character, percent) {
            ('%', true) => b"%%",
            ('{', false) => b"{{}",
            ('}', false) => b"{}}",
            ('~', false) => b"~~",
            _ => encoded,
        };
        if result.len() + replacement.len() < capacity {
            result.extend_from_slice(replacement)
        }
        offset += count;
    }
    result
}

pub(super) fn unformat(source: &[u8], only_colors: bool) -> Vec<u8> {
    let mut result = Vec::with_capacity(source.len());
    let mut offset = 0;
    let mut literal = false;
    while offset < source.len() {
        let token = tokens::parse(&source[offset..], literal);
        if token.bytes == 0 {
            break;
        }
        if only_colors {
            if !source[offset..].starts_with(b"~C") {
                result.extend_from_slice(&source[offset..offset + token.bytes]);
            }
        } else {
            match token.kind {
                2..=4 => result.extend_from_slice(&source[offset..offset + token.bytes]),
                5 => result.push(b'~'),
                12 => result.push(b'{'),
                13 => result.push(b'}'),
                _ => {}
            }
        }
        if token.kind == 11 {
            literal = !literal
        }
        offset += token.bytes;
    }
    result
}

pub(super) fn shortened<'a, 'b>(
    source: &'a str,
    capacity: usize,
    suffix: &'b str,
) -> (&'a str, &'b str) {
    let available = capacity.saturating_sub(1);
    if source.len() <= available {
        return (source, "");
    }
    let suffix = truncate(suffix, available);
    (truncate(source, available - suffix.len()), suffix)
}

pub(super) fn allowed(character: char, flags: u8) -> bool {
    if flags & 1 != 0 && character.is_control() {
        return false;
    }
    let halfwidth_kana = ('\u{FF61}'..='\u{FF9F}').contains(&character);
    if flags & 2 != 0 && halfwidth_kana {
        return false;
    }
    if flags & 4 != 0 && !character.is_ascii() && !halfwidth_kana {
        return false;
    }
    if flags & 8 != 0 && matches!(character, ' ' | '\u{3000}') {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::{allowed, escape, shortened, unformat};

    #[test]
    fn escaping_and_unformatting_preserve_unicode() {
        let source = "ｶ中😀{~}%";
        let escaped = escape(source.as_bytes(), 100, false);
        assert_eq!(unformat(&escaped, false), source.as_bytes());
        assert_eq!(escape("中%😀".as_bytes(), 8, true), "中%%".as_bytes());
    }

    #[test]
    fn truncation_reserves_suffix_and_never_splits_a_scalar() {
        assert_eq!(shortened("中😀文", 9, "…"), ("中", "…"));
        assert_eq!(shortened("中😀文", 3, "…"), ("", ""));
        assert_eq!(shortened("中😀", 8, "…"), ("中😀", ""));
    }

    #[test]
    fn field_flags_retain_independent_restrictions() {
        assert!(allowed('中', 1));
        assert!(!allowed('\n', 1));
        assert!(!allowed('\u{3000}', 8));
        assert!(allowed('ｶ', 4));
        assert!(!allowed('ｶ', 2));
        assert!(!allowed('中', 4));
    }
}
