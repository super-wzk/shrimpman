use super::Error;
use crate::model::{IssuedSignSession, SignCharacter, SignInSuccess};
use binrw::{BinRead, binread};
use jiff::Timestamp;
use shrimpman_common::binary::{CountedVec, U8OrU16Length, UnixTimestamp32};
use shrimpman_domain::{
    TimeRange,
    account::CourseRights,
    character::{CharacterId, Gender, WeaponType},
    mezeporta::{MezeportaFesta, MezeportaStall},
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use std::{io::Cursor, net::SocketAddrV4};

// Lengths count raw bytes, including any C-string terminators. Erupe 9.2's
// shorter festival tail is read below.
#[binread]
struct Response {
    #[br(temp)]
    patch_count: u8,
    #[br(temp)]
    entrance_count: u8,
    #[br(temp, assert(character_count <= 16, "at most 16 characters are supported"))]
    character_count: u8,
    session_id: u32,
    token: [u8; SIGN_SESSION_TOKEN_LEN],
    issued_at: UnixTimestamp32,
    #[br(temp, count = patch_count)]
    _patch_servers: Vec<CountedVec<u8, u8>>,
    #[br(count = entrance_count)]
    entrance_servers: Vec<CountedVec<u8, u8>>,
    #[br(count = character_count)]
    characters: Vec<Character>,
    #[br(temp)]
    _friends: CountedVec<U8OrU16Length, Relation>,
    #[br(temp)]
    _guild_members: CountedVec<U8OrU16Length, Relation>,
    notices: CountedVec<u8, Notice>,
    last_character_id: u32,
    rights: u32,
    #[br(temp)]
    _filter: CountedVec<u16, u8>,
    #[br(temp)]
    _cap_link: CapLink,
    return_expires_at: UnixTimestamp32,
    #[br(temp)]
    _unknown: u32,
    festa: Festa,
}

#[binread]
struct Character {
    id: u32,
    hr: u16,
    weapon_type: u16,
    last_sign_in_at: UnixTimestamp32,
    gender: u8,
    #[br(assert(is_new <= 1, "invalid new-character flag"))]
    is_new: u8,
    legacy_gr: u8,
    #[br(assert(uses_u16_gr <= 1, "invalid GR flag"))]
    uses_u16_gr: u8,
    name: [u8; 16],
    #[br(temp)]
    _description: [u8; 32],
    gr: u16,
    #[br(temp)]
    _unknown: [u8; 2],
}

#[binread]
struct Relation {
    #[br(temp)]
    _source_character_id: u32,
    #[br(temp)]
    _related_character_id: u32,
    #[br(temp)]
    _name: CountedVec<u8, u8>,
}

#[binread]
struct Notice {
    #[br(temp)]
    _flags: u16,
    content: CountedVec<u16, u8>,
}

#[binread]
struct CapLink {
    #[br(temp)]
    key_type: u16,
    #[br(temp, if(key_type == 51728))]
    key_version: u16,
    #[br(temp, if(key_type == 51728 && matches!(key_version, 20000 | 20002)))]
    _key: Option<CountedVec<u16, u8>>,
    #[br(temp)]
    _entries: CountedVec<u8, CapLinkEntry>,
    #[br(temp)]
    host_type: u16,
    #[br(temp)]
    host_enabled: u16,
    #[br(temp)]
    host_version: u16,
    #[br(temp, if(host_type == 51729 && host_enabled == 1 && host_version == 20000))]
    _host: Option<CountedVec<u16, u8>>,
}

#[binread]
struct CapLinkEntry {
    #[br(temp)]
    _kind: u8,
    #[br(temp)]
    _id: u32,
    #[br(temp)]
    _text: CountedVec<u8, u8>,
}

#[derive(BinRead)]
struct Festa {
    id: u32,
    starts_at: UnixTimestamp32,
    expires_at: UnixTimestamp32,
}

#[derive(BinRead)]
struct FestaDetails {
    tickets: CountedVec<u8, u32>,
    stalls: CountedVec<u8, u8>,
}

pub(super) fn sign_in(bytes: &[u8]) -> Result<SignInSuccess, Error> {
    let Some((&status, body)) = bytes.split_first() else {
        return Err(Error::invalid_response("empty sign-in response"));
    };
    if status != 1 {
        return Err(Error::TcpResponse(status));
    }
    let mut reader = Cursor::new(body);
    let response = Response::read_be(&mut reader)
        .map_err(|error| Error::invalid_response(error.to_string()))?;
    let festa = response.festa.read_details(&mut reader)?;
    if reader.position() != body.len() as u64 {
        return Err(Error::invalid_response(format!(
            "unexpected bytes after sign-in response: decoded {} of {} bytes",
            reader.position() + 1,
            bytes.len(),
        )));
    }
    let entrance_servers = response
        .entrance_servers
        .into_iter()
        .map(|address| {
            let address = Vec::from(address);
            let address = address.strip_suffix(&[0]).unwrap_or(&address);
            std::str::from_utf8(address)
                .map_err(|error| {
                    Error::invalid_response(format!("invalid entrance address: {error}"))
                })?
                .parse::<SocketAddrV4>()
                .map_err(|error| {
                    Error::invalid_response(format!("invalid entrance address: {error}"))
                })
        })
        .collect::<Result<_, _>>()?;
    let characters = response
        .characters
        .into_iter()
        .map(Character::into_domain)
        .collect::<Result<_, _>>()?;
    let notices = Vec::from(response.notices)
        .into_iter()
        .map(|notice| Vec::from(notice.content))
        .collect();
    super::super::validate_sign_in(SignInSuccess {
        session: IssuedSignSession {
            session_id: SignSessionId::from(response.session_id),
            token: response.token,
            issued_at: response.issued_at.into(),
        },
        entrance_servers,
        characters,
        notices,
        last_character_id: (response.last_character_id != 0)
            .then(|| CharacterId::from(response.last_character_id)),
        rights: CourseRights::from_bits_retain(response.rights),
        return_expires_at: response.return_expires_at.into(),
        festa,
    })
}

impl Character {
    fn into_domain(self) -> Result<SignCharacter, Error> {
        let weapon_type = match self.weapon_type {
            0 => WeaponType::SwordAndShield,
            1 => WeaponType::HeavyBowgun,
            2 => WeaponType::Hammer,
            3 => WeaponType::GreatSword,
            4 => WeaponType::Lance,
            5 => WeaponType::LightBowgun,
            6 => WeaponType::LongSword,
            7 => WeaponType::DualBlades,
            8 => WeaponType::HuntingHorn,
            9 => WeaponType::Gunlance,
            10 => WeaponType::Bow,
            11 => WeaponType::Tonfa,
            12 => WeaponType::SwitchAxe,
            13 => WeaponType::MagnetSpike,
            _ => return Err(Error::invalid_response("invalid character weapon type")),
        };
        let gender = match self.gender {
            0 => Gender::Male,
            1 => Gender::Female,
            _ => return Err(Error::invalid_response("invalid character gender")),
        };
        let last_sign_in_at = Timestamp::from(self.last_sign_in_at);
        Ok(SignCharacter {
            id: CharacterId::from(self.id),
            name: self.name.to_vec(),
            hr: self.hr,
            gr: if self.uses_u16_gr != 0 {
                self.gr
            } else {
                u16::from(self.legacy_gr)
            },
            weapon_type,
            gender,
            last_sign_in_at: (last_sign_in_at != Timestamp::UNIX_EPOCH).then_some(last_sign_in_at),
            is_new: self.is_new != 0,
        })
    }
}

impl Festa {
    fn read_details(self, reader: &mut Cursor<&[u8]>) -> Result<Option<MezeportaFesta>, Error> {
        // Erupe 9.2 ends a disabled festival after two zero timestamps, even
        // though it supplies a nonzero ID. Only that exact short tail is valid.
        if Timestamp::from(self.starts_at) == Timestamp::UNIX_EPOCH
            && Timestamp::from(self.expires_at) == Timestamp::UNIX_EPOCH
            && reader.position() == reader.get_ref().len() as u64
        {
            return Ok(None);
        }
        let details = FestaDetails::read_be(reader)
            .map_err(|error| Error::invalid_response(error.to_string()))?;
        if self.id == 0 {
            return Ok(None);
        }
        let tickets = Vec::from(details.tickets);
        let [solo_ticket_allowance, group_ticket_allowance] = tickets.as_slice() else {
            return Err(Error::invalid_response(
                "festival must contain two ticket allowances",
            ));
        };
        let stalls = Vec::from(details.stalls)
            .into_iter()
            .map(|stall| {
                Ok(match stall {
                    2 => MezeportaStall::TokotokoPartnya,
                    3 => MezeportaStall::Unknown3,
                    4 => MezeportaStall::VolpakkunTogether,
                    5 => MezeportaStall::Unknown5,
                    6 => MezeportaStall::Unknown6,
                    7 => MezeportaStall::Unknown7,
                    8 => MezeportaStall::Unknown8,
                    9 => MezeportaStall::Unknown9,
                    10 => MezeportaStall::Unknown10,
                    _ => return Err(Error::invalid_response("invalid festival stall")),
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Some(MezeportaFesta {
            id: self.id,
            period: TimeRange::new(self.starts_at.into(), self.expires_at.into()),
            solo_ticket_allowance: *solo_ticket_allowance,
            group_ticket_allowance: *group_ticket_allowance,
            stalls,
        }))
    }
}
