use jiff::Timestamp;
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::inbound::DeleteCharacter;
use crate::{
    InternalError, SignSessionContext,
    application::session_token::hash_session_token,
};

const DELETE_SUCCESS: u8 = 1;

pub(super) struct DeleteCharacterHandler;

#[async_trait::async_trait]
impl Handler<SignSessionContext, BinrwOutboundSender> for DeleteCharacterHandler {
    type Inbound = DeleteCharacter;
    type Error = InternalError;

    async fn handle(
        &self,
        context: SignSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        if delete_character(context, inbound, Timestamp::now()).await? {
            outbound.send(DELETE_SUCCESS).await?;
        }
        Ok(())
    }
}

async fn delete_character(
    context: SignSessionContext,
    inbound: DeleteCharacter,
    now: Timestamp,
) -> Result<bool, InternalError> {
    let service = context.service_context();
    let DeleteCharacter {
        session_token,
        character_id,
        session_id,
    } = inbound;
    let token_hash = hash_session_token(&session_token);
    let Some(account_id) = service
        .sign_sessions()
        .authenticate(session_id, &token_hash, now)
        .await?
    else {
        return Ok(false);
    };

    Ok(service
        .characters()
        .delete_for_account(account_id, character_id, now)
        .await?)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use jiff::SignedDuration;
    use shrimpman_domain::{TimeRange, account::Account};

    use super::*;
    use crate::SignServiceContext;

    const TOKEN: &[u8; 16] = b"0123456789ABCDEF";

    struct DeleteFixture {
        service: Arc<SignServiceContext>,
        account: Account,
        request: DeleteCharacter,
        now: Timestamp,
    }

    async fn fixture() -> DeleteFixture {
        let service = Arc::new(SignServiceContext::for_test(false).await);
        let account = service
            .accounts()
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
        let character = service.characters().create_new(&account).await.unwrap();
        let now = Timestamp::new(1_800_000_000, 0).unwrap();
        let session_id = service
            .sign_sessions()
            .create(
                &account,
                hash_session_token(TOKEN),
                TimeRange::from_duration(now, SignedDuration::from_mins(5)),
            )
            .await
            .unwrap();

        DeleteFixture {
            service,
            account,
            request: DeleteCharacter {
                session_token: TOKEN.to_vec(),
                character_id: character.id,
                session_id,
            },
            now,
        }
    }

    #[tokio::test]
    async fn deletes_a_character_owned_by_the_authenticated_account() {
        let fixture = fixture().await;

        let deleted = delete_character(
            SignSessionContext::new(Arc::clone(&fixture.service)),
            fixture.request,
            fixture.now,
        )
        .await
        .unwrap();

        assert!(deleted);
        assert!(
            fixture
                .service
                .characters()
                .list_active(&fixture.account)
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_session_token() {
        let mut fixture = fixture().await;
        fixture.request.session_token = b"wrong".to_vec();

        let deleted = delete_character(
            SignSessionContext::new(Arc::clone(&fixture.service)),
            fixture.request,
            fixture.now,
        )
        .await
        .unwrap();

        assert!(!deleted);
        assert_eq!(
            fixture
                .service
                .characters()
                .list_active(&fixture.account)
                .await
                .unwrap()
                .len(),
            1
        );
    }
}
