use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};

use super::{character, error_response};
use crate::{SignService, application::use_cases::password_sign_in::model};

pub(super) async fn handle(
    State(service): State<SignService>,
    request: Result<Json<RequestBody>, JsonRejection>,
) -> Result<Json<ResponseBody>, axum::response::Response> {
    let Json(request) = request.map_err(|error| {
        tracing::info!(%error, "Rejected malformed HTTP sign-in request");
        error_response(StatusCode::BAD_REQUEST, "invalid_request")
    })?;
    let outcome = service
        .password_sign_in(model::Request {
            username: request.username,
            password: request.password,
        })
        .await
        .map_err(|error| {
            tracing::error!(%error, "HTTP sign-in failed");
            error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal_error")
        })?;

    match outcome {
        model::Outcome::Success(success) => Ok(Json(ResponseBody::from(*success))),
        model::Outcome::IllegalInput => {
            Err(error_response(StatusCode::BAD_REQUEST, "illegal_input"))
        }
        model::Outcome::WrongPassword => {
            Err(error_response(StatusCode::UNAUTHORIZED, "wrong_password"))
        }
    }
}

#[derive(Deserialize)]
pub(super) struct RequestBody {
    username: String,
    password: String,
}

#[derive(Serialize)]
pub(super) struct ResponseBody {
    session: SessionBody,
    entrance_servers: Vec<String>,
    characters: Vec<character::ResponseBody>,
    last_character_id: Option<u32>,
    rights: u32,
    return_expires_at: Timestamp,
}

#[derive(Serialize)]
struct SessionBody {
    session_id: u32,
    token: String,
    issued_at: Timestamp,
}

impl From<model::Success> for ResponseBody {
    fn from(success: model::Success) -> Self {
        Self {
            session: SessionBody {
                session_id: success.session.id.into(),
                token: String::from_utf8(success.session.token.to_vec())
                    .expect("generated Sign session tokens are ASCII"),
                issued_at: success.session.issued_at,
            },
            entrance_servers: success.entrance_servers,
            characters: success
                .characters
                .into_iter()
                .map(|signed_in| character::ResponseBody::from(signed_in.character))
                .collect(),
            last_character_id: success.last_character_id.map(Into::into),
            rights: success.rights.bits(),
            return_expires_at: success.return_expires_at,
        }
    }
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
    use crate::SignServiceContext;

    #[tokio::test]
    async fn signs_in_with_json() {
        let response = test_router(true)
            .await
            .oneshot(json_request(json!({
                "username": "alice",
                "password": "secret"
            })))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = response_json(response).await;
        assert_eq!(body["session"]["session_id"], 1);
        assert_eq!(body["session"]["token"].as_str().unwrap().len(), 16);
        assert!(body["session"]["issued_at"].is_string());
        assert_eq!(body["entrance_servers"], json!([]));
        assert_eq!(body["characters"], json!([]));
        assert_eq!(body["last_character_id"], Value::Null);
        assert_eq!(body["rights"], 12);
        assert!(body["return_expires_at"].is_string());
    }

    #[tokio::test]
    async fn rejects_wrong_password() {
        let router = test_router(true).await;
        router
            .clone()
            .oneshot(json_request(json!({
                "username": "alice",
                "password": "secret"
            })))
            .await
            .unwrap();

        let response = router
            .oneshot(json_request(json!({
                "username": "alice",
                "password": "wrong"
            })))
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "wrong_password" })
        );
    }

    #[tokio::test]
    async fn rejects_malformed_json() {
        let request = Request::post("/sign-in")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from("{"))
            .unwrap();
        let response = test_router(false).await.oneshot(request).await.unwrap();

        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(
            response_json(response).await,
            json!({ "error": "invalid_request" })
        );
    }

    async fn test_router(auto_sign_up: bool) -> Router {
        let service = SignService::new(SignServiceContext::for_test(auto_sign_up).await).unwrap();
        super::super::router(service)
    }

    fn json_request(body: Value) -> Request<Body> {
        Request::post("/sign-in")
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
            .unwrap()
    }

    async fn response_json(response: Response) -> Value {
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        serde_json::from_slice(&body).unwrap()
    }
}
