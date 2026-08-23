use std::sync::Arc;

use jiff::SignedDuration;
use shrimpman_persistence::{
    AccountRepository, CharacterRepository, MezeportaFestivalRepository, SignSessionRepository,
};

/// Dependencies shared by every Sign connection.
pub struct SignServiceContext {
    auto_sign_up: bool,
    session_ttl: SignedDuration,
    accounts: AccountRepository,
    characters: CharacterRepository,
    mezeporta_festivals: MezeportaFestivalRepository,
    sign_sessions: SignSessionRepository,
}

impl SignServiceContext {
    /// Creates a service context from explicitly assembled dependencies.
    pub fn new(
        auto_sign_up: bool,
        session_ttl: SignedDuration,
        accounts: AccountRepository,
        characters: CharacterRepository,
        mezeporta_festivals: MezeportaFestivalRepository,
        sign_sessions: SignSessionRepository,
    ) -> Self {
        Self {
            auto_sign_up,
            session_ttl,
            accounts,
            characters,
            mezeporta_festivals,
            sign_sessions,
        }
    }

    pub(crate) const fn auto_sign_up(&self) -> bool {
        self.auto_sign_up
    }

    pub(crate) const fn session_ttl(&self) -> SignedDuration {
        self.session_ttl
    }

    pub(crate) fn accounts(&self) -> &AccountRepository {
        &self.accounts
    }

    pub(crate) fn characters(&self) -> &CharacterRepository {
        &self.characters
    }

    pub(crate) fn mezeporta_festivals(&self) -> &MezeportaFestivalRepository {
        &self.mezeporta_festivals
    }

    pub(crate) fn sign_sessions(&self) -> &SignSessionRepository {
        &self.sign_sessions
    }

    #[cfg(test)]
    pub(crate) async fn for_test(auto_sign_up: bool) -> Self {
        let mut builder = toasty::Db::builder();
        builder.models(shrimpman_persistence::models());
        let db = builder.connect("sqlite::memory:").await.unwrap();
        db.push_schema().await.unwrap();

        Self::new(
            auto_sign_up,
            SignedDuration::from_mins(5),
            AccountRepository::new(&db),
            CharacterRepository::new(&db),
            MezeportaFestivalRepository::new(&db),
            SignSessionRepository::new(&db),
        )
    }
}

/// State and dependencies belonging to one Sign connection.
pub struct SignSessionContext {
    service: Arc<SignServiceContext>,
}

impl SignSessionContext {
    pub(crate) fn new(service: Arc<SignServiceContext>) -> Self {
        Self { service }
    }

    /// Returns the dependencies shared by the Sign service.
    pub fn service_context(&self) -> &SignServiceContext {
        &self.service
    }
}
