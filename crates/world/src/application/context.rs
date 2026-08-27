use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard, OnceLock, Weak},
};

use shrimpman_domain::{account::AccountId, character::CharacterId, world::LandKey};
use shrimpman_persistence::{CharacterRepository, SignSessionRepository};
use shrimpman_protocol::BinrwOutboundSender;
use tokio_util::sync::CancellationToken;

use crate::{
    exchange::{ExchangeState, LandExchange},
    response::WireResponse,
};

/// Persistent repositories used by World application services.
pub struct WorldRepositories {
    characters: CharacterRepository,
    sign_sessions: SignSessionRepository,
}

impl WorldRepositories {
    pub(crate) fn new(
        characters: CharacterRepository,
        sign_sessions: SignSessionRepository,
    ) -> Self {
        Self {
            characters,
            sign_sessions,
        }
    }
}

/// Dependencies and live sessions shared by every World connection.
pub(super) struct WorldServiceContext {
    repositories: WorldRepositories,
    sessions: Mutex<HashMap<CharacterId, Weak<WorldSessionState>>>,
}

impl WorldServiceContext {
    pub(super) fn new(repositories: WorldRepositories) -> Self {
        Self {
            repositories,
            sessions: Mutex::new(HashMap::new()),
        }
    }

    pub(super) fn session(&self, character_id: CharacterId) -> Option<WorldSession> {
        let mut sessions = self.lock_sessions();
        let Some(state) = sessions.get(&character_id)?.upgrade() else {
            sessions.remove(&character_id);
            return None;
        };
        if state.cancellation.is_cancelled() {
            return None;
        }
        let identity = state.identity.get()?;

        Some(WorldSession {
            account_id: identity.account_id,
            character_id: identity.character_id,
            land: state.land.clone(),
            cancellation: state.cancellation.clone(),
        })
    }

    fn bind_character(
        &self,
        state: &Arc<WorldSessionState>,
        account_id: AccountId,
        character_id: CharacterId,
    ) -> Result<(), SessionBindError> {
        if state.identity.get().is_some() {
            return Err(SessionBindError::ConnectionAlreadyBound);
        }

        let mut sessions = self.lock_sessions();
        if sessions
            .get(&character_id)
            .and_then(Weak::upgrade)
            .is_some_and(|existing| !existing.cancellation.is_cancelled())
        {
            return Err(SessionBindError::CharacterOnline);
        }

        state
            .identity
            .set(AuthenticatedWorldSession {
                account_id,
                character_id,
            })
            .map_err(|_| SessionBindError::ConnectionAlreadyBound)?;
        sessions.insert(character_id, Arc::downgrade(state));
        Ok(())
    }

    fn disconnect(&self, state: &Arc<WorldSessionState>) {
        state.cancellation.cancel();
        let Some(identity) = state.identity.get() else {
            return;
        };

        let state = Arc::downgrade(state);
        let mut sessions = self.lock_sessions();
        if sessions
            .get(&identity.character_id)
            .is_some_and(|registered| registered.ptr_eq(&state))
        {
            sessions.remove(&identity.character_id);
        }
    }

    fn lock_sessions(&self) -> MutexGuard<'_, HashMap<CharacterId, Weak<WorldSessionState>>> {
        self.sessions
            .lock()
            .expect("World session registry poisoned")
    }
}

/// Authenticated identity and Land associated with one live connection.
pub struct WorldSession {
    account_id: AccountId,
    character_id: CharacterId,
    land: LandKey,
    cancellation: CancellationToken,
}

impl WorldSession {
    pub const fn account_id(&self) -> AccountId {
        self.account_id
    }

    pub const fn character_id(&self) -> CharacterId {
        self.character_id
    }

    pub const fn land(&self) -> &LandKey {
        &self.land
    }

    /// Requests this connection to close.
    pub fn disconnect(&self) {
        self.cancellation.cancel();
    }
}

/// State and dependencies belonging to one World connection.
#[derive(Clone)]
pub(crate) struct WorldSessionContext {
    service: Arc<WorldServiceContext>,
    state: Arc<WorldSessionState>,
    exchange_state: Arc<ExchangeState>,
}

impl WorldSessionContext {
    pub(super) fn new(service: Arc<WorldServiceContext>, land: LandKey) -> Self {
        Self {
            service,
            state: Arc::new(WorldSessionState {
                land,
                identity: OnceLock::new(),
                cancellation: CancellationToken::new(),
            }),
            exchange_state: Arc::new(ExchangeState::default()),
        }
    }

    pub(super) fn characters(&self) -> &CharacterRepository {
        &self.service.repositories.characters
    }

    pub(super) fn sign_sessions(&self) -> &SignSessionRepository {
        &self.service.repositories.sign_sessions
    }

    pub(super) fn land(&self) -> &LandKey {
        &self.state.land
    }

    pub(super) fn bind_character(
        &self,
        account_id: AccountId,
        character_id: CharacterId,
    ) -> Result<(), SessionBindError> {
        self.service
            .bind_character(&self.state, account_id, character_id)
    }

    pub(super) fn disconnect(&self) {
        self.state.cancellation.cancel();
    }

    pub(crate) fn exchange(&self, sender: BinrwOutboundSender) -> LandExchange {
        LandExchange::new(
            sender,
            Arc::clone(&self.exchange_state),
            self.state.cancellation.clone(),
        )
    }

    pub(super) fn complete_response(&self, response: WireResponse) -> bool {
        self.exchange_state.complete(response)
    }

    pub(super) async fn cancelled(&self) {
        self.state.cancellation.cancelled().await;
    }

    pub(super) fn guard(&self) -> WorldSessionGuard {
        WorldSessionGuard {
            service: Arc::clone(&self.service),
            state: Arc::clone(&self.state),
        }
    }
}

struct WorldSessionState {
    land: LandKey,
    identity: OnceLock<AuthenticatedWorldSession>,
    cancellation: CancellationToken,
}

struct AuthenticatedWorldSession {
    account_id: AccountId,
    character_id: CharacterId,
}

#[derive(Debug)]
pub(super) enum SessionBindError {
    ConnectionAlreadyBound,
    CharacterOnline,
}

pub(super) struct WorldSessionGuard {
    service: Arc<WorldServiceContext>,
    state: Arc<WorldSessionState>,
}

impl Drop for WorldSessionGuard {
    fn drop(&mut self) {
        self.service.disconnect(&self.state);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn indexes_only_one_live_session_per_character() {
        let db = crate::test_database().await;
        let service = Arc::new(WorldServiceContext::new(crate::test_repositories(&db)));
        let account_id = AccountId::from(1);
        let character_id = CharacterId::from(2);
        let first =
            WorldSessionContext::new(Arc::clone(&service), LandKey::from("first".to_owned()));
        let second =
            WorldSessionContext::new(Arc::clone(&service), LandKey::from("second".to_owned()));
        let first_guard = first.guard();
        let second_guard = second.guard();

        first.bind_character(account_id, character_id).unwrap();
        assert!(matches!(
            second.bind_character(account_id, character_id),
            Err(SessionBindError::CharacterOnline)
        ));
        assert_eq!(service.session(character_id).unwrap().land(), first.land());

        drop(first_guard);
        second.bind_character(account_id, character_id).unwrap();
        assert_eq!(service.session(character_id).unwrap().land(), second.land());

        drop(second_guard);
        assert!(service.session(character_id).is_none());
    }
}
