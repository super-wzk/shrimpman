use shrimpman_persistence::{
    AccountRepository, CharacterRepository, MezeportaFestivalRepository, SignSessionRepository,
};
use toasty::Db;

use crate::SignDatabaseConfig;

/// Sign-owned Toasty database handle.
#[derive(Debug, Clone)]
pub struct SignDatabase {
    db: Db,
}

impl SignDatabase {
    /// Connects the database and registers the shared persistent model graph.
    pub async fn connect(config: &SignDatabaseConfig) -> toasty::Result<Self> {
        let mut builder = Db::builder();
        builder.models(shrimpman_persistence::models());

        Ok(Self {
            db: builder.connect(&config.url).await?,
        })
    }

    /// Creates the account repository used by Sign.
    pub fn account_repository(&self) -> AccountRepository {
        AccountRepository::new(&self.db)
    }

    /// Creates the character repository used by Sign.
    pub fn character_repository(&self) -> CharacterRepository {
        CharacterRepository::new(&self.db)
    }

    /// Creates the Mezeporta Festival repository used by Sign.
    pub fn mezeporta_festival_repository(&self) -> MezeportaFestivalRepository {
        MezeportaFestivalRepository::new(&self.db)
    }

    /// Creates the Sign-session repository used by Sign.
    pub fn sign_session_repository(&self) -> SignSessionRepository {
        SignSessionRepository::new(&self.db)
    }
}
