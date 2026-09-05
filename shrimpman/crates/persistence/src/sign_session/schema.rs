use jiff::Timestamp;

use crate::account::AccountRow;

#[derive(Debug, toasty::Model)]
#[table = "sign_sessions"]
pub(crate) struct SignSessionRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    #[index]
    pub(super) account_id: u32,

    #[belongs_to]
    account: toasty::Deferred<AccountRow>,

    pub(super) token_hash: Vec<u8>,

    #[column("validity_starts_at")]
    pub(super) starts_at: Timestamp,

    #[column("validity_expires_at")]
    pub(super) expires_at: Timestamp,
}
