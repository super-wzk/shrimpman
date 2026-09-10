//! Decode a declared legacy code page once at resource ingress.

#[cfg(windows)]
pub fn decode_source(source: &[u8], code_page: u32) -> Result<String, String> {
    use windows::Win32::Globalization::{MB_ERR_INVALID_CHARS, MultiByteToWideChar};
    if source.is_empty() {
        return Ok(String::new());
    }
    let length = unsafe { MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, source, None) };
    if length <= 0 {
        return Err(format!(
            "invalid resource text for Windows code page {code_page}: {}",
            windows::core::Error::from_thread()
        ));
    }
    let mut utf16 = vec![0; length as usize];
    let written =
        unsafe { MultiByteToWideChar(code_page, MB_ERR_INVALID_CHARS, source, Some(&mut utf16)) };
    if written != length {
        return Err(format!(
            "failed to decode resource text for Windows code page {code_page}: {}",
            windows::core::Error::from_thread()
        ));
    }
    String::from_utf16(&utf16).map_err(|error| format!("invalid decoded resource text: {error}"))
}

#[cfg(not(windows))]
pub fn decode_source(source: &[u8], code_page: u32) -> Result<String, String> {
    if source.is_empty() {
        return Ok(String::new());
    }
    let encoding = match code_page {
        932 => encoding_rs::SHIFT_JIS,
        936 => encoding_rs::GBK,
        949 => encoding_rs::EUC_KR,
        950 => encoding_rs::BIG5,
        65001 => encoding_rs::UTF_8,
        _ => return Err(format!("unsupported source code page {code_page}")),
    };
    encoding
        .decode_without_bom_handling_and_without_replacement(source)
        .map(std::borrow::Cow::into_owned)
        .ok_or_else(|| format!("invalid resource text for code page {code_page}"))
}

#[cfg(test)]
mod tests {
    use super::decode_source;

    #[test]
    fn startup_decoding_preserves_controls_and_rejects_incomplete_sequences() {
        assert_eq!(
            decode_source(b"~C00\x83\x65\x83\x58\x83\x67 %s\n", 932).unwrap(),
            "~C00テスト %s\n"
        );
        assert!(decode_source(&[0x81], 932).is_err());
        assert_eq!(decode_source(b"\xC7\xD1\xB1\xDB", 949).unwrap(), "한글");
        assert_eq!(decode_source(b"\xC1\x63\xC5\xE9", 950).unwrap(), "繁體");
    }
}
