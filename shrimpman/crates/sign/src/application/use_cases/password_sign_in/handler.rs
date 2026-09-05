use argon2::{
    Argon2,
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng},
};
use jiff::Timestamp;
use shrimpman_domain::TimeRange;
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::{inbound::PasswordSignIn, model, outbound::PasswordSignInResponse};
use crate::{
    InternalError, SignServiceContext, SignSessionContext,
    application::{
        service_names, session_token::generate_session_token, use_cases::create_character,
    },
};

const MAX_SIGN_IN_NOTICES: usize = 4;

pub(super) struct PasswordSignInHandler;

impl Handler<SignSessionContext, BinrwOutboundSender> for PasswordSignInHandler {
    type Inbound = PasswordSignIn;
    type Error = InternalError;

    async fn handle(
        &self,
        context: SignSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let (username, character_requested) = match inbound.username.strip_suffix('+') {
            Some(username) => (username.to_owned(), true),
            None => (inbound.username, false),
        };
        let mut response = password_sign_in(
            context.service_context(),
            model::Request {
                username,
                password: inbound.password,
            },
        )
        .await?;
        if let model::Outcome::Success(success) = &mut response {
            ensure_character(context.service_context(), success, character_requested).await?;
        }
        outbound
            .send(PasswordSignInResponse::try_from(response)?)
            .await?;
        Ok(())
    }
}

pub(crate) async fn password_sign_in(
    service: &SignServiceContext,
    request: model::Request,
) -> Result<model::Outcome, InternalError> {
    if request.username.is_empty() {
        tracing::info!("Rejected password sign-in with an empty username");
        return Ok(model::Outcome::IllegalInput);
    }

    let now = Timestamp::now();
    let account = match service
        .accounts()
        .find_by_username(&request.username)
        .await?
    {
        Some(account) => {
            if !verify_password(request.password, account.password_hash.clone()).await? {
                tracing::info!("Rejected password sign-in with invalid credentials");
                return Ok(model::Outcome::WrongPassword);
            }
            account
        }
        None if service.auto_sign_up() => {
            let password_hash = hash_password(request.password).await?;
            let account = service
                .accounts()
                .create(request.username, password_hash)
                .await?;
            tracing::info!(account_id = ?account.id, "Created account during sign-in");
            account
        }
        None => {
            tracing::info!("Rejected password sign-in with invalid credentials");
            return Ok(model::Outcome::WrongPassword);
        }
    };

    let rights = account.rights;
    let characters = service.characters().list_active(account.id).await?;

    let character_sign_in_history = service.characters().sign_in_history(&characters).await?;
    let entrance_instances = service.discovery().instances(&service_names::ENTRANCE);
    let entrance_servers = service
        .entrance_selector()
        .select(&entrance_instances)
        .and_then(|instance| instance.advertise_addr.clone())
        .into_iter()
        .collect::<Vec<_>>();
    if entrance_servers.is_empty() {
        tracing::warn!("No Entrance service instance is available for sign-in");
    }
    let notices = service
        .sign_in_notices()
        .list_active_at(now, MAX_SIGN_IN_NOTICES)
        .await?;
    let festa = service.mezeporta_festas().find_active_at(now).await?;
    let character_count = characters.len();
    let notice_count = notices.len();

    let token = generate_session_token();
    let session_id = service
        .sign_sessions()
        .create(
            &account,
            &token,
            TimeRange::from_duration(now, service.session_ttl()),
        )
        .await?;
    let return_period = service.accounts().record_sign_in(&account, now).await?;

    let last_character_id = character_sign_in_history.last_character_id();
    let characters = characters
        .into_iter()
        .map(|character| model::SignedInCharacter {
            last_sign_in_at: character_sign_in_history
                .last_sign_in_at(character.id)
                .unwrap_or(now),
            character,
        })
        .collect();
    let response = model::Success {
        session: model::IssuedSession {
            id: session_id,
            token,
            issued_at: now,
        },
        entrance_servers,
        characters,
        notices,
        last_character_id,
        rights,
        return_expires_at: return_period.expires_at(),
        festa,
    };

    tracing::info!(
        account_id = ?account.id,
        character_count,
        notice_count,
        "Password sign-in succeeded"
    );

    Ok(model::Outcome::Success(Box::new(response)))
}

async fn ensure_character(
    service: &SignServiceContext,
    success: &mut model::Success,
    requested: bool,
) -> Result<(), InternalError> {
    let has_pending_character = success
        .characters
        .iter()
        .any(|signed_in| signed_in.character.is_new());
    let should_create = success.characters.is_empty() || (requested && !has_pending_character);
    if !should_create {
        return Ok(());
    }

    let outcome = create_character::handler::create_character(
        service,
        create_character::model::Request {
            session_token: success.session.token,
            session_id: success.session.id,
        },
        success.session.issued_at,
    )
    .await?;
    let character = match outcome {
        create_character::model::Outcome::Created(character)
        | create_character::model::Outcome::PendingCharacterExists(character) => character,
        create_character::model::Outcome::InvalidSession => {
            return Err(InternalError::InvalidIssuedSession);
        }
    };
    success.characters.push(model::SignedInCharacter {
        character,
        last_sign_in_at: success.session.issued_at,
    });

    Ok(())
}

async fn hash_password(password: String) -> Result<String, InternalError> {
    Ok(tokio::task::spawn_blocking(move || {
        let salt = SaltString::generate(OsRng);
        Argon2::default()
            .hash_password(password.as_bytes(), &salt)
            .map(|hash| hash.to_string())
    })
    .await??)
}

async fn verify_password(password: String, password_hash: String) -> Result<bool, InternalError> {
    Ok(tokio::task::spawn_blocking(move || {
        let password_hash = PasswordHash::new(&password_hash)?;
        match Argon2::default().verify_password(password.as_bytes(), &password_hash) {
            Ok(()) => Ok(true),
            Err(argon2::password_hash::Error::Password) => Ok(false),
            Err(error) => Err(error),
        }
    })
    .await??)
}

#[cfg(test)]
mod tests {
    use shrimpman_domain::account::CourseRights;

    use super::*;
    use crate::SignServiceContext;

    #[tokio::test]
    async fn rejects_an_unknown_account_when_auto_sign_up_is_disabled() {
        let service = SignServiceContext::for_test(false).await;
        let response = password_sign_in(&service, request("secret")).await.unwrap();

        assert!(matches!(response, model::Outcome::WrongPassword));
        assert!(
            service
                .accounts()
                .find_by_username("alice")
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn creates_an_account_without_a_character_and_rejects_a_wrong_password() {
        let service = SignServiceContext::for_test(true).await;
        let password = "a".repeat(128);
        let first = password_sign_in(&service, request(password.clone()))
            .await
            .unwrap();

        assert!(matches!(first, model::Outcome::Success(_)));
        let account = service
            .accounts()
            .find_by_username("alice")
            .await
            .unwrap()
            .unwrap();
        assert!(account.password_hash.starts_with("$argon2id$v=19$"));
        assert_eq!(
            account.rights.bits(),
            CourseRights::HUNTER_LIFE
                .union(CourseRights::EXTRA_A)
                .bits()
        );
        let characters = service.characters().list_active(account.id).await.unwrap();
        assert!(characters.is_empty());

        let second = password_sign_in(&service, request(password)).await.unwrap();

        assert!(matches!(second, model::Outcome::Success(_)));
        let characters = service.characters().list_active(account.id).await.unwrap();
        assert!(characters.is_empty());

        let wrong_password = password_sign_in(&service, request("wrong")).await.unwrap();

        assert!(matches!(wrong_password, model::Outcome::WrongPassword));
    }

    #[tokio::test]
    async fn tcp_adapter_provisions_one_pending_character() {
        let service = SignServiceContext::for_test(true).await;
        let outcome = password_sign_in(&service, request("secret")).await.unwrap();
        let model::Outcome::Success(mut success) = outcome else {
            panic!("password sign-in should succeed")
        };

        ensure_character(&service, &mut success, false)
            .await
            .unwrap();
        assert_eq!(success.characters.len(), 1);
        assert!(success.characters[0].character.is_new());

        ensure_character(&service, &mut success, true)
            .await
            .unwrap();
        assert_eq!(success.characters.len(), 1);
    }

    fn request(password: impl Into<String>) -> model::Request {
        model::Request {
            username: "alice".to_owned(),
            password: password.into(),
        }
    }
}
