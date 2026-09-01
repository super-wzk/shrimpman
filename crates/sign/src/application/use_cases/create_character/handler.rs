use jiff::Timestamp;
use shrimpman_domain::character::Character;

use super::model;
use crate::{InternalError, SignServiceContext};

pub(crate) async fn create_character(
    service: &SignServiceContext,
    request: model::Request,
    now: Timestamp,
) -> Result<model::Outcome, InternalError> {
    let Some(account_id) = service
        .sign_sessions()
        .authenticate(request.session_id, &request.session_token, now)
        .await?
    else {
        tracing::info!(
            session_id = ?request.session_id,
            "Rejected character creation with an invalid session"
        );
        return Ok(model::Outcome::InvalidSession);
    };

    let characters = service.characters().list_active(account_id).await?;
    if let Some(character) = characters.into_iter().find(Character::is_new) {
        tracing::info!(
            ?account_id,
            "Rejected character creation with a pending character"
        );
        return Ok(model::Outcome::PendingCharacterExists(character));
    }

    let character = service.characters().create_new(account_id).await?;
    tracing::info!(?account_id, character_id = ?character.id, "Created character");

    Ok(model::Outcome::Created(character))
}

#[cfg(test)]
mod tests {
    use jiff::SignedDuration;
    use shrimpman_domain::{TimeRange, session::SIGN_SESSION_TOKEN_LEN};

    use super::*;
    use crate::SignServiceContext;

    const TOKEN: [u8; SIGN_SESSION_TOKEN_LEN] = *b"0123456789ABCDEF";

    #[tokio::test]
    async fn creates_only_one_pending_character() {
        let (service, request, now) = fixture().await;
        let session_id = request.session_id;

        let first = create_character(&service, request, now).await.unwrap();
        assert!(matches!(first, model::Outcome::Created(_)));

        let second = create_character(
            &service,
            model::Request {
                session_token: TOKEN,
                session_id,
            },
            now,
        )
        .await
        .unwrap();

        assert!(matches!(second, model::Outcome::PendingCharacterExists(_)));
    }

    #[tokio::test]
    async fn rejects_an_invalid_session() {
        let (service, mut request, now) = fixture().await;
        request.session_token = [b'x'; SIGN_SESSION_TOKEN_LEN];

        let outcome = create_character(&service, request, now).await.unwrap();

        assert!(matches!(outcome, model::Outcome::InvalidSession));
    }

    async fn fixture() -> (SignServiceContext, model::Request, Timestamp) {
        let service = SignServiceContext::for_test(false).await;
        let account = service
            .accounts()
            .create("alice".to_owned(), "hash".to_owned())
            .await
            .unwrap();
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

        (
            service,
            model::Request {
                session_token: TOKEN,
                session_id,
            },
            now,
        )
    }
}
