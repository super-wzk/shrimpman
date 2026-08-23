use argon2::{
    Argon2,
    password_hash::{
        PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng,
    },
};
use jiff::Timestamp;
use rand::{RngExt, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use shrimpman_domain::{TimeRange, character::Character};
use shrimpman_protocol::{BinrwOutboundSender, Handler};

use super::{
    SESSION_TOKEN_LEN,
    inbound::PasswordSignIn,
    outbound::{IssuedSignSession, PasswordSignInResponse, SignInSuccess},
};
use crate::{InternalError, SignSessionContext};

const MAX_SIGN_IN_NOTICES: usize = u8::MAX as usize;

pub(super) struct PasswordSignInHandler;

#[async_trait::async_trait]
impl Handler<SignSessionContext, BinrwOutboundSender> for PasswordSignInHandler {
    type Inbound = PasswordSignIn;
    type Error = InternalError;

    async fn handle(
        &self,
        context: SignSessionContext,
        inbound: Self::Inbound,
        outbound: BinrwOutboundSender,
    ) -> Result<(), Self::Error> {
        let response = password_sign_in(context, inbound).await?;
        outbound.send(response).await?;
        Ok(())
    }
}

async fn password_sign_in(
    context: SignSessionContext,
    inbound: PasswordSignIn,
) -> Result<PasswordSignInResponse, InternalError> {
    let service = context.service_context();
    let (username, request_new_character) = match inbound.username.strip_suffix('+') {
        Some(username) => (username, true),
        None => (inbound.username.as_str(), false),
    };

    if username.is_empty() {
        return Ok(PasswordSignInResponse::IllegalInput);
    }

    let now = Timestamp::now();
    let account = match service.accounts().find_by_username(username).await? {
        Some(account) => {
            if !verify_password(inbound.password, account.password_hash.clone()).await? {
                return Ok(PasswordSignInResponse::WrongPassword);
            }
            account
        }
        None if service.auto_sign_up() => {
            let password_hash = hash_password(inbound.password).await?;
            service
                .accounts()
                .create(username.to_owned(), password_hash)
                .await?
        }
        None => return Ok(PasswordSignInResponse::WrongPassword),
    };

    let rights = account.rights;
    let mut characters = service.characters().list_active(&account).await?;
    let should_create_character = characters.is_empty()
        || (request_new_character && !characters.iter().any(Character::is_new));

    if should_create_character {
        characters.push(service.characters().create_new(&account).await?);
    }

    let character_sign_in_history = service.characters().sign_in_history(&characters).await?;
    let festival = service.mezeporta_festivals().find_active_at(now).await?;
    let notices = service
        .sign_in_notices()
        .list_active_at(now, MAX_SIGN_IN_NOTICES)
        .await?;
    let token = generate_session_token();
    let session_id = service
        .sign_sessions()
        .create(
            &account,
            hash_session_token(&token),
            TimeRange::from_duration(now, service.session_ttl()),
        )
        .await?;
    let return_period = service.accounts().record_sign_in(&account, now).await?;
    let response = SignInSuccess::new(
        IssuedSignSession::new(session_id, token, now),
        rights,
        characters,
        &character_sign_in_history,
        return_period.expires_at(),
        festival,
        notices,
    )?;

    Ok(PasswordSignInResponse::Success(response))
}

fn generate_session_token() -> [u8; SESSION_TOKEN_LEN] {
    let mut rng = rand::rng();
    std::array::from_fn(|_| rng.sample(Alphanumeric))
}

fn hash_session_token(token: &[u8; SESSION_TOKEN_LEN]) -> Vec<u8> {
    Sha256::digest(token).to_vec()
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
    use std::sync::Arc;

    use shrimpman_domain::account::CourseRights;

    use super::*;
    use crate::SignServiceContext;

    #[test]
    fn generates_and_hashes_a_session_token() {
        let token = generate_session_token();

        assert!(token.iter().all(u8::is_ascii_alphanumeric));
        assert_eq!(hash_session_token(&token).len(), 32);
    }

    #[tokio::test]
    async fn rejects_an_unknown_account_when_auto_sign_up_is_disabled() {
        let service = Arc::new(SignServiceContext::for_test(false).await);
        let response = password_sign_in(
            SignSessionContext::new(Arc::clone(&service)),
            request("secret"),
        )
        .await
        .unwrap();

        assert!(matches!(response, PasswordSignInResponse::WrongPassword));
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
    async fn creates_an_account_reuses_its_character_and_rejects_a_wrong_password() {
        let service = Arc::new(SignServiceContext::for_test(true).await);
        let password = "a".repeat(128);
        let first = password_sign_in(
            SignSessionContext::new(Arc::clone(&service)),
            request(password.clone()),
        )
        .await
        .unwrap();

        assert!(matches!(first, PasswordSignInResponse::Success(_)));
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
        let characters = service.characters().list_active(&account).await.unwrap();
        assert_eq!(characters.len(), 1);

        let second = password_sign_in(
            SignSessionContext::new(Arc::clone(&service)),
            request(password),
        )
        .await
        .unwrap();

        assert!(matches!(second, PasswordSignInResponse::Success(_)));
        let characters = service.characters().list_active(&account).await.unwrap();
        assert_eq!(characters.len(), 1);

        let wrong_password = password_sign_in(
            SignSessionContext::new(service),
            request("wrong"),
        )
        .await
        .unwrap();

        assert!(matches!(
            wrong_password,
            PasswordSignInResponse::WrongPassword
        ));
    }

    fn request(password: impl Into<String>) -> PasswordSignIn {
        PasswordSignIn {
            username: "alice".to_owned(),
            password: password.into(),
        }
    }
}
