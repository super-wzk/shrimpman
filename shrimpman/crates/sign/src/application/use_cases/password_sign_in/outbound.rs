use binrw::{BinWrite, binwrite};
use jiff::Timestamp;
use shrimpman_common::binary::{
    Bool8, CountedVec, FixedCString, PrefixedCString, U8OrU16Length, UnixTimestamp32,
};
use shrimpman_domain::{
    account::CourseRights,
    character::{CharacterId, Gender, WeaponType},
    mezeporta::MezeportaFesta,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
    sign_in_notice::SignInNotice,
};

use super::model;
use crate::InternalError;

const CHARACTER_NAME_LEN: usize = 16;
const CHARACTER_DESCRIPTION_LEN: usize = 32;
const EMPTY_TEXT_FILTER: [u8; 26] = *b"\0\x18smc\0\0\0\0\0nam\0\0\0\0\0msg\0\0\0\0\0";

// Handler outbounds are type-erased into a heap allocation immediately.
#[allow(clippy::large_enum_variant)]
#[derive(BinWrite)]
pub(super) enum PasswordSignInResponse {
    #[bw(magic = 1_u8)]
    Success(SignInSuccess),
    #[bw(magic = 3_u8)]
    IllegalInput,
    #[bw(magic = 12_u8)]
    WrongPassword,
}

#[derive(BinWrite)]
struct IssuedSignSession {
    #[bw(map = |id: &SignSessionId| u32::from(*id))]
    session_id: SignSessionId,
    token: [u8; SIGN_SESSION_TOKEN_LEN],
    timestamp: UnixTimestamp32,
}

#[binwrite]
pub(super) struct SignInSuccess {
    #[bw(try_calc = u8::try_from(patch_servers.len()))]
    patch_server_count: u8,
    #[bw(try_calc = u8::try_from(entrance_servers.len()))]
    entrance_server_count: u8,
    #[bw(try_calc = u8::try_from(characters.len()))]
    character_count: u8,
    session: IssuedSignSession,
    patch_servers: Vec<PrefixedCString<u8>>,
    entrance_servers: Vec<PrefixedCString<u8>>,
    characters: Vec<SignCharacter>,
    friends: CountedVec<U8OrU16Length, CharacterRelationEntry>,
    guild_members: CountedVec<U8OrU16Length, CharacterRelationEntry>,
    notices: CountedVec<u8, LoginNotice>,
    #[bw(map = |id: &Option<CharacterId>| (*id).map(u32::from).unwrap_or_default())]
    last_character_id: Option<CharacterId>,
    #[bw(map = |rights: &CourseRights| rights.bits())]
    rights: CourseRights,
    #[bw(calc = EMPTY_TEXT_FILTER)]
    filter: [u8; EMPTY_TEXT_FILTER.len()],
    // CAPLINK has been discontinued, so its configuration is always disabled.
    #[bw(calc = [0_u8; 9])]
    cap_link: [u8; 9],
    return_expires_at: UnixTimestamp32,
    #[bw(calc = 0_u32)]
    unknown: u32,
    festa: SignInMezeportaFesta,
}

#[derive(BinWrite)]
struct SignInMezeportaFesta {
    id: u32,
    starts_at: UnixTimestamp32,
    ends_at: UnixTimestamp32,
    tickets: CountedVec<u8, u32>,
    stalls: CountedVec<u8, u8>,
}

#[binwrite]
struct LoginNotice {
    // Erupe leaves all login notice flags disabled.
    #[bw(calc = 0_u16)]
    flags: u16,
    content: PrefixedCString<u16>,
}

impl TryFrom<SignInNotice> for LoginNotice {
    type Error = InternalError;

    fn try_from(notice: SignInNotice) -> Result<Self, Self::Error> {
        Ok(Self {
            content: PrefixedCString::new(notice.content)?,
        })
    }
}

impl TryFrom<model::Outcome> for PasswordSignInResponse {
    type Error = InternalError;

    fn try_from(outcome: model::Outcome) -> Result<Self, Self::Error> {
        match outcome {
            model::Outcome::Success(success) => {
                Ok(Self::Success(SignInSuccess::try_from(*success)?))
            }
            model::Outcome::IllegalInput => Ok(Self::IllegalInput),
            model::Outcome::WrongPassword => Ok(Self::WrongPassword),
        }
    }
}

#[derive(BinWrite)]
struct CharacterRelationEntry {
    #[bw(map = |id: &CharacterId| u32::from(*id))]
    source_character_id: CharacterId,
    #[bw(map = |id: &CharacterId| u32::from(*id))]
    related_character_id: CharacterId,
    name: PrefixedCString<u8>,
}

#[binwrite]
struct SignCharacter {
    #[bw(map = |id: &CharacterId| u32::from(*id))]
    id: CharacterId,
    hr: u16,
    #[bw(map = |weapon_type: &WeaponType| *weapon_type as u16)]
    weapon_type: WeaponType,
    last_sign_in_at: UnixTimestamp32,
    #[bw(map = |gender: &Gender| *gender as u8)]
    gender: Gender,
    is_new: Bool8,
    #[bw(calc = 0_u8)]
    legacy_gr: u8,
    #[bw(calc = Bool8::from(true))]
    uses_u16_gr: Bool8,
    name: FixedCString<CHARACTER_NAME_LEN>,
    description: FixedCString<CHARACTER_DESCRIPTION_LEN>,
    gr: u16,
    #[bw(calc = 0_u8)]
    unknown_1: u8,
    #[bw(calc = 0_u8)]
    unknown_2: u8,
}

impl TryFrom<model::Success> for SignInSuccess {
    type Error = InternalError;

    fn try_from(success: model::Success) -> Result<Self, Self::Error> {
        Ok(Self {
            session: success.session.into(),
            patch_servers: Vec::new(),
            entrance_servers: success
                .entrance_servers
                .into_iter()
                .map(|address| PrefixedCString::new(address.into_bytes()))
                .collect::<Result<_, _>>()?,
            characters: success
                .characters
                .into_iter()
                .map(SignCharacter::try_from)
                .collect::<Result<_, _>>()?,
            friends: Vec::new().into(),
            guild_members: Vec::new().into(),
            notices: success
                .notices
                .into_iter()
                .map(LoginNotice::try_from)
                .collect::<Result<Vec<_>, _>>()?
                .into(),
            last_character_id: success.last_character_id,
            rights: success.rights,
            return_expires_at: success.return_expires_at.into(),
            festa: success
                .festa
                .map_or_else(SignInMezeportaFesta::disabled, SignInMezeportaFesta::from),
        })
    }
}

impl From<model::IssuedSession> for IssuedSignSession {
    fn from(session: model::IssuedSession) -> Self {
        Self {
            session_id: session.id,
            token: session.token,
            timestamp: session.issued_at.into(),
        }
    }
}

impl From<MezeportaFesta> for SignInMezeportaFesta {
    fn from(festa: MezeportaFesta) -> Self {
        Self {
            id: festa.id,
            starts_at: festa.period.starts_at().into(),
            ends_at: festa.period.expires_at().into(),
            tickets: vec![festa.solo_ticket_allowance, festa.group_ticket_allowance].into(),
            stalls: festa
                .stalls
                .into_iter()
                .map(|stall| stall as u8)
                .collect::<Vec<_>>()
                .into(),
        }
    }
}

impl SignInMezeportaFesta {
    fn disabled() -> Self {
        Self {
            id: 0,
            starts_at: Timestamp::UNIX_EPOCH.into(),
            ends_at: Timestamp::UNIX_EPOCH.into(),
            tickets: vec![0, 0].into(),
            stalls: Vec::new().into(),
        }
    }
}

impl TryFrom<model::SignedInCharacter> for SignCharacter {
    type Error = InternalError;

    fn try_from(signed_in: model::SignedInCharacter) -> Result<Self, Self::Error> {
        let character = signed_in.character;
        Ok(Self {
            id: character.id,
            hr: character.hr,
            weapon_type: character.weapon_type,
            last_sign_in_at: signed_in.last_sign_in_at.into(),
            gender: character.gender,
            is_new: character.is_new().into(),
            name: FixedCString::new(character.name.as_bytes())?,
            description: FixedCString::new(character.description.as_bytes())?,
            gr: character.gr,
        })
    }
}

#[cfg(test)]
mod tests {
    use binrw::io::Cursor;
    use jiff::SignedDuration;
    use shrimpman_domain::{
        TimeRange,
        character::{Character, CharacterId, Gender, WeaponType},
    };

    use super::*;

    #[test]
    fn encodes_login_notices() {
        let mut output = Cursor::new(Vec::new());

        CountedVec::<u8, LoginNotice>::new(vec![
            LoginNotice::try_from(notice("first")).unwrap(),
            LoginNotice::try_from(notice("second")).unwrap(),
        ])
        .write_be(&mut output)
        .unwrap();

        assert_eq!(
            output.into_inner(),
            b"\x02\0\0\0\x06first\0\0\0\0\x07second\0"
        );
    }

    #[test]
    fn encodes_login_notice_content_as_utf8() {
        let mut output = Cursor::new(Vec::new());
        let text = "啊🦐";

        LoginNotice::try_from(notice(text))
            .unwrap()
            .write_be(&mut output)
            .unwrap();

        let output = output.into_inner();
        assert_eq!(&output[..4], &[0, 0, 0, 8]);
        assert_eq!(std::str::from_utf8(&output[4..11]).unwrap(), text);
        assert_eq!(output[11], 0);
    }

    #[test]
    fn login_notice_length_counts_utf8_bytes_and_its_terminator() {
        let text = format!("{}ab", "界".repeat(21_844));
        let mut output = Cursor::new(Vec::new());
        LoginNotice::try_from(notice(&text))
            .unwrap()
            .write_be(&mut output)
            .unwrap();
        let output = output.into_inner();

        assert_eq!(&output[..4], &[0, 0, 0xFF, 0xFF]);
        assert_eq!(output.len(), 4 + usize::from(u16::MAX));
        assert_eq!(
            std::str::from_utf8(&output[4..output.len() - 1]).unwrap(),
            text
        );
        assert_eq!(output.last(), Some(&0));

        let oversized = LoginNotice::try_from(notice(&format!("{text}a"))).unwrap();
        assert!(oversized.write_be(&mut Cursor::new(Vec::new())).is_err());
        assert!(LoginNotice::try_from(notice("before\0after")).is_err());
    }

    #[test]
    fn encodes_the_success_response_header_and_character() {
        let timestamp_seconds = 1_800_000_000;
        let timestamp = Timestamp::new(timestamp_seconds, 0).unwrap();
        let name = "啊啊啊啊啊";
        let description = format!("{}🦐", "界".repeat(9));
        let success = SignInSuccess::try_from(model::Success {
            session: model::IssuedSession {
                id: SignSessionId::from(7),
                token: *b"0123456789ABCDEF",
                issued_at: timestamp,
            },
            entrance_servers: vec!["127.0.0.1:53310".to_owned()],
            characters: vec![model::SignedInCharacter {
                character: Character {
                    id: CharacterId::from(3),
                    gender: Gender::Female,
                    savedata: Some(vec![1]),
                    name: name.to_owned(),
                    description: description.clone(),
                    gr: 2,
                    hr: 1,
                    weapon_type: WeaponType::GreatSword,
                },
                last_sign_in_at: timestamp,
            }],
            notices: Vec::new(),
            last_character_id: None,
            rights: CourseRights::HUNTER_LIFE.union(CourseRights::EXTRA_A),
            return_expires_at: timestamp,
            festa: None,
        })
        .unwrap();
        let mut output = Cursor::new(Vec::new());

        PasswordSignInResponse::Success(success)
            .write_be(&mut output)
            .unwrap();
        let output = output.into_inner();

        assert_eq!(&output[..4], &[1, 0, 1, 1]);
        assert_eq!(&output[4..8], &7_u32.to_be_bytes());
        assert_eq!(&output[8..24], b"0123456789ABCDEF");
        assert_eq!(
            &output[24..28],
            &u32::try_from(timestamp_seconds).unwrap().to_be_bytes()
        );
        let entrance_server = b"\x10127.0.0.1:53310\0";
        assert_eq!(&output[28..28 + entrance_server.len()], entrance_server);
        let character_offset = 28 + entrance_server.len();
        assert_eq!(
            &output[character_offset..character_offset + 4],
            &3_u32.to_be_bytes()
        );
        let name_offset = character_offset + 16;
        assert_eq!(
            std::str::from_utf8(&output[name_offset..name_offset + 15]).unwrap(),
            name
        );
        assert_eq!(output[name_offset + 15], 0);
        let description_offset = name_offset + CHARACTER_NAME_LEN;
        assert_eq!(
            std::str::from_utf8(&output[description_offset..description_offset + 31]).unwrap(),
            description
        );
        assert_eq!(output[description_offset + 31], 0);
        let filter_offset = output
            .windows(EMPTY_TEXT_FILTER.len())
            .position(|window| window == EMPTY_TEXT_FILTER)
            .unwrap();
        let cap_link_offset = filter_offset + EMPTY_TEXT_FILTER.len();
        assert_eq!(&output[cap_link_offset..cap_link_offset + 9], &[0; 9]);
    }

    #[test]
    fn rejects_utf8_character_text_that_exceeds_its_fixed_field() {
        for (name, description) in [
            (format!("{}🦐", "界".repeat(4)), String::new()),
            (String::new(), format!("{}🦐🦐", "界".repeat(8))),
        ] {
            let character = model::SignedInCharacter {
                character: Character {
                    id: CharacterId::from(3),
                    gender: Gender::Female,
                    savedata: Some(vec![1]),
                    name,
                    description,
                    gr: 2,
                    hr: 1,
                    weapon_type: WeaponType::GreatSword,
                },
                last_sign_in_at: Timestamp::UNIX_EPOCH,
            };

            assert!(matches!(
                SignCharacter::try_from(character),
                Err(InternalError::FixedCString(_))
            ));
        }
    }

    fn notice(content: &str) -> SignInNotice {
        let starts_at = Timestamp::new(1_800_000_000, 0).unwrap();
        SignInNotice {
            id: 1,
            content: content.to_owned(),
            period: TimeRange::from_duration(starts_at, SignedDuration::from_hours(1)),
            priority: 0,
        }
    }
}
