use shrimpman_domain::{TimeRange, account::Account, session::SignSessionId};
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
}
