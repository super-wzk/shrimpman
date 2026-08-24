use binrw::{NullString, binread};
use shrimpman_domain::{character::CharacterId, session::SignSessionId};

/// A request to delete a character using an issued Sign session.
#[binread]
pub(super) struct DeleteCharacter {
    #[br(map = |value: NullString| value.0)]
    pub(super) session_token: Vec<u8>,

    #[br(map = |id: u32| CharacterId::from(id))]
    pub(super) character_id: CharacterId,

    #[br(map = |id: u32| SignSessionId::from(id))]
    pub(super) session_id: SignSessionId,
}

#[cfg(test)]
mod tests {
    use binrw::{BinReaderExt, io::Cursor};

    use super::*;

    #[test]
    fn parses_session_character_and_id() {
        let mut input = b"0123456789ABCDEF\0".to_vec();
        input.extend_from_slice(&42_u32.to_be_bytes());
        input.extend_from_slice(&7_u32.to_be_bytes());

        let request: DeleteCharacter = Cursor::new(input).read_be().unwrap();

        assert_eq!(request.session_token, b"0123456789ABCDEF");
        assert_eq!(u32::from(request.character_id), 42);
        assert_eq!(u32::from(request.session_id), 7);
    }
}
