use binrw::binread;
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};

/// A request to delete a character using an issued Sign session.
#[binread]
pub(super) struct DeleteCharacter {
    pub(super) session_token: [u8; SIGN_SESSION_TOKEN_LEN],
    #[br(temp, magic = 0_u8)]
    _session_token_terminator: (),

    #[br(map = |id: u32| CharacterId::from(id))]
    pub(super) character_id: CharacterId,

    #[br(map = |id: u32| SignSessionId::from(id))]
    pub(super) session_id: SignSessionId,
}

#[cfg(test)]
mod tests {
    use binrw::{BinReaderExt, io::Cursor};

    use super::*;

    fn request_body(token: &[u8]) -> Vec<u8> {
        let mut input = token.to_vec();
        input.push(0);
        input.extend_from_slice(&42_u32.to_be_bytes());
        input.extend_from_slice(&7_u32.to_be_bytes());
        input
    }

    #[test]
    fn parses_session_character_and_id() {
        let input = request_body(b"0123456789ABCDEF");

        let request: DeleteCharacter = Cursor::new(input).read_be().unwrap();

        assert_eq!(request.session_token, *b"0123456789ABCDEF");
        assert_eq!(u32::from(request.character_id), 42);
        assert_eq!(u32::from(request.session_id), 7);
    }

    #[test]
    fn rejects_a_session_token_with_the_wrong_length() {
        assert!(
            Cursor::new(request_body(b"short"))
                .read_be::<DeleteCharacter>()
                .is_err()
        );
    }
}
