use jiff::Timestamp;
use rand::{RngExt, distr::Alphanumeric};
use sha2::{Digest, Sha256};
use shrimpman_domain::{TimeRange, character::Character};
use shrimpman_protocol::Handler;

use super::{
    SESSION_TOKEN_LEN,
    inbound::PasswordSignIn,
    outbound::{IssuedSignSession, PasswordSignInResponse, SignInSuccess},
};
use crate::{InternalError, SignSessionContext};

const BCRYPT_MAX_PASSWORD_LEN: usize = 71;

pub(super) struct PasswordSignInHandler;

#[async_trait::async_trait]
impl Handler<SignSessionContext> for PasswordSignInHandler {
    type Inbound = PasswordSignIn;
    type Outbound = PasswordSignInResponse;
    type Error = InternalError;

    async fn handle(
        &self,
        context: SignSessionContext,
        inbound: Self::Inbound,
    ) -> Result<Vec<Self::Outbound>, Self::Error> {
        let service = context.service_context();
        let (username, request_new_character) = match inbound.username.strip_suffix('+') {
            Some(username) => (username, true),
            None => (inbound.username.as_str(), false),
        };

        if username.is_empty() || inbound.password.len() > BCRYPT_MAX_PASSWORD_LEN {
            return Ok(vec![PasswordSignInResponse::IllegalInput]);
        }

        let now = Timestamp::now();
        let account = match service.accounts().find_by_username(username).await? {
            Some(account) => {
                if !verify_password(inbound.password, account.password_hash.clone()).await? {
                    return Ok(vec![PasswordSignInResponse::WrongPassword]);
                }
                account
            }
            None => {
                let password_hash = hash_password(inbound.password).await?;
                service
                    .accounts()
                    .create(username.to_owned(), password_hash)
                    .await?
            }
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
        )?;

        Ok(vec![PasswordSignInResponse::Success(response)])
    }
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
        bcrypt::non_truncating_hash(password, bcrypt::DEFAULT_COST)
    })
    .await??)
}

async fn verify_password(password: String, password_hash: String) -> Result<bool, InternalError> {
    Ok(tokio::task::spawn_blocking(move || {
        bcrypt::non_truncating_verify(password, &password_hash)
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
    async fn rejects_a_password_exceeding_the_bcrypt_limit() {
        let response = PasswordSignInHandler
            .handle(
                SignSessionContext::new(Arc::new(SignServiceContext::for_test().await)),
                PasswordSignIn {
                    username: "alice".to_owned(),
                    password: "a".repeat(BCRYPT_MAX_PASSWORD_LEN + 1),
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            response.as_slice(),
            [PasswordSignInResponse::IllegalInput]
        ));
    }

    #[tokio::test]
    async fn creates_an_account_reuses_its_character_and_rejects_a_wrong_password() {
        let service = Arc::new(SignServiceContext::for_test().await);
        let first = PasswordSignInHandler
            .handle(
                SignSessionContext::new(Arc::clone(&service)),
                PasswordSignIn {
                    username: "alice".to_owned(),
                    password: "secret".to_owned(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            first.as_slice(),
            [PasswordSignInResponse::Success(_)]
        ));
        let account = service
            .accounts()
            .find_by_username("alice")
            .await
            .unwrap()
            .unwrap();
        assert_ne!(account.password_hash, "secret");
        assert_eq!(
            account.rights.bits(),
            CourseRights::HUNTER_LIFE
                .union(CourseRights::EXTRA_A)
                .bits()
        );
        let characters = service.characters().list_active(&account).await.unwrap();
        assert_eq!(characters.len(), 1);

        let second = PasswordSignInHandler
            .handle(
                SignSessionContext::new(Arc::clone(&service)),
                PasswordSignIn {
                    username: "alice".to_owned(),
                    password: "secret".to_owned(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            second.as_slice(),
            [PasswordSignInResponse::Success(_)]
        ));
        let characters = service.characters().list_active(&account).await.unwrap();
        assert_eq!(characters.len(), 1);

        let wrong_password = PasswordSignInHandler
            .handle(
                SignSessionContext::new(service),
                PasswordSignIn {
                    username: "alice".to_owned(),
                    password: "wrong".to_owned(),
                },
            )
            .await
            .unwrap();

        assert!(matches!(
            wrong_password.as_slice(),
            [PasswordSignInResponse::WrongPassword]
        ));
    }
}
