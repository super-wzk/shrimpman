use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// Decode one scalar without requiring the bytes after it to be valid UTF-8.
pub(crate) fn decode_first(bytes: &[u8]) -> Option<(char, usize)> {
    let length = match *bytes.first()? {
        0x00..=0x7f => 1,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return None,
    };
    let text = std::str::from_utf8(bytes.get(..length)?).ok()?;
    Some((text.chars().next()?, length))
}

pub(crate) fn floor_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.min(text.len());
    while !text.is_char_boundary(offset) {
        offset -= 1;
    }
    offset
}

pub(crate) fn prev_boundary(text: &str, offset: usize) -> usize {
    floor_boundary(text, offset.min(text.len()).saturating_sub(1))
}

pub(crate) fn next_boundary(text: &str, offset: usize) -> usize {
    let mut offset = offset.saturating_add(1).min(text.len());
    while !text.is_char_boundary(offset) {
        offset += 1;
    }
    offset
}

pub(crate) fn byte_offset(text: &str, chars: usize) -> usize {
    text.char_indices()
        .nth(chars)
        .map_or(text.len(), |(offset, _)| offset)
}

pub(crate) fn truncate(text: &str, limit: usize) -> &str {
    &text[..floor_boundary(text, limit)]
}

pub(crate) fn char_columns(character: char) -> usize {
    character.width_cjk().unwrap_or(0)
}

pub(crate) fn display_columns(text: &str) -> usize {
    text.width_cjk()
}

/// Insert into a NUL-terminated native field. The native limit excludes its NUL.
/// Both the retained suffix and the inserted prefix remain complete UTF-8.
pub(crate) fn insert(buffer: &mut [u8], cursor: usize, limit: usize, text: &str) -> Option<usize> {
    let length = buffer.iter().position(|byte| *byte == 0)?;
    if limit >= buffer.len() || length > limit || text.contains('\0') {
        return None;
    }
    let current = std::str::from_utf8(&buffer[..length]).ok()?;
    if !current.is_char_boundary(cursor) {
        return None;
    }
    let inserted = truncate(text, limit - length).as_bytes();
    buffer.copy_within(cursor..=length, cursor + inserted.len());
    buffer[cursor..cursor + inserted.len()].copy_from_slice(inserted);
    Some(inserted.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_scalar_rejects_invalid_and_incomplete_sequences() {
        for text in ["a", "é", "中", "𠮷", "😀", "\0"] {
            assert_eq!(
                decode_first(text.as_bytes()),
                Some((text.chars().next().unwrap(), text.len()))
            );
        }
        for bytes in [
            &b""[..],
            &[0x80],
            &[0xc0, 0x80],
            &[0xed, 0xa0, 0x80],
            &[0xf4, 0x90, 0x80, 0x80],
            &[0xe4, 0xb8],
        ] {
            assert_eq!(decode_first(bytes), None);
        }
        assert_eq!(decode_first(&[b'a', 0xff]), Some(('a', 1)));
    }

    #[test]
    fn navigation_and_truncation_never_split_a_scalar() {
        let text = "A中😀e\u{301}";
        let boundaries = [0, 1, 4, 8, 9, 11];
        for pair in boundaries.windows(2) {
            assert_eq!(next_boundary(text, pair[0]), pair[1]);
            assert_eq!(prev_boundary(text, pair[1]), pair[0]);
        }
        for offset in 0..=text.len() + 2 {
            assert!(text.is_char_boundary(floor_boundary(text, offset)));
            assert!(text.is_char_boundary(next_boundary(text, offset)));
            assert!(text.is_char_boundary(prev_boundary(text, offset)));
            assert!(truncate(text, offset).len() <= offset);
        }
        assert_eq!(truncate(text, 7), "A中");
        assert_eq!(byte_offset(text, 3), 8);
        assert_eq!(byte_offset(text, usize::MAX), text.len());
    }

    #[test]
    fn display_width_accounts_for_wide_and_zero_width_characters() {
        assert_eq!(display_columns("A中😀e\u{301}"), 6);
        assert_eq!(char_columns('\u{301}'), 0);
        assert_eq!(char_columns('中'), 2);
        assert_eq!(char_columns('😀'), 2);
    }

    #[test]
    fn insertion_preserves_the_suffix_and_capacity_at_utf8_boundaries() {
        let mut buffer = [0; 16];
        buffer[..4].copy_from_slice("A中".as_bytes());
        assert_eq!(insert(&mut buffer, 1, 11, "😀é𠮷"), Some(6));
        assert_eq!(&buffer[..11], "A😀é中\0".as_bytes());
        assert_eq!(insert(&mut buffer, 2, 15, "x"), None);
        assert_eq!(insert(&mut buffer, 10, 16, "x"), None);
        assert_eq!(insert(&mut buffer, 10, 9, "x"), None);
        assert_eq!(insert(&mut buffer, 10, 10, "😀"), Some(0));
        assert_eq!(insert(&mut buffer, 10, 15, "a\0b"), None);
        assert_eq!(insert(&mut [b'x'; 16], 0, 15, "x"), None);
        assert_eq!(insert(&mut [0xff, 0], 0, 1, "x"), None);
        assert_eq!(insert(&mut [], 0, 0, "x"), None);
    }
}
