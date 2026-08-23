use binrw::{BinWrite, NullString, binread};
use shrimpman_common::encoding::decode_shift_jis;
use shrimpman_protocol::Handler;

use crate::{
    InternalError, SignSessionContext,
    router::{SignPacketRegistration, VersionSelector},
};

/// A username-and-password Sign packet.
#[binread]
#[cfg_attr(
    not(test),
    expect(dead_code, reason = "the password sign-in handler is not implemented yet")
)]
struct PasswordSignIn {
    #[br(try_map = |value: NullString| decode_shift_jis(&value.0))]
    username: String,

    #[br(try_map = |value: NullString| decode_shift_jis(&value.0))]
    password: String,

    #[br(temp)]
    _extra: NullString,
}

#[derive(BinWrite)]
struct PasswordSignInResponse;

struct PasswordSignInHandler;

#[async_trait::async_trait]
impl Handler<SignSessionContext> for PasswordSignInHandler {
    type Inbound = PasswordSignIn;
    type Outbound = PasswordSignInResponse;
    type Error = InternalError;

    async fn handle(
        &self,
        _context: SignSessionContext,
        _inbound: Self::Inbound,
    ) -> Result<Vec<Self::Outbound>, Self::Error> {
        unimplemented!()
    }
}

inventory::submit! {
    SignPacketRegistration::new(
        &["SIGN:", "DSGN:", "DLTSKEYSIGN:"],
        VersionSelector::Any,
        &PasswordSignInHandler,
    )
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
