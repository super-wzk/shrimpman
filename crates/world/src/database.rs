use shrimpman_persistence::{CharacterRepository, SignSessionRepository};
use toasty::Db;

use crate::{WorldDatabaseConfig, WorldRepositories};

/// World-owned Toasty database handle.
pub struct WorldDatabase {
    db: Db,
}

impl WorldDatabase {
    /// Connects the database and registers the shared persistent model graph.
    pub async fn connect(config: &WorldDatabaseConfig) -> toasty::Result<Self> {
        let mut builder = Db::builder();
        builder.models(shrimpman_persistence::models());

        Ok(Self {
            db: builder.connect(&config.url).await?,
        })
    }

    /// Creates the repositories used by World application services.
    pub fn repositories(&self) -> WorldRepositories {
        WorldRepositories::new(
            CharacterRepository::new(&self.db),
            SignSessionRepository::new(&self.db),
        )
    }
}
