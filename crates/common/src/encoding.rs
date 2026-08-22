use encoding_rs::SHIFT_JIS;
use thiserror::Error;

/// An invalid byte sequence encountered while decoding Shift-JIS.
#[derive(Debug, Error)]
#[error("invalid Shift-JIS string")]
pub struct ShiftJisDecodeError;

/// Strictly decodes one Shift-JIS byte string.
pub fn decode_shift_jis(bytes: &[u8]) -> Result<String, ShiftJisDecodeError> {
    let (decoded, had_errors) = SHIFT_JIS.decode_without_bom_handling(bytes);

    if had_errors {
        Err(ShiftJisDecodeError)
    } else {
        Ok(decoded.into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::decode_shift_jis;

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
}
