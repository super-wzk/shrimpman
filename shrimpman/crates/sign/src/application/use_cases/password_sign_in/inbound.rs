use binrw::{NullString, binread};

/// A username-and-password Sign packet.
#[binread]
pub(super) struct PasswordSignIn {
    #[br(try_map = |value: NullString| String::from_utf8(value.0))]
    pub(super) username: String,

    #[br(try_map = |value: NullString| String::from_utf8(value.0))]
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
    fn decodes_utf8_credentials_without_changing_the_text() {
        let username = "猎人🦐";
        let password = "密码🔑";
        let input = format!("{username}\0{password}\0\0");

        let packet = parse(input.as_bytes()).unwrap();

        assert_eq!(packet.username, username);
        assert_eq!(packet.password, password);
    }

    #[test]
    fn rejects_invalid_utf8_in_either_credential() {
        for input in [
            b"\x83\x65\x83\x58\x83\x67\0pass\0\0".as_slice(),
            b"user\0\xED\xA0\x80\0\0",
            b"user\0\xF0\x9F\xA6\0\0",
        ] {
            assert!(parse(input).is_err(), "accepted invalid UTF-8: {input:?}");
        }
    }
}
