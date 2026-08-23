use crate::time_range::StoredTimeRange;

#[derive(Debug, toasty::Model)]
#[table = "sign_in_notices"]
#[index(priority, id)]
pub(crate) struct SignInNoticeRow {
    #[key]
    #[auto]
    pub(super) id: u32,

    pub(super) content: String,
    pub(super) period: StoredTimeRange,

    #[default(0)]
    pub(super) priority: i32,
}
