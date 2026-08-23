use crate::{account::AccountRow, time_range::StoredTimeRange};

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
    pub(super) validity: StoredTimeRange,
}
