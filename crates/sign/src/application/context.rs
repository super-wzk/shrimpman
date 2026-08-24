use std::sync::Arc;

use jiff::SignedDuration;
#[cfg(test)]
use shrimpman_discovery::selector::RoundRobinSelector;
use shrimpman_discovery::{client::DiscoveryClient, selector::Selector};
use shrimpman_persistence::{
    AccountRepository, CharacterRepository, MezeportaFestivalRepository, SignInNoticeRepository,
    SignSessionRepository,
};

/// Dependencies shared by every Sign connection.
pub struct SignServiceContext {
    auto_sign_up: bool,
    session_ttl: SignedDuration,
    discovery: DiscoveryClient,
    entrance_selector: Box<dyn Selector>,
    repositories: SignRepositories,
}

/// Persistent repositories used by Sign application services.
pub struct SignRepositories {
    accounts: AccountRepository,
    characters: CharacterRepository,
    mezeporta_festivals: MezeportaFestivalRepository,
    sign_sessions: SignSessionRepository,
    sign_in_notices: SignInNoticeRepository,
}

impl SignRepositories {
    pub fn new(
        accounts: AccountRepository,
        characters: CharacterRepository,
        mezeporta_festivals: MezeportaFestivalRepository,
        sign_sessions: SignSessionRepository,
        sign_in_notices: SignInNoticeRepository,
    ) -> Self {
        Self {
            accounts,
            characters,
            mezeporta_festivals,
            sign_sessions,
            sign_in_notices,
        }
    }
}

impl SignServiceContext {
    /// Creates a service context from explicitly assembled dependencies.
    pub fn new(
        auto_sign_up: bool,
        session_ttl: SignedDuration,
        discovery: DiscoveryClient,
        entrance_selector: impl Selector + 'static,
        repositories: SignRepositories,
    ) -> Self {
        Self {
            auto_sign_up,
            session_ttl,
            discovery,
            entrance_selector: Box::new(entrance_selector),
            repositories,
        }
    }

    pub(crate) const fn auto_sign_up(&self) -> bool {
        self.auto_sign_up
    }

    pub(crate) const fn session_ttl(&self) -> SignedDuration {
        self.session_ttl
    }

    pub(crate) const fn discovery(&self) -> &DiscoveryClient {
        &self.discovery
    }

    pub(crate) fn entrance_selector(&self) -> &dyn Selector {
        self.entrance_selector.as_ref()
    }

    pub(crate) fn accounts(&self) -> &AccountRepository {
        &self.repositories.accounts
    }

    pub(crate) fn characters(&self) -> &CharacterRepository {
        &self.repositories.characters
    }

    pub(crate) fn mezeporta_festivals(&self) -> &MezeportaFestivalRepository {
        &self.repositories.mezeporta_festivals
    }

    pub(crate) fn sign_sessions(&self) -> &SignSessionRepository {
        &self.repositories.sign_sessions
    }

    pub(crate) fn sign_in_notices(&self) -> &SignInNoticeRepository {
        &self.repositories.sign_in_notices
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
            DiscoveryClient::connect(Default::default()).unwrap(),
            RoundRobinSelector::new(),
            SignRepositories::new(
                AccountRepository::new(&db),
                CharacterRepository::new(&db),
                MezeportaFestivalRepository::new(&db),
                SignSessionRepository::new(&db),
                SignInNoticeRepository::new(&db),
            ),
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
