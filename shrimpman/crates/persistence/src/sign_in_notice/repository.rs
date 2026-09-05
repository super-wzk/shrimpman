use jiff::Timestamp;
use shrimpman_domain::sign_in_notice::SignInNotice;
use toasty::Db;

use super::SignInNoticeRow;

/// Toasty-backed Sign-in notice persistence.
#[derive(Debug, Clone)]
pub struct SignInNoticeRepository {
    db: Db,
}

impl SignInNoticeRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn list_active_at(
        &self,
        timestamp: Timestamp,
        limit: usize,
    ) -> toasty::Result<Vec<SignInNotice>> {
        let mut db = self.db.clone();
        let notices = toasty::query!(
            SignInNoticeRow FILTER
                .starts_at <= #timestamp
                AND .expires_at >= #timestamp
        )
        .order_by(SignInNoticeRow::fields().priority().desc())
        .order_by(SignInNoticeRow::fields().id().desc())
        .limit(limit)
        .exec(&mut db)
        .await?;

        Ok(notices.into_iter().map(SignInNotice::from).collect())
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;
    use shrimpman_domain::TimeRange;

    use super::*;
    #[tokio::test]
    async fn lists_active_notices_by_priority_with_a_limit() {
        let db = crate::test_database().await;
        let now = Timestamp::new(1_800_000_000, 0).unwrap();
        let active = TimeRange::from_duration(
            now - SignedDuration::from_hours(1),
            SignedDuration::from_hours(2),
        );
        let expired = TimeRange::from_duration(
            now - SignedDuration::from_hours(2),
            SignedDuration::from_hours(1),
        );
        let mut connection = db.clone();

        for (content, period, priority) in [
            ("lower", active, 1),
            ("expired", expired, 10),
            ("higher", active, 2),
        ] {
            let starts_at = period.starts_at();
            let expires_at = period.expires_at();
            toasty::create!(SignInNoticeRow {
                content: content.to_owned(),
                starts_at,
                expires_at,
                priority,
            })
            .exec(&mut connection)
            .await
            .unwrap();
        }

        let notices = SignInNoticeRepository::new(&db)
            .list_active_at(now, 1)
            .await
            .unwrap();

        assert_eq!(notices.len(), 1);
        assert_eq!(notices[0].content, "higher");
    }
}
