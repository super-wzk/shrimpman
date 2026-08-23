use jiff::Timestamp;

use super::{StoredGender, StoredWeaponType};
use crate::account::AccountRow;

#[derive(Debug, toasty::Model)]
#[table = "characters"]
pub(crate) struct CharacterRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    #[index]
    pub(super) account_id: u32,

    #[belongs_to]
    account: toasty::Deferred<AccountRow>,

    #[has_many(pair = character)]
    sign_in_records: toasty::Deferred<Vec<CharacterSignInRecordRow>>,

    #[default(StoredGender::Male)]
    pub(super) gender: StoredGender,

    pub(super) savedata: Option<Vec<u8>>,

    #[default(String::new())]
    pub(super) name: String,

    #[default(String::new())]
    pub(super) description: String,

    #[default(0)]
    pub(super) gr: u16,

    #[default(0)]
    pub(super) hr: u16,

    #[default(StoredWeaponType::SwordAndShield)]
    pub(super) weapon_type: StoredWeaponType,

    pub(super) deleted_at: Option<Timestamp>,
}

#[derive(Debug, toasty::Model)]
#[table = "character_sign_in_records"]
#[index(character_id, signed_in_at, id)]
pub(crate) struct CharacterSignInRecordRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    pub(super) character_id: u32,

    #[belongs_to]
    character: toasty::Deferred<CharacterRow>,

    pub(super) signed_in_at: Timestamp,
}
