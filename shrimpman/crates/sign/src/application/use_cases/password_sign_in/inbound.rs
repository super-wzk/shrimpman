use binrw::{NullString, binread};
use shrimpman_common::encoding::decode_shift_jis;

/// A username-and-password Sign packet.
#[binread]
pub(super) struct PasswordSignIn {
    #[br(try_map = |value: NullString| decode_shift_jis(&value.0))]
    pub(super) username: String,

    #[br(try_map = |value: NullString| decode_shift_jis(&value.0))]
    pub(super) password: String,

    #[br(temp)]
    _extra: NullString,
}

#[cfg(test)]
mod tests {
    use binrw::{BinReaderExt, BinResult, io::Cursor};

    use super::*;

    fn parse(input: &[u8]) -> BinResult<PasswordSignIn> {
        Cursor::new(input).read_be()
    }

    #[test]
    fn parses_credential_body() {
        let packet = parse(b"user\0pass\0\0").unwrap();

        assert_eq!(packet.username, "user");
        assert_eq!(packet.password, "pass");
    }

    #[test]
    fn decodes_shift_jis_credentials() {
        let mut input = vec![0x83, 0x65, 0x83, 0x58, 0x83, 0x67, 0];
        input.extend_from_slice(b"pass\0\0");

        assert_eq!(parse(&input).unwrap().username, "テスト");
    }
}
