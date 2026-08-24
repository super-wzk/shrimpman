use binrw::binread;
use shrimpman_common::binary::CountedVec;
use shrimpman_domain::character::CharacterId;

/// A request for Worlds and the presence of selected characters.
#[binread]
pub(super) struct ListWorldsPresence {
    #[br(map = |ids: CountedVec<u16, u32>| Vec::<u32>::from(ids)
        .into_iter()
        .map(CharacterId::from)
        .collect())]
    pub(super) character_ids: Vec<CharacterId>,
}

#[cfg(test)]
mod tests {
    use binrw::{BinReaderExt, io::Cursor};

    use super::*;

    #[test]
    fn parses_character_ids() {
        let input = [
            0x00, 0x02,
            0x00, 0x00, 0x00, 0x2a,
            0x00, 0x00, 0x00, 0x64,
        ];
        let request: ListWorldsPresence = Cursor::new(input).read_be().unwrap();

        assert_eq!(
            request
                .character_ids
                .into_iter()
                .map(u32::from)
                .collect::<Vec<_>>(),
            [42, 100]
        );
    }
}
