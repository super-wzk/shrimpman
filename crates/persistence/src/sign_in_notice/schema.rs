use jiff::Timestamp;

#[derive(Debug, toasty::Model)]
#[table = "sign_in_notices"]
#[index(priority, id)]
pub(crate) struct SignInNoticeRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    pub(super) content: String,

    #[column("period_starts_at")]
    pub(super) starts_at: Timestamp,

    #[column("period_expires_at")]
    pub(super) expires_at: Timestamp,

    #[default(0)]
    pub(super) priority: i32,
}
