use super::{Error, character};
use jiff::Timestamp;
use mhf_launcher::{IssuedSignSession, SignInSuccess};
use serde::{Deserialize, Serialize};
use shrimpman_domain::{
    TimeRange,
    account::CourseRights,
    character::CharacterId,
    mezeporta::{MezeportaFesta, MezeportaStall},
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use std::net::SocketAddrV4;

#[derive(Serialize)]
pub(super) struct Request<'a> {
    pub(super) username: &'a str,
    pub(super) password: &'a str,
}

#[derive(Deserialize)]
pub(super) struct Response {
    session: Session,
    entrance_servers: Vec<String>,
    characters: Vec<character::Response>,
    notices: Vec<String>,
    last_character_id: Option<u32>,
    rights: u32,
    return_expires_at: Timestamp,
    festa: Option<Festa>,
}

#[derive(Deserialize)]
struct Session {
    session_id: u32,
    token: String,
    issued_at: Timestamp,
}

#[derive(Deserialize)]
struct Festa {
    id: u32,
    starts_at: Timestamp,
    expires_at: Timestamp,
    solo_ticket_allowance: u32,
    group_ticket_allowance: u32,
    stalls: Vec<MezeportaStall>,
}

impl Response {
    pub(super) fn into_domain(self) -> Result<SignInSuccess, Error> {
        let token = self.session.token.into_bytes().try_into().map_err(|_| {
            Error::invalid_response(format!(
                "Sign session token must contain exactly {SIGN_SESSION_TOKEN_LEN} bytes"
            ))
        })?;
        let entrance_servers = self
            .entrance_servers
            .into_iter()
            .map(|server| {
                server.parse::<SocketAddrV4>().map_err(|error| {
                    Error::invalid_response(format!(
                        "entrance server address {server:?} is invalid: {error}"
                    ))
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let characters = self
            .characters
            .into_iter()
            .map(character::Response::into_domain)
            .collect::<Result<Vec<_>, _>>()?;
        let last_character_id = self.last_character_id.map(CharacterId::from);

        super::super::validate_sign_in(SignInSuccess {
            session: IssuedSignSession {
                session_id: SignSessionId::from(self.session.session_id),
                token,
                issued_at: self.session.issued_at,
            },
            entrance_servers,
            characters,
            notices: self.notices.into_iter().map(String::into_bytes).collect(),
            last_character_id,
            rights: CourseRights::from_bits_retain(self.rights),
            return_expires_at: self.return_expires_at,
            festa: self.festa.map(Festa::into_domain),
        })
    }
}

impl Festa {
    fn into_domain(self) -> MezeportaFesta {
        MezeportaFesta {
            id: self.id,
            period: TimeRange::new(self.starts_at, self.expires_at),
            solo_ticket_allowance: self.solo_ticket_allowance,
            group_ticket_allowance: self.group_ticket_allowance,
            stalls: self.stalls,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_notices_non_ascii_token_and_festa_into_the_launcher_domain() {
        let starts_at = Timestamp::new(1_800_000_000, 0).unwrap();
        let expires_at = Timestamp::new(1_800_003_600, 0).unwrap();
        let response = Response {
            session: Session {
                session_id: 1,
                token: "令牌0123456789".to_owned(),
                issued_at: starts_at,
            },
            entrance_servers: vec!["127.0.0.1:53310".to_owned()],
            characters: Vec::new(),
            notices: vec!["Welcome".to_owned()],
            last_character_id: None,
            rights: 12,
            return_expires_at: expires_at,
            festa: Some(Festa {
                id: 7,
                starts_at,
                expires_at,
                solo_ticket_allowance: 5,
                group_ticket_allowance: 2,
                stalls: vec![MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether],
            }),
        };

        let sign_in = response.into_domain().unwrap();
        assert_eq!(sign_in.session.token, "令牌0123456789".as_bytes());
        assert_eq!(sign_in.notices, [b"Welcome".to_vec()]);
        assert_eq!(
            sign_in.festa,
            Some(MezeportaFesta {
                id: 7,
                period: TimeRange::new(starts_at, expires_at),
                solo_ticket_allowance: 5,
                group_ticket_allowance: 2,
                stalls: vec![MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether],
            })
        );
    }
}
