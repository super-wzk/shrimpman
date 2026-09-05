mod character;
mod sign_in;

use serde::{Serialize, de::DeserializeOwned};
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_mhf_launcher::{PasswordCredentials, SignCharacter, SignInSuccess};
use std::{fmt, time::Duration};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct Client {
    base_url: String,
}

impl Client {
    pub(crate) fn new(base_url: &str) -> Result<Self, Error> {
        let base_url = base_url.trim().trim_end_matches('/');
        if base_url.is_empty() {
            return Err(Error::InvalidRequest(
                "Sign HTTP base URL must not be empty".to_owned(),
            ));
        }

        Ok(Self {
            base_url: base_url.to_owned(),
        })
    }

    pub(crate) fn sign_in(
        &self,
        credentials: &PasswordCredentials,
        on_done: impl FnOnce(Result<SignInSuccess, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let request = sign_in::Request {
            username: &credentials.username,
            password: &credentials.password,
        };
        self.post(
            "/sign-in",
            &request,
            move |result: Result<sign_in::Response, Error>| {
                on_done(result.and_then(sign_in::Response::into_domain));
            },
        )
    }

    pub(crate) fn create_character(
        &self,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        on_done: impl FnOnce(Result<SignCharacter, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let request = session_request(session_id, &session_token)?;
        self.post(
            "/characters",
            &request,
            move |result: Result<character::Response, Error>| {
                on_done(result.and_then(character::Response::into_domain));
            },
        )
    }

    pub(crate) fn delete_character(
        &self,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        character_id: CharacterId,
        on_done: impl FnOnce(Result<CharacterId, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let request = session_request(session_id, &session_token)?;
        let request = ehttp::Request::post_json(
            format!("{}/characters/{}", self.base_url, u32::from(character_id)),
            &request,
        )
        .map_err(|error| Error::InvalidRequest(error.to_string()))?
        .with_method(ehttp::Method::DELETE)
        .with_timeout(Some(REQUEST_TIMEOUT));
        ehttp::fetch(request, move |response| {
            on_done(require_success(response).map(|_| character_id));
        });
        Ok(())
    }

    fn post<RequestBody, ResponseBody>(
        &self,
        path: &str,
        body: &RequestBody,
        on_done: impl FnOnce(Result<ResponseBody, Error>) + Send + 'static,
    ) -> Result<(), Error>
    where
        RequestBody: Serialize + ?Sized,
        ResponseBody: DeserializeOwned + 'static,
    {
        let request = ehttp::Request::post_json(format!("{}{path}", self.base_url), body)
            .map_err(|error| Error::InvalidRequest(error.to_string()))?
            .with_timeout(Some(REQUEST_TIMEOUT));
        ehttp::fetch(request, move |response| {
            on_done(parse_response(response));
        });
        Ok(())
    }
}

fn session_request(
    session_id: SignSessionId,
    session_token: &[u8; SIGN_SESSION_TOKEN_LEN],
) -> Result<character::SessionRequest<'_>, Error> {
    let session_token = std::str::from_utf8(session_token)
        .map_err(|_| Error::InvalidRequest("Sign session token is not valid ASCII".to_owned()))?;
    Ok(character::SessionRequest {
        session_id: session_id.into(),
        session_token,
    })
}

#[derive(Debug)]
pub(crate) enum Error {
    InvalidRequest(String),
    Transport(String),
    Response { status: u16, code: Option<String> },
    InvalidResponse(String),
}

impl Error {
    pub(crate) fn code(&self) -> Option<&str> {
        match self {
            Self::Response { code, .. } => code.as_deref(),
            _ => None,
        }
    }

    pub(super) fn invalid_response(message: impl Into<String>) -> Self {
        Self::InvalidResponse(message.into())
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidRequest(message) => formatter.write_str(message),
            Self::Transport(message) => write!(formatter, "Sign HTTP request failed: {message}"),
            Self::Response {
                status,
                code: Some(code),
            } => write!(formatter, "Sign HTTP returned {status} ({code})"),
            Self::Response { status, code: None } => {
                write!(formatter, "Sign HTTP returned status {status}")
            }
            Self::InvalidResponse(message) => {
                write!(
                    formatter,
                    "Sign HTTP returned an invalid response: {message}"
                )
            }
        }
    }
}

#[derive(serde::Deserialize)]
struct ErrorResponse {
    error: String,
}

fn parse_response<ResponseBody>(
    response: ehttp::Result<ehttp::Response>,
) -> Result<ResponseBody, Error>
where
    ResponseBody: DeserializeOwned,
{
    require_success(response)?
        .json()
        .map_err(|error| Error::InvalidResponse(error.to_string()))
}

fn require_success(response: ehttp::Result<ehttp::Response>) -> Result<ehttp::Response, Error> {
    let response = response.map_err(|error| Error::Transport(error.to_string()))?;
    if !response.ok {
        let code = response
            .json::<ErrorResponse>()
            .ok()
            .map(|response| response.error);
        return Err(Error::Response {
            status: response.status,
            code,
        });
    }
    Ok(response)
}
