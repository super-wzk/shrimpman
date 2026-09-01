use jiff::Timestamp;
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::{inbound::DeleteCharacter, model};
use crate::{InternalError, SignServiceContext, SignSessionContext};

const DELETE_SUCCESS: u8 = 1;

pub(super) struct DeleteCharacterHandler;

impl Handler<SignSessionContext, BinrwOutboundSender> for DeleteCharacterHandler {
    type Inbound = DeleteCharacter;
    type Error = InternalError;

    async fn handle(
        &self,
        context: SignSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let outcome = delete_character(
            context.service_context(),
            model::Request {
                session_token: inbound.session_token,
                character_id: inbound.character_id,
                session_id: inbound.session_id,
            },
            Timestamp::now(),
        )
        .await?;
        if matches!(outcome, model::Outcome::Deleted) {
            outbound.send(DELETE_SUCCESS).await?;
        }
        Ok(())
    }
}

pub(crate) async fn delete_character(
    service: &SignServiceContext,
    request: model::Request,
    now: Timestamp,
) -> Result<model::Outcome, InternalError> {
    let model::Request {
        session_token,
        character_id,
        session_id,
    } = request;
    let Some(account_id) = service
        .sign_sessions()
        .authenticate(session_id, &session_token, now)
        .await?
    else {
        tracing::info!(
            ?session_id,
            ?character_id,
            "Rejected character deletion with an invalid session"
        );
        return Ok(model::Outcome::InvalidSession);
    };

    let deleted = service
        .characters()
        .delete_for_account(account_id, character_id, now)
        .await?;
    if deleted {
        tracing::info!(?account_id, ?character_id, "Deleted character");
        Ok(model::Outcome::Deleted)
    } else {
        tracing::info!(
            ?account_id,
            ?character_id,
            "Character deletion did not match an active owned character"
        );
        Ok(model::Outcome::NotFound)
    }
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;
    use shrimpman_domain::{TimeRange, account::AccountId, session::SIGN_SESSION_TOKEN_LEN};

    use super::*;
    use crate::SignServiceContext;

    const TOKEN: [u8; SIGN_SESSION_TOKEN_LEN] = *b"0123456789ABCDEF";

    struct DeleteFixture {
        service: SignServiceContext,
        account_id: AccountId,
        request: model::Request,
        now: Timestamp,
    }

    async fn fixture() -> DeleteFixture {
        let service = SignServiceContext::for_test(false).await;
        let account = service
            .accounts()
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let character = service.characters().create_new(account.id).await.unwrap();
        let now = Timestamp::new(1_800_000_000, 0).unwrap();
        let session_id = service
            .sign_sessions()
            .create(
                &account,
                &TOKEN,
                TimeRange::from_duration(now, SignedDuration::from_mins(5)),
            )
            .await
            .unwrap();

        DeleteFixture {
            service,
            account_id: account.id,
            request: model::Request {
                session_token: TOKEN,
                character_id: character.id,
                session_id,
            },
            now,
        }
    }

    #[tokio::test]
    async fn deletes_a_character_owned_by_the_authenticated_account() {
        let fixture = fixture().await;

        let outcome = delete_character(&fixture.service, fixture.request, fixture.now)
            .await
            .unwrap();

        assert!(matches!(outcome, model::Outcome::Deleted));
        assert!(
            fixture
                .service
                .characters()
                .list_active(fixture.account_id)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_session_token() {
        let mut fixture = fixture().await;
        fixture.request.session_token = [b'x'; SIGN_SESSION_TOKEN_LEN];

        let outcome = delete_character(&fixture.service, fixture.request, fixture.now)
            .await
            .unwrap();

        assert!(matches!(outcome, model::Outcome::InvalidSession));
        assert_eq!(
            fixture
                .service
                .characters()
                .list_active(fixture.account_id)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
