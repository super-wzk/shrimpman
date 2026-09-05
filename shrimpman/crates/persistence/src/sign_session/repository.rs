use jiff::Timestamp;
use sha2::{Digest, Sha256};
use shrimpman_domain::{
    TimeRange,
    account::{Account, AccountId},
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use toasty::Db;

use super::SignSessionRow;

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
        token: &[u8; SIGN_SESSION_TOKEN_LEN],
        validity: TimeRange,
    ) -> toasty::Result<SignSessionId> {
        let mut db = self.db.clone();
        let account_id = u32::from(account.id);
        let token_hash = hash_token(token);
        let starts_at = validity.starts_at();
        let expires_at = validity.expires_at();
        let session = toasty::create!(SignSessionRow {
            account_id,
            token_hash,
            starts_at,
            expires_at,
        })
        .exec(&mut db)
        .await?;
        Ok(SignSessionId::from(session.id))
    }

    /// Resolves an unexpired session with a matching token to its account.
    pub async fn authenticate(
        &self,
        id: SignSessionId,
        token: &[u8; SIGN_SESSION_TOKEN_LEN],
        authenticated_at: Timestamp,
    ) -> toasty::Result<Option<AccountId>> {
        let mut db = self.db.clone();
        let id = u32::from(id);
        let token_hash = hash_token(token);
        let session = toasty::query!(
            SignSessionRow FILTER
                .id == #id
                AND .token_hash == #token_hash
                AND .starts_at <= #authenticated_at
                AND .expires_at >= #authenticated_at
        )
        .first()
        .exec(&mut db)
        .await?;

        Ok(session.map(|session| AccountId::from(session.account_id)))
    }
}

fn hash_token(token: &[u8; SIGN_SESSION_TOKEN_LEN]) -> Vec<u8> {
    Sha256::digest(token).to_vec()
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
        let token = b"0123456789ABCDEF";
        let invalid_token = b"FEDCBA9876543210";
        let session_id = repository
            .create(&account, token, TimeRange::new(starts_at, expires_at))
            .await
            .unwrap();

        assert_eq!(
            repository
                .authenticate(session_id, token, starts_at)
                .await
                .unwrap(),
            Some(account.id)
        );
        assert_eq!(
            repository
                .authenticate(session_id, token, expires_at)
                .await
                .unwrap(),
            Some(account.id)
        );
        assert_eq!(
            repository
                .authenticate(session_id, invalid_token, starts_at)
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            repository
                .authenticate(
                    session_id,
                    token,
                    expires_at + SignedDuration::from_nanos(1),
                )
                .await
                .unwrap(),
            None
        );
        assert_eq!(
            repository
                .authenticate(SignSessionId::from(u32::MAX), token, starts_at)
                .await
                .unwrap(),
            None
        );
    }
}
