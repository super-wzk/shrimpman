use binrw::{
    BinRead, BinResult, binread,
    io::SeekFrom,
};
use shrimpman_common::binary::CountedVec;
use shrimpman_domain::character::CharacterId;

/// A request for Worlds and the presence of selected characters.
#[binread]
pub(super) struct ListWorldsPresence {
    #[br(parse_with = parse_character_ids)]
    pub(super) character_ids: Option<Vec<CharacterId>>,
}

#[binrw::parser(reader, endian)]
fn parse_character_ids() -> BinResult<Option<Vec<CharacterId>>> {
    let position = reader.stream_position()?;
    let end = reader.seek(SeekFrom::End(0))?;
    reader.seek(SeekFrom::Start(position))?;

    if position == end {
        return Ok(None);
    }

    let ids = CountedVec::<u16, u32>::read_options(reader, endian, ())?;
    Ok(Some(
        Vec::<u32>::from(ids)
            .into_iter()
            .map(CharacterId::from)
            .collect(),
    ))
}

#[cfg(test)]
mod tests {
    use binrw::{BinReaderExt, io::Cursor};

    use super::*;

    #[test]
    fn accepts_an_absent_body() {
        let request: ListWorldsPresence = Cursor::new([]).read_be().unwrap();

        assert!(request.character_ids.is_none());
    }

    #[test]
    fn distinguishes_an_empty_character_list_from_an_absent_body() {
        let request: ListWorldsPresence = Cursor::new([0, 0]).read_be().unwrap();

        assert_eq!(request.character_ids.unwrap(), []);
    }

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
                .unwrap()
                .into_iter()
                .map(u32::from)
                .collect::<Vec<_>>(),
            [42, 100]
        );
    }

    #[test]
    fn rejects_a_truncated_body() {
        for input in [vec![0], vec![0, 1, 0, 0, 0]] {
            assert!(Cursor::new(input)
                .read_be::<ListWorldsPresence>()
                .is_err());
        }
    }
}
