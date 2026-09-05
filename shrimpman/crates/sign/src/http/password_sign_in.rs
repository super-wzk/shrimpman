use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
    http::StatusCode,
};
use jiff::Timestamp;
use serde::{Deserialize, Serialize};
use shrimpman_domain::mezeporta::{MezeportaFesta, MezeportaStall};

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
    notices: Vec<String>,
    last_character_id: Option<u32>,
    rights: u32,
    return_expires_at: Timestamp,
    festa: Option<FestaBody>,
}

#[derive(Serialize)]
struct SessionBody {
    session_id: u32,
    token: String,
    issued_at: Timestamp,
}

#[derive(Serialize)]
struct FestaBody {
    id: u32,
    starts_at: Timestamp,
    expires_at: Timestamp,
    solo_ticket_allowance: u32,
    group_ticket_allowance: u32,
    stalls: Vec<MezeportaStall>,
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
                .map(|signed_in| {
                    character::ResponseBody::signed_in(
                        signed_in.character,
                        signed_in.last_sign_in_at,
                    )
                })
                .collect(),
            notices: success
                .notices
                .into_iter()
                .map(|notice| notice.content)
                .collect(),
            last_character_id: success.last_character_id.map(Into::into),
            rights: success.rights.bits(),
            return_expires_at: success.return_expires_at,
            festa: success.festa.map(FestaBody::from),
        }
    }
}

impl From<MezeportaFesta> for FestaBody {
    fn from(festa: MezeportaFesta) -> Self {
        Self {
            id: festa.id,
            starts_at: festa.period.starts_at(),
            expires_at: festa.period.expires_at(),
            solo_ticket_allowance: festa.solo_ticket_allowance,
            group_ticket_allowance: festa.group_ticket_allowance,
            stalls: festa.stalls,
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
    use shrimpman_domain::{
        TimeRange,
        account::CourseRights,
        mezeporta::{MezeportaFesta, MezeportaStall},
        session::SignSessionId,
        sign_in_notice::SignInNotice,
    };
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
        assert_eq!(body["notices"], json!([]));
        assert_eq!(body["last_character_id"], Value::Null);
        assert_eq!(body["rights"], 12);
        assert!(body["return_expires_at"].is_string());
        assert_eq!(body["festa"], Value::Null);
    }

    #[test]
    fn response_includes_notices_and_festa() {
        let starts_at = Timestamp::new(1_800_000_000, 0).unwrap();
        let expires_at = Timestamp::new(1_800_003_600, 0).unwrap();
        let response = ResponseBody::from(model::Success {
            session: model::IssuedSession {
                id: SignSessionId::from(1),
                token: *b"0123456789ABCDEF",
                issued_at: starts_at,
            },
            entrance_servers: Vec::new(),
            characters: Vec::new(),
            notices: vec![SignInNotice {
                id: 1,
                content: "Welcome".to_owned(),
                period: TimeRange::new(starts_at, expires_at),
                priority: 1,
            }],
            last_character_id: None,
            rights: CourseRights::empty(),
            return_expires_at: expires_at,
            festa: Some(MezeportaFesta {
                id: 7,
                period: TimeRange::new(starts_at, expires_at),
                solo_ticket_allowance: 5,
                group_ticket_allowance: 2,
                stalls: vec![MezeportaStall::Unknown3, MezeportaStall::VolpakkunTogether],
            }),
        });

        let body = serde_json::to_value(response).unwrap();
        assert_eq!(body["notices"], json!(["Welcome"]));
        assert_eq!(
            body["festa"],
            json!({
                "id": 7,
                "starts_at": starts_at,
                "expires_at": expires_at,
                "solo_ticket_allowance": 5,
                "group_ticket_allowance": 2,
                "stalls": ["Unknown3", "VolpakkunTogether"]
            })
        );
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
