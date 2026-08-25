use shrimpman_domain::{TimeRange, sign_in_notice::SignInNotice};

use super::SignInNoticeRow;

impl From<SignInNoticeRow> for SignInNotice {
    fn from(notice: SignInNoticeRow) -> Self {
        Self {
            id: notice.id,
            content: notice.content,
            period: TimeRange::new(notice.starts_at, notice.expires_at),
            priority: notice.priority,
        }
    }
}
