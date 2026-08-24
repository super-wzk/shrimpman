use jiff::Timestamp;
use shrimpman_domain::{
    TimeRange,
    account::{Account, AccountId},
    session::SignSessionId,
};
use toasty::Db;

use super::SignSessionRow;
use crate::time_range::StoredTimeRange;

/// Toasty-backed Sign session persistence.
#[derive(Debug, Clone)]
pub struct SignSessionRepository {
    db: Db,
}

impl SignSessionRepository {
    /// Creates the repository from an already connected database.
    pub fn new(db: &Db) -> Self {
        Self { db: db.clone() }
    }

    pub async fn create(
        &self,
        account: &Account,
        token_hash: Vec<u8>,
        validity: TimeRange,
    ) -> toasty::Result<SignSessionId> {
        let mut db = self.db.clone();
        let account_id = u32::from(account.id);
        let validity = StoredTimeRange::from(validity);
        let session = toasty::create!(SignSessionRow {
            account_id,
            token_hash,
            validity,
        })
        .exec(&mut db)
        .await?;
        Ok(SignSessionId::from(session.id))
    }

    /// Resolves an unexpired session with a matching token hash to its account.
    pub async fn authenticate(
        &self,
        id: SignSessionId,
        token_hash: &[u8],
        authenticated_at: Timestamp,
    ) -> toasty::Result<Option<AccountId>> {
        let mut db = self.db.clone();
        let id = u32::from(id);
        let token_hash = token_hash.to_vec();
        let session = toasty::query!(
            SignSessionRow FILTER
                .id == #id
                AND .token_hash == #token_hash
        )
        .first()
        .exec(&mut db)
        .await?;

        // Toasty 0.10 cannot serialize SQLite predicates over Timestamp fields.
        Ok(session
            .filter(|session| TimeRange::from(session.validity).contains(authenticated_at))
            .map(|session| AccountId::from(session.account_id)))
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;

    use super::*;
    use crate::AccountRepository;

    #[tokio::test]
    async fn authenticates_only_a_matching_unexpired_session() {
        let db = crate::test_database().await;
        let account = AccountRepository::new(&db)
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let repository = SignSessionRepository::new(&db);
        let starts_at = Timestamp::new(1_800_000_000, 0).unwrap();
        let expires_at = starts_at + SignedDuration::from_mins(5);
        let token_hash = vec![7; 32];
        let session_id = repository
            .create(
                &account,
                token_hash.clone(),
                TimeRange::new(starts_at, expires_at),
            )
            .await
            .unwrap();

        assert_eq!(
            repository
                .authenticate(session_id, &token_hash, starts_at)
                .await
                .unwrap(),
            Some(account.id)
        );
        assert_eq!(
            repository
                .authenticate(session_id, &token_hash, expires_at)
                .await
                .unwrap(),
            Some(account.id)
        );
        assert_eq!(
            repository
                .authenticate(session_id, &[8; 32], starts_at)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            repository
                .authenticate(
                    session_id,
                    &token_hash,
                    expires_at + SignedDuration::from_nanos(1),
                )
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            repository
                .authenticate(SignSessionId::from(u32::MAX), &token_hash, starts_at)
                .await
                .unwrap(),
            None
        );
    }
}
