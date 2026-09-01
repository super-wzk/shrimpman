use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use serde::Deserialize;
use shrimpman_domain::session::SignSessionId;

use super::{character, error_response};
use crate::{SignService, application::use_cases::create_character::model};

pub(super) async fn handle(
    State(service): State<SignService>,
    request: Result<Json<RequestBody>, JsonRejection>,
) -> Result<(StatusCode, Json<character::ResponseBody>), axum::response::Response> {
    let Json(request) = request.map_err(|error| {
        tracing::info!(%error, "Rejected malformed HTTP character creation request");
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    })?;
    let session_token = request
        .session_token
        .into_bytes()
        .try_into()
        .map_err(|_| error_response(StatusCode::BAD_REQUEST, "invalid_request"))?;
    let outcome = service
        .create_character(model::Request {
            session_token,
            session_id: SignSessionId::from(request.session_id),
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "HTTP character creation failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        })?;

    match outcome {
        model::Outcome::Created(character) => Ok((
            StatusCode::CREATED,
            Json(character::ResponseBody::from(character)),
        )),
        model::Outcome::InvalidSession => {
            Err(error_response(StatusCode::UNAUTHORIZED, "invalid_session"))
        }
        model::Outcome::PendingCharacterExists(_) => Err(error_response(
            StatusCode::CONFLICT,
            "pending_character_exists",
        )),
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
    use crate::{SignServiceContext, application::use_cases::password_sign_in};

    struct Fixture {
        router: Router,
        session_id: u32,
        session_token: String,
    }

    #[tokio::test]
    async fn creates_one_pending_character() {
        let fixture = fixture().await;
        let request = create_request(fixture.session_id, &fixture.session_token);
        let response = fixture.router.clone().oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::CREATED);
        let body = response_json(response).await;
        assert_eq!(body["id"], 1);
        assert_eq!(body["is_new"], true);

        let request = create_request(fixture.session_id, &fixture.session_token);
        let response = fixture.router.oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::CONFLICT);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "pending_character_exists" })
        );
    }

    #[tokio::test]
    async fn rejects_an_invalid_session() {
        let service = SignService::new(SignServiceContext::for_test(false).await).unwrap();
        let response = super::super::router(service)
            .oneshot(create_request(1, "0123456789ABCDEF"))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "invalid_session" })
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

        Fixture {
            session_id: success.session.id.into(),
            session_token: String::from_utf8(success.session.token.to_vec()).unwrap(),
            router: super::super::router(service),
        }
    }

    fn create_request(session_id: u32, session_token: &str) -> Request<Body> {
        Request::post("/characters")
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
