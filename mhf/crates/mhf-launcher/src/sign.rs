use crate::{MhfConfig, TranslationConfig, runtime::SignEncoding};
use jiff::Timestamp;
use shrimpman_domain::{
    account::CourseRights,
    character::{CharacterId, Gender, WeaponType},
    mezeporta::MezeportaFesta,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use std::net::SocketAddrV4;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Config {
    pub credentials: PasswordCredentials,
    pub sign_encoding: SignEncoding,
    pub sign_in: SignInSuccess,
    pub selected_character_id: CharacterId,
    pub translation: Option<TranslationConfig>,
    pub mhf: MhfConfig,
}

#[derive(Debug, PartialEq, Eq)]
pub struct PasswordCredentials {
    pub username: String,
    pub password: String,
}

#[derive(Debug, PartialEq, Eq)]
pub struct IssuedSignSession {
    pub session_id: SignSessionId,
    pub token: [u8; SIGN_SESSION_TOKEN_LEN],
    pub issued_at: Timestamp,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SignCharacter {
    pub id: CharacterId,
    pub name: Vec<u8>,
    pub gr: u16,
    pub hr: u16,
    pub weapon_type: WeaponType,
    pub gender: Gender,
    pub last_sign_in_at: Option<Timestamp>,
    pub is_new: bool,
}

#[derive(Debug, PartialEq, Eq)]
pub struct SignInSuccess {
    pub session: IssuedSignSession,
    pub entrance_servers: Vec<SocketAddrV4>,
    pub characters: Vec<SignCharacter>,
    pub notices: Vec<Vec<u8>>,
    pub last_character_id: Option<CharacterId>,
    pub rights: CourseRights,
    pub return_expires_at: Timestamp,
    pub festa: Option<MezeportaFesta>,
}

impl SignInSuccess {
    pub(crate) fn selected_character(
        &self,
        selected_character_id: CharacterId,
    ) -> Result<&SignCharacter, String> {
        if self.entrance_servers.is_empty() {
            return Err("sign-in result has no entrance server".to_owned());
        }
        if self
            .entrance_servers
            .iter()
            .any(|server| server.port() == 0)
        {
            return Err("sign-in result contains an entrance server with port 0".to_owned());
        }
        if self.characters.len() > 16 {
            return Err(format!(
                "sign-in result has {} characters; at most 16 are supported",
                self.characters.len(),
            ));
        }
        if u32::from(self.session.session_id) == 0 {
            return Err("sign session ID must not be 0".to_owned());
        }
        if self
            .characters
            .iter()
            .any(|character| u32::from(character.id) == 0)
        {
            return Err("character IDs must not be 0".to_owned());
        }

        self.characters
            .iter()
            .find(|character| character.id == selected_character_id)
            .ok_or_else(|| {
                "selected_character_id does not identify an authenticated character".to_owned()
            })
    }
}
