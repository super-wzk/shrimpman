use jiff::Timestamp;
use serde::Serialize;
use shrimpman_domain::character::{Character, Gender, WeaponType};

#[derive(Serialize)]
pub(super) struct ResponseBody {
    id: u32,
    name: String,
    gr: u16,
    hr: u16,
    weapon_type: WeaponType,
    gender: Gender,
    last_sign_in_at: Option<Timestamp>,
    is_new: bool,
}

impl ResponseBody {
    /// A character in a sign-in response, with its last sign-in time.
    pub(super) fn signed_in(character: Character, last_sign_in_at: Timestamp) -> Self {
        Self::from_character(character, Some(last_sign_in_at))
    }

    fn from_character(character: Character, last_sign_in_at: Option<Timestamp>) -> Self {
        let is_new = character.is_new();

        Self {
            id: character.id.into(),
            name: character.name,
            gr: character.gr,
            hr: character.hr,
            weapon_type: character.weapon_type,
            gender: character.gender,
            last_sign_in_at,
            is_new,
        }
    }
}

impl From<Character> for ResponseBody {
    fn from(character: Character) -> Self {
        Self::from_character(character, None)
    }
}
