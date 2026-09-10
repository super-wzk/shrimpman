use super::Error;
use crate::model::SignCharacter;
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use shrimpman_domain::character::{CharacterId, Gender, WeaponType};

#[derive(Serialize)]
pub(super) struct SessionRequest<'a> {
    pub(super) session_id: u32,
    pub(super) session_token: &'a str,
}

#[derive(Deserialize)]
pub(super) struct Response {
    id: u32,
    name: String,
    gr: u16,
    hr: u16,
    weapon_type: WeaponType,
    gender: Gender,
    last_sign_in_at: Option<Timestamp>,
    is_new: bool,
}

impl Response {
    pub(super) fn into_domain(self) -> Result<SignCharacter, Error> {
        if self.id == 0 {
            return Err(Error::invalid_response("character ID must not be 0"));
        }

        Ok(SignCharacter {
            id: CharacterId::from(self.id),
            name: self.name.into_bytes(),
            gr: self.gr,
            hr: self.hr,
            weapon_type: self.weapon_type,
            gender: self.gender,
            last_sign_in_at: self.last_sign_in_at,
            is_new: self.is_new,
        })
    }
}
