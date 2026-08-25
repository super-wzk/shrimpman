use jiff::Timestamp;
use shrimpman_domain::account::CourseRights;

use crate::{character::CharacterRow, sign_session::SignSessionRow};

const DEFAULT_RIGHTS: CourseRights = CourseRights::HUNTER_LIFE.union(CourseRights::EXTRA_A);

#[derive(Debug, toasty::Model)]
#[table = "accounts"]
pub(crate) struct AccountRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    #[unique]
    pub(super) username: String,

    pub(super) password_hash: String,

    #[default(DEFAULT_RIGHTS.bits())]
    pub(super) rights: u32,

    #[has_many(pair = account)]
    characters: toasty::Deferred<Vec<CharacterRow>>,

    #[has_many(pair = account)]
    sign_in_records: toasty::Deferred<Vec<AccountSignInRecordRow>>,

    #[has_many(pair = account)]
    return_periods: toasty::Deferred<Vec<AccountReturnPeriodRow>>,

    #[has_many(pair = account)]
    sign_sessions: toasty::Deferred<Vec<SignSessionRow>>,
}

#[derive(Debug, toasty::Model)]
#[table = "account_sign_in_records"]
#[index(account_id, signed_in_at, id)]
pub(crate) struct AccountSignInRecordRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    pub(super) account_id: u32,

    #[belongs_to]
    account: toasty::Deferred<AccountRow>,

    pub(super) signed_in_at: Timestamp,
}

#[derive(Debug, toasty::Model)]
#[table = "account_return_periods"]
#[index(account_id, id)]
pub(crate) struct AccountReturnPeriodRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    pub(super) account_id: u32,

    #[belongs_to]
    account: toasty::Deferred<AccountRow>,

    #[column("period_starts_at")]
    pub(super) starts_at: Timestamp,

    #[column("period_expires_at")]
    pub(super) expires_at: Timestamp,
}
