use axum::{
    Json,
    extract::{
        Path, State,
        rejection::{JsonRejection, PathRejection},
    },
    http::StatusCode,
};
use serde::Deserialize;
use shrimpman_domain::{character::CharacterId, session::SignSessionId};

use super::error_response;
use crate::{SignService, application::use_cases::delete_character::model};

pub(super) async fn handle(
    character_id: Result<Path<u32>, PathRejection>,
    State(service): State<SignService>,
    request: Result<Json<RequestBody>, JsonRejection>,
) -> Result<StatusCode, axum::response::Response> {
    let Path(character_id) = character_id.map_err(|error| {
        tracing::info!(%error, "Rejected malformed HTTP character deletion path");
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    })?;
    let Json(request) = request.map_err(|error| {
        tracing::info!(%error, "Rejected malformed HTTP character deletion request");
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    })?;
    let session_token = request
        .session_token
        .into_bytes()
        .try_into()
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid_request"))?;
    let outcome = service
        .delete_character(model::Request {
            session_token,
            character_id: CharacterId::from(character_id),
            session_id: SignSessionId::from(request.session_id),
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "HTTP character deletion failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        })?;

    match outcome {
        model::Outcome::Deleted => Ok(StatusCode::NO_CONTENT),
        model::Outcome::InvalidSession => {
            Err(error_response(StatusCode::UNAUTHORIZED, "invalid_session"))
        }
        model::Outcome::NotFound => {
            Err(error_response(StatusCode::NOT_FOUND, "character_not_found"))
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RequestBody {
    session_id: u32,
    session_token: String,
}

#[cfg(test)]
mod tests {
    use axum::{
        Router,
        body::{Body, to_bytes},
        http::{Request, header},
        response::Response,
    };
    use serde_json::{Value, json};
    use tower::ServiceExt;

    use super::*;
    use crate::{
        SignServiceContext,
        application::use_cases::{create_character, password_sign_in},
    };

    struct Fixture {
        router: Router,
        character_id: u32,
        session_id: u32,
        session_token: String,
    }

    #[tokio::test]
    async fn deletes_an_owned_character() {
        let fixture = fixture().await;
        let request = delete_request(
            fixture.character_id,
            fixture.session_id,
            &fixture.session_token,
        );
        let response = fixture.router.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        assert!(
            to_bytes(response.into_body(), usize::MAX)
                .await
                .unwrap()
                .is_empty()
        );

        let request = delete_request(
            fixture.character_id,
            fixture.session_id,
            &fixture.session_token,
        );
        let response = fixture.router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "character_not_found" })
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_session() {
        let service = SignService::new(SignServiceContext::for_test(false).await).unwrap();
        let response = super::super::router(service)
            .oneshot(delete_request(1, 1, "0123456789ABCDEF"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "invalid_session" })
        );
    }

    #[tokio::test]
    async fn rejects_a_session_token_with_the_wrong_length() {
        let service = SignService::new(SignServiceContext::for_test(false).await).unwrap();
        let response = super::super::router(service)
            .oneshot(delete_request(1, 1, "short"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "invalid_request" })
        );
    }

    async fn fixture() -> Fixture {
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
        let outcome = service
            .password_sign_in(password_sign_in::model::Request {
                username: "alice".to_owned(),
                password: "secret".to_owned(),
            })
            .await
            .unwrap();
        let password_sign_in::model::Outcome::Success(success) = outcome else {
            panic!("password sign-in should succeed")
        };
        let session_id = success.session.id;
        let session_token = success.session.token;
        let outcome = service
            .create_character(create_character::model::Request {
                session_token,
                session_id,
            })
            .await
            .unwrap();
        let create_character::model::Outcome::Created(character) = outcome else {
            panic!("character creation should succeed")
        };

        Fixture {
            character_id: character.id.into(),
            session_id: session_id.into(),
            session_token: String::from_utf8(session_token.to_vec()).unwrap(),
            router: super::super::router(service),
        }
    }

    fn delete_request(character_id: u32, session_id: u32, session_token: &str) -> Request<Body> {
        Request::delete(format!("/characters/{character_id}"))
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(
                json!({
                    "session_id": session_id,
                    "session_token": session_token,
                })
                .to_string(),
            ))
            .unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
