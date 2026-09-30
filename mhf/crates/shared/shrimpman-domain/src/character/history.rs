use std::collections::HashMap;

use jiff::Timestamp;

use super::CharacterId;

#[derive(Default)]
pub struct CharacterSignInHistory {
    last_character_id: Option<CharacterId>,
    sign_ins: HashMap<CharacterId, Timestamp>,
}

impl CharacterSignInHistory {
    pub fn new(
        last_character_id: Option<CharacterId>,
        sign_ins: impl IntoIterator<Item = (CharacterId, Timestamp)>,
    ) -> Self {
        Self {
            last_character_id,
            sign_ins: sign_ins.into_iter().collect(),
        }
    }

    pub fn last_character_id(&self) -> Option<CharacterId> {
        self.last_character_id
    }

    pub fn last_sign_in_at(&self, character_id: CharacterId) -> Option<Timestamp> {
        self.sign_ins.get(&character_id).copied()
    }
}
