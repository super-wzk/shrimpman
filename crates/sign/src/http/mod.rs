use std::net::SocketAddr;

use axum::{
    Json, Router,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, post},
};
use serde::{Deserialize, Serialize};

use crate::SignService;

mod character;
mod create_character;
mod delete_character;
mod password_sign_in;
mod server;

pub use server::Server;

const DEFAULT_PORT: u16 = 53_313;

/// HTTP listener configuration for the Sign service.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct Config {
    /// Address on which the Sign HTTP listener accepts requests.
    pub listen_addr: SocketAddr,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen_addr: SocketAddr::from(([0, 0, 0, 0], DEFAULT_PORT)),
        }
    }
}

fn router(service: SignService) -> Router {
    Router::new()
        .route("/sign-in", post(password_sign_in::handle))
        .route("/characters", post(create_character::handle))
        .route(
            "/characters/{character_id}",
            delete(delete_character::handle),
        )
        .with_state(service)
}

fn error_response(status: StatusCode, error: &'static str) -> Response {
    (status, Json(ErrorResponse { error })).into_response()
}

#[derive(Serialize)]
struct ErrorResponse {
    error: &'static str,
}
