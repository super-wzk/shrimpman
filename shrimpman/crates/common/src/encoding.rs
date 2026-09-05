use encoding_rs::SHIFT_JIS;
use thiserror::Error;

/// An invalid byte sequence encountered while decoding Shift-JIS.
#[derive(Debug, Error)]
#[error("invalid Shift-JIS string")]
pub struct ShiftJisDecodeError;

/// A string that cannot be represented as Shift-JIS.
#[derive(Debug, Error)]
#[error("string cannot be represented as Shift-JIS")]
pub struct ShiftJisEncodeError;

/// Strictly decodes one Shift-JIS byte string.
pub fn decode_shift_jis(bytes: &[u8]) -> Result<String, ShiftJisDecodeError> {
    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);

    if had_errors {
        Err(ShiftJisDecodeError)
    } else {
        Ok(decoded.into_owned())
    }
}

/// Strictly encodes one string as Shift-JIS.
pub fn encode_shift_jis(value: &str) -> Result<Vec<u8>, ShiftJisEncodeError> {
    let (encoded, _, had_errors) = SHIFT_JIS.encode(value);

    if had_errors {
        Err(ShiftJisEncodeError)
    } else {
        Ok(encoded.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_shift_jis, encode_shift_jis};

    #[test]
    fn decodes_valid_shift_jis() {
        assert_eq!(
            decode_shift_jis(&[0x83, 0x65, 0x83, 0x58, 0x83, 0x67]).unwrap(),
            "テスト"
        );
    }

    #[test]
    fn rejects_invalid_shift_jis() {
        assert!(decode_shift_jis(&[0x82]).is_err());
    }

    #[test]
    fn encodes_valid_shift_jis() {
        assert_eq!(
            encode_shift_jis("テスト").unwrap(),
            [0x83, 0x65, 0x83, 0x58, 0x83, 0x67]
        );
    }

    #[test]
    fn rejects_unrepresentable_shift_jis() {
        assert!(encode_shift_jis("🦐").is_err());
    }
}
