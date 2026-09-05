use std::net::Ipv4Addr;

use binrw::{BinWrite, NullString, binwrite};
use shrimpman_common::binary::UnixTimestamp32;
use shrimpman_domain::world::{ClientCompatibility, WorldContent, WorldSeason, WorldType};

use crate::entrance_list::EntranceList;

const INDEX_MARKER: u16 = 0x10;
pub(super) const MAX_INDEXED_ENTRIES: usize = INDEX_MARKER as usize;

/// The available Worlds returned to an Entrance client.
#[derive(BinWrite)]
#[bw(magic = b"SV2")]
pub(crate) struct WorldList(pub(crate) EntranceList<WorldEntry, WorldListMetadata>);

#[derive(BinWrite)]
pub(crate) struct WorldListMetadata {
    pub(crate) server_time: UnixTimestamp32,
    pub(crate) max_guild_members: u32,
}

#[binwrite]
pub(crate) struct WorldEntry {
    #[bw(map = |address: &Ipv4Addr| address.to_bits(), little)]
    pub(crate) address: Ipv4Addr,
    #[bw(map = |index: &u16| *index | INDEX_MARKER)]
    pub(crate) index: u16,
    #[bw(calc = 0_u16)]
    reserved: u16,
    #[bw(try_calc = u16::try_from(lands.len()))]
    land_count: u16,
    #[bw(map = |world_type: &WorldType| *world_type as u8)]
    pub(crate) world_type: WorldType,
    #[bw(map = |season: &WorldSeason| *season as u8)]
    pub(crate) season: WorldSeason,
    #[bw(map = |content: &WorldContent| *content as u8)]
    pub(crate) content: WorldContent,
    pub(crate) text: WorldText,
    #[bw(map = |compatibility: &ClientCompatibility| *compatibility as u32)]
    pub(crate) client_compatibility: ClientCompatibility,
    pub(crate) lands: Vec<LandEntry>,
}

#[binwrite]
#[bw(assert(
    name.len() + description.len() + 2 <= 65,
    "world name and description exceed their 65-byte field"
))]
pub(crate) struct WorldText {
    #[bw(calc = 0_u8)]
    legacy_length: u8,
    pub(crate) name: NullString,
    #[bw(pad_after = 65 - name.len() - description.len() - 2)]
    pub(crate) description: NullString,
}

#[binwrite]
pub(crate) struct LandEntry {
    pub(crate) port: u16,
    #[bw(map = |index: &u16| *index | INDEX_MARKER)]
    pub(crate) index: u16,
    pub(crate) max_players: u16,
    pub(crate) current_players: u16,
    #[bw(calc = [0_u16; 4])]
    reserved: [u16; 4],
    #[bw(calc = 0_u16)]
    initial_friend_count: u16,
    #[bw(calc = 0_u16)]
    initial_guild_member_count: u16,
    #[bw(calc = [
        319_u16,
        254_u16.wrapping_sub(*current_players),
        255_u16.wrapping_sub(*current_players),
        12_345_u16,
    ])]
    opaque_tail: [u16; 4],
}

#[cfg(test)]
mod tests {
    use binrw::{BinWriterExt, io::Cursor};
    use jiff::Timestamp;

    use super::*;
    use crate::MhfBin8;

    fn land() -> LandEntry {
        LandEntry {
            port: 54_001,
            index: 0,
            max_players: 100,
            current_players: 5,
        }
    }

    fn server_time() -> UnixTimestamp32 {
        Timestamp::new(0x0102_0304, 0).unwrap().into()
    }

    #[test]
    fn writes_a_modern_world_entry() {
        let world = WorldEntry {
            address: Ipv4Addr::LOCALHOST,
            index: 0,
            world_type: WorldType::Free,
            season: WorldSeason::Cold,
            content: WorldContent::GRankQuests,
            text: WorldText {
                name: NullString::from("World"),
                description: NullString::from("Description"),
            },
            client_compatibility: ClientCompatibility::PSV,
            lands: vec![land()],
        };
        let mut output = Cursor::new(Vec::new());

        output.write_be(&world).unwrap();
        let output = output.into_inner();

        assert_eq!(output.len(), 111);
        assert_eq!(&output[0..4], &[1, 0, 0, 127]);
        assert_eq!(&output[4..14], &[0, 16, 0, 0, 0, 1, 1, 2, 5, 0]);
        assert_eq!(&output[14..31], b"World\0Description");
        assert!(output[31..79].iter().all(|byte| *byte == 0));
        assert_eq!(&output[79..83], &0x2042_u32.to_be_bytes());
        assert_eq!(&output[83..91], &[0xd2, 0xf1, 0, 16, 0, 100, 0, 5]);
        assert!(output[91..103].iter().all(|byte| *byte == 0));
        assert_eq!(
            &output[103..111],
            &[0x01, 0x3f, 0, 249, 0, 250, 0x30, 0x39]
        );
    }

    #[test]
    fn rejects_world_text_exceeding_its_fixed_width() {
        let text = WorldText {
            name: NullString(vec![b'a'; 64]),
            description: NullString::default(),
        };
        let mut output = Cursor::new(Vec::new());

        assert!(output.write_be(&text).is_err());
    }

    #[test]
    fn wraps_the_world_list_in_an_sv2_bin8_response() {
        let response = MhfBin8::new(WorldList(EntranceList {
            entries: Vec::new(),
            metadata: WorldListMetadata {
                server_time: server_time(),
                max_guild_members: 60,
            },
        }));
        let body = [1, 2, 3, 4, 0, 0, 0, 60];
        let mut segment = b"SV2\0\0\0\x08".to_vec();
        segment.extend_from_slice(&0x01ed_a7a7_u32.to_be_bytes());
        segment.extend_from_slice(&body);
        let expected = MhfBin8::new(segment);
        let mut actual_output = Cursor::new(Vec::new());
        let mut expected_output = Cursor::new(Vec::new());

        actual_output.write_be(&response).unwrap();
        expected_output.write_be(&expected).unwrap();

        assert_eq!(actual_output.into_inner(), expected_output.into_inner());
    }
}
