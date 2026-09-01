use serde::Serialize;
use shrimpman_domain::character::Character;

#[derive(Serialize)]
pub(super) struct ResponseBody {
    id: u32,
    name: String,
    gr: u16,
    hr: u16,
    is_new: bool,
}

impl From<Character> for ResponseBody {
    fn from(character: Character) -> Self {
        let is_new = character.is_new();

        Self {
            id: character.id.into(),
            name: character.name,
            gr: character.gr,
            hr: character.hr,
            is_new,
        }
    }
}
