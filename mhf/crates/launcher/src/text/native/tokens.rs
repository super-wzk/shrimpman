use crate::text::utf8::{decode_first, display_columns};
use unicode_segmentation::UnicodeSegmentation;

/// Native markup keeps its token IDs; its length is now a UTF-8 byte count.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Token {
    pub kind: u32,
    pub bytes: usize,
    pub character: Option<char>,
    pub columns: usize,
}

impl Token {
    const fn control(kind: u32, bytes: usize) -> Self {
        Self {
            kind,
            bytes,
            character: None,
            columns: 0,
        }
    }

    pub fn argument(self, source: &[u8]) -> u32 {
        source
            .iter()
            .copied()
            .skip(2)
            .take(self.bytes.saturating_sub(2))
            .take_while(u8::is_ascii_digit)
            .fold(0u32, |value, digit| {
                value
                    .saturating_mul(10)
                    .saturating_add(u32::from(digit - b'0'))
            })
    }
}

pub(super) fn parse(source: &[u8], literal: bool) -> Token {
    let Some(&first) = source.first() else {
        return Token::control(0, 0);
    };
    if first == 0 {
        return Token::control(0, 0);
    }
    if source.starts_with(b"\r\n") {
        return Token::control(4, 2);
    }
    if first == b'\n' {
        return Token::control(4, 1);
    }
    if source.starts_with(b"~%") {
        return Token::control(11, 2);
    }
    if !literal {
        if first == b'~' {
            let kind = match source.get(1) {
                Some(b'~') => return Token::control(5, 2),
                Some(b'A') => 6,
                Some(b'B') => 7,
                Some(b'C') => 8,
                Some(b'K') => 9,
                Some(b'S') => 10,
                _ => 0,
            };
            if kind != 0 {
                let max = if kind == 9 { 3 } else { 2 };
                let digits = source
                    .iter()
                    .skip(2)
                    .take(max)
                    .take_while(|byte| byte.is_ascii_digit())
                    .count();
                return Token::control(kind, 2 + digits);
            }
        } else if first == b'{' {
            match source.get(1) {
                Some(b'{') if source.get(2) == Some(&b'}') => return Token::control(12, 3),
                Some(b'}') => {
                    return Token::control(
                        if source.get(2) == Some(&b'}') { 13 } else { 1 },
                        if source.get(2) == Some(&b'}') { 3 } else { 2 },
                    );
                }
                Some(letter @ (b'I' | b'i' | b'K' | b'k' | b'U' | b'u')) => {
                    let digits = source
                        .iter()
                        .skip(2)
                        .take_while(|byte| byte.is_ascii_digit())
                        .count();
                    if source.get(2 + digits) == Some(&b'}') {
                        let kind = match letter.to_ascii_uppercase() {
                            b'I' => 15,
                            b'K' => 14,
                            _ => 16,
                        };
                        return Token::control(kind, 3 + digits);
                    }
                }
                _ => {}
            }
        }
    }
    // Invalid input always makes progress. The validators reject it at entry;
    // this replacement also prevents a corrupt resource from stalling a loop.
    let valid = match std::str::from_utf8(source) {
        Ok(text) => text,
        Err(error) => std::str::from_utf8(&source[..error.valid_up_to()]).unwrap_or_default(),
    };
    let grapheme = valid.graphemes(true).next();
    let (character, scalar_bytes) = decode_first(source).unwrap_or(('\u{FFFD}', 1));
    let bytes = grapheme.map_or(scalar_bytes, str::len);
    let columns = grapheme.map_or(1, display_columns);
    Token {
        kind: match columns {
            0 => 1,
            1 => 2,
            _ => 3,
        },
        bytes,
        character: Some(character),
        columns,
    }
}

#[cfg(test)]
mod tests {
    use super::parse;

    #[test]
    fn unicode_width_is_independent_of_encoded_length() {
        for (text, kind, bytes) in [("ش", 2, 2), ("ｶ", 2, 3), ("中", 3, 3), ("😀", 3, 4)] {
            let token = parse(text.as_bytes(), false);
            assert_eq!((token.kind, token.bytes), (kind, bytes));
        }
    }

    #[test]
    fn preserves_markup_and_literal_mode() {
        for (text, kind, count) in [
            ("~C83中", 8, 4),
            ("~K1234", 9, 5),
            ("{U12}中", 16, 5),
            ("{{}", 12, 3),
            ("{}}", 13, 3),
        ] {
            let token = parse(text.as_bytes(), false);
            assert_eq!((token.kind, token.bytes), (kind, count));
        }
        assert_eq!(parse(b"~C83", true).bytes, 1);
        assert_eq!(parse(b"~%", true).kind, 11);
        assert_eq!(parse(b"{U12", false).bytes, 1);
    }

    #[test]
    fn truncated_and_invalid_sequences_do_not_skip_following_ascii() {
        assert_eq!(parse(&[0xE4, b'~', b'C'], false).bytes, 1);
        assert_eq!(parse(&[0xF0, 0x9F], false).bytes, 1);
        assert_eq!(parse(&[], false).kind, 0);
    }

    #[test]
    fn keeps_combining_and_joined_emoji_in_one_display_token() {
        for (text, columns) in [("e\u{301}", 1), ("👩‍💻", 2), ("🇨🇳", 2)] {
            let token = parse(text.as_bytes(), false);
            assert_eq!((token.bytes, token.columns), (text.len(), columns));
        }
    }
}
