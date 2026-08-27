use binrw::binread;
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};

use crate::{envelope::LandPacket, response::RequestHandle};

const MSG_SYS_LOGIN: u16 = 0x0014;

/// Authenticates a selected character on a Land connection.
#[binread]
pub(super) struct LoginRequest {
    pub(super) request_handle: RequestHandle,
    #[br(map = |id: u32| CharacterId::from(id))]
    pub(super) character_id: CharacterId,
    #[br(map = |id: u32| SignSessionId::from(id))]
    pub(super) session_id: SignSessionId,
    #[br(temp)]
    _reserved_0: u16,
    pub(super) request_version: u16,
    #[br(temp)]
    _repeated_character_id: u32,
    #[br(temp)]
    _reserved_1: u16,
    #[br(temp)]
    _session_token_length: u16,
    pub(super) session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    #[br(temp, magic = 0_u8)]
    _session_token_terminator: (),
}

impl LandPacket for LoginRequest {
    const OPCODE: u16 = MSG_SYS_LOGIN;
}

#[cfg(test)]
mod tests {
    use binrw::BinReaderExt;
    use std::io::Cursor;

    use super::*;

    fn login_body(token: &[u8]) -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(&7_u32.to_be_bytes());
        input.extend_from_slice(&42_u32.to_be_bytes());
        input.extend_from_slice(&9_u32.to_be_bytes());
        input.extend_from_slice(&0_u16.to_be_bytes());
        input.extend_from_slice(&11_u16.to_be_bytes());
        input.extend_from_slice(&42_u32.to_be_bytes());
        input.extend_from_slice(&0_u16.to_be_bytes());
        input.extend_from_slice(&(SIGN_SESSION_TOKEN_LEN as u16 + 1).to_be_bytes());
        input.extend_from_slice(token);
        input.push(0);
        input
    }

    #[test]
    fn parses_the_login_body() {
        let input = login_body(b"0123456789ABCDEF");

        let request: LoginRequest = Cursor::new(input).read_be().unwrap();

        assert_eq!(u32::from(request.request_handle), 7);
        assert_eq!(u32::from(request.character_id), 42);
        assert_eq!(u32::from(request.session_id), 9);
        assert_eq!(request.request_version, 11);
        assert_eq!(request.session_token, *b"0123456789ABCDEF");
    }

    #[test]
    fn ignores_legacy_login_fields() {
        let mut input = login_body(b"0123456789ABCDEF");
        input[12..14].copy_from_slice(&1_u16.to_be_bytes());
        input[16..20].copy_from_slice(&43_u32.to_be_bytes());
        input[20..22].copy_from_slice(&1_u16.to_be_bytes());
        input[22..24].copy_from_slice(&0_u16.to_be_bytes());

        assert!(Cursor::new(input).read_be::<LoginRequest>().is_ok());
    }

    #[test]
    fn rejects_a_session_token_with_the_wrong_length() {
        assert!(
            Cursor::new(login_body(b"short"))
                .read_be::<LoginRequest>()
                .is_err()
        );
    }
}
