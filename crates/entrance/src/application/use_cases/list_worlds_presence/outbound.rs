use binrw::{BinWrite, binwrite};

use super::super::list_worlds::outbound::WorldList;
use crate::{MhfBin8, entrance_list::EntranceList};

const INDEX_MARKER: u16 = 0x10;
const LOCATION_BASE: u16 = 0x1000;

/// The available Worlds followed by the selected characters' current Worlds.
#[derive(BinWrite)]
pub(super) struct ListWorldsPresenceResponse {
    pub(super) worlds: MhfBin8<WorldList>,
    pub(super) presences: MhfBin8<CharacterPresenceList>,
}

#[binwrite]
pub(super) struct CharacterPresence {
    #[bw(map = |indices: &Option<(u16, u16)>| match *indices {
        Some((world_index, land_index)) => {
            LOCATION_BASE | (world_index << 8) | INDEX_MARKER | land_index
        }
        None => 0,
    })]
    pub(super) world_land_indices: Option<(u16, u16)>,
    #[bw(calc = 0_u16)]
    unknown: u16,
}

#[derive(BinWrite)]
#[bw(magic = b"USR")]
pub(super) struct CharacterPresenceList(pub(super) EntranceList<CharacterPresence>);

#[cfg(test)]
mod tests {
    use binrw::{BinWriterExt, io::Cursor};
    use jiff::Timestamp;
    use shrimpman_common::binary::UnixTimestamp32;

    use super::*;
    use crate::application::use_cases::list_worlds::outbound::WorldListMetadata;

    fn worlds() -> MhfBin8<WorldList> {
        MhfBin8::new(WorldList(EntranceList {
            entries: Vec::new(),
            metadata: WorldListMetadata {
                server_time: UnixTimestamp32::from(
                    Timestamp::new(0x0102_0304, 0).expect("valid test timestamp"),
                ),
                max_guild_members: 60,
            },
        }))
    }

    fn presences() -> Vec<CharacterPresence> {
        vec![
            CharacterPresence {
                world_land_indices: Some((1, 2)),
            },
            CharacterPresence {
                world_land_indices: None,
            },
        ]
    }

    #[test]
    fn writes_location_codes_in_request_order() {
        let body = presences();
        let mut output = Cursor::new(Vec::new());

        output.write_be(&body).unwrap();

        assert_eq!(output.into_inner(), [0x11, 0x12, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn writes_the_usr_magic_and_empty_header() {
        let segment = CharacterPresenceList(Vec::new().into());
        let mut output = Cursor::new(Vec::new());

        output.write_be(&segment).unwrap();

        assert_eq!(output.into_inner(), b"USR\0\0\0\0");
    }

    #[test]
    fn appends_an_independent_usr_bin8_response() {
        let response = ListWorldsPresenceResponse {
            worlds: worlds(),
            presences: MhfBin8::new(CharacterPresenceList(presences().into())),
        };
        let presences = presences();
        let mut actual_output = Cursor::new(Vec::new());
        let mut expected_output = Cursor::new(Vec::new());

        actual_output.write_be(&response).unwrap();
        expected_output.write_be(&worlds()).unwrap();
        expected_output
            .write_be(&MhfBin8::new(CharacterPresenceList(presences.into())))
            .unwrap();

        assert_eq!(actual_output.into_inner(), expected_output.into_inner());
    }
}
