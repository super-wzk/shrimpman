use jiff::Timestamp;
use shrimpman_domain::{
    TimeRange,
    account::{Account, return_period},
};
use toasty::Db;

use super::{AccountReturnPeriodRow, AccountRow, AccountSignInRecordRow};
use crate::time_range::StoredTimeRange;

/// Toasty-backed account persistence.
#[derive(Debug, Clone)]
pub struct AccountRepository {
    db: Db,
}

impl AccountRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn find_by_username(&self, username: &str) -> toasty::Result<Option<Account>> {
        let mut db = self.db.clone();
        let account = toasty::query!(AccountRow FILTER .username == #username)
            .first()
            .exec(&mut db)
            .await?;

        Ok(account.map(Account::from))
    }

    pub async fn create(&self, username: String, password_hash: String) -> toasty::Result<Account> {
        let mut db = self.db.clone();
        let account = toasty::create!(AccountRow {
            username,
            password_hash,
        })
        .exec(&mut db)
        .await?;

        Ok(account.into())
    }

    pub async fn record_sign_in(
        &self,
        account: &Account,
        signed_in_at: Timestamp,
    ) -> toasty::Result<TimeRange> {
        let mut db = self.db.clone();
        let mut transaction = db.transaction_builder().begin().await?;
        let account_id = u32::from(account.id);
        let last_sign_in_at = toasty::query!(
            AccountSignInRecordRow FILTER .account_id == #account_id
        )
        .order_by(AccountSignInRecordRow::fields().signed_in_at().desc())
        .order_by(AccountSignInRecordRow::fields().id().desc())
        .first()
        .exec(&mut transaction)
        .await?
        .map(|record| record.signed_in_at);
        let current_period = if return_period::should_start(last_sign_in_at, signed_in_at) {
            None
        } else {
            toasty::query!(AccountReturnPeriodRow FILTER .account_id == #account_id)
                .order_by(AccountReturnPeriodRow::fields().id().desc())
                .first()
                .exec(&mut transaction)
                .await?
        };
        let return_period = match current_period {
            Some(period) => period.period.into(),
            None => {
                let period = return_period::starting_at(signed_in_at);
                let stored_period = StoredTimeRange::from(period);
                toasty::create!(AccountReturnPeriodRow {
                    account_id,
                    period: stored_period,
                })
                .exec(&mut transaction)
                .await?;
                period
            }
        };

        toasty::create!(AccountSignInRecordRow {
            account_id,
            signed_in_at,
        })
        .exec(&mut transaction)
        .await?;
        transaction.commit().await?;

        Ok(return_period)
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;
    #[tokio::test]
    async fn keeps_one_row_per_return_period() {
        let db = crate::test_database().await;
        let repository = AccountRepository::new(&db);
        let account = repository
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let first_sign_in = Timestamp::new(1_700_000_000, 0).unwrap();
        let second_sign_in = first_sign_in + SignedDuration::from_hours(1);
        let returning_sign_in = second_sign_in + SignedDuration::from_hours(91 * 24);

        let first_period = repository
            .record_sign_in(&account, first_sign_in)
            .await
            .unwrap();
        let second_period = repository
            .record_sign_in(&account, second_sign_in)
            .await
            .unwrap();
        let returning_period = repository
            .record_sign_in(&account, returning_sign_in)
            .await
            .unwrap();

        assert_eq!(second_period.starts_at(), first_period.starts_at());
        assert_eq!(second_period.expires_at(), first_period.expires_at());
        assert_eq!(returning_period.starts_at(), returning_sign_in);

        let mut db = db.clone();
        let account_id = u32::from(account.id);
        assert_eq!(
            toasty::query!(AccountReturnPeriodRow FILTER .account_id == #account_id)
                .exec(&mut db)
                .await
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            toasty::query!(AccountSignInRecordRow FILTER .account_id == #account_id)
                .exec(&mut db)
                .await
                .unwrap()
                .len(),
            3
        );
    }
}
