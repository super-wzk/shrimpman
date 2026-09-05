use shrimpman_persistence::{
    AccountRepository, CharacterRepository, MezeportaFestaRepository, SignInNoticeRepository,
    SignSessionRepository,
};
use toasty::Db;

use crate::{SignDatabaseConfig, SignRepositories};

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

    /// Creates the repositories used by Sign application services.
    pub fn repositories(&self) -> SignRepositories {
        SignRepositories::new(
            AccountRepository::new(&self.db),
            CharacterRepository::new(&self.db),
            MezeportaFestaRepository::new(&self.db),
            SignSessionRepository::new(&self.db),
            SignInNoticeRepository::new(&self.db),
        )
    }
}
