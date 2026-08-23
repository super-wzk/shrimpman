use shrimpman_domain::sign_in_notice::SignInNotice;

use super::SignInNoticeRow;

impl From<SignInNoticeRow> for SignInNotice {
    fn from(notice: SignInNoticeRow) -> Self {
        Self {
            id: notice.id,
            content: notice.content,
            period: notice.period.into(),
            priority: notice.priority,
        }
    }
}
