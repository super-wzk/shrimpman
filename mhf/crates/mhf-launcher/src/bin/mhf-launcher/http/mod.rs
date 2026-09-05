mod character;
mod sign_in;

use serde::{Serialize, de::DeserializeOwned};
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_mhf_launcher::{PasswordCredentials, SignCharacter, SignInSuccess};
use std::{fmt, time::Duration};
use ureq::http::Method;

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) struct Client {
    base_url: String,
    agent: ureq::Agent,
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
            agent: ureq::Agent::config_builder()
                .timeout_connect(Some(Duration::from_secs(5)))
                .timeout_global(Some(REQUEST_TIMEOUT))
                .http_status_as_error(false)
                .build()
                .into(),
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
        self.request(
            Method::DELETE,
            &format!("/characters/{}", u32::from(character_id)),
            &request,
            move |result| on_done(result.map(|_| character_id)),
        )
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
        self.request(Method::POST, path, body, move |result| {
            on_done(result.and_then(|bytes| {
                serde_json::from_slice(&bytes)
                    .map_err(|error| Error::InvalidResponse(error.to_string()))
            }));
        })
    }

    fn request(
        &self,
        method: Method,
        path: &str,
        body: &(impl Serialize + ?Sized),
        on_done: impl FnOnce(Result<Vec<u8>, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let body =
            serde_json::to_vec(body).map_err(|error| Error::InvalidRequest(error.to_string()))?;
        let request = ureq::http::Request::builder()
            .method(method)
            .uri(format!("{}{path}", self.base_url))
            .header("Content-Type", "application/json")
            .body(body)
            .map_err(|error| Error::InvalidRequest(error.to_string()))?;
        let agent = self.agent.clone();
        std::thread::Builder::new()
            .name("sign-http".to_owned())
            .spawn(move || {
                let result = agent
                    .run(request)
                    .map_err(Error::Transport)
                    .and_then(require_success);
                on_done(result);
            })
            .map_err(|error| {
                Error::InvalidRequest(format!("failed to start Sign request: {error}"))
            })?;
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
    Transport(ureq::Error),
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

fn require_success(mut response: ureq::http::Response<ureq::Body>) -> Result<Vec<u8>, Error> {
    let status = response.status();
    let bytes = response
        .body_mut()
        .read_to_vec()
        .map_err(Error::Transport)?;
    if !status.is_success() {
        let code = serde_json::from_slice::<ErrorResponse>(&bytes)
            .ok()
            .map(|response| response.error);
        return Err(Error::Response {
            status: status.as_u16(),
            code,
        });
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::mpsc,
        thread,
        time::Instant,
    };

    #[test]
    fn all_operations_time_out_waiting_for_headers_or_body() {
        thread::scope(|scope| {
            for partial_response in [
                "",
                "HTTP/1.1 200 OK\r\nContent-Length: 100\r\nContent-Type: application/json\r\n\r\n{",
            ] {
                for operation in ["sign_in", "create", "delete"] {
                    scope.spawn(move || {
                        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
                        listener.set_nonblocking(true).unwrap();
                        let client =
                            Client::new(&format!("http://{}", listener.local_addr().unwrap()))
                                .unwrap();
                        let (sender, receiver) = mpsc::channel();
                        let started = Instant::now();
                        match operation {
                            "sign_in" => client.sign_in(
                                &PasswordCredentials {
                                    username: "hunter".to_owned(),
                                    password: "test-password".to_owned(),
                                },
                                move |result| sender.send(result.map(|_| ())).unwrap(),
                            ),
                            "create" => client.create_character(
                                SignSessionId::from(1),
                                *b"0123456789abcdef",
                                move |result| sender.send(result.map(|_| ())).unwrap(),
                            ),
                            "delete" => client.delete_character(
                                SignSessionId::from(1),
                                *b"0123456789abcdef",
                                CharacterId::from(7),
                                move |result| sender.send(result.map(|_| ())).unwrap(),
                            ),
                            _ => unreachable!(),
                        }
                        .unwrap();
                        let mut stream = loop {
                            match listener.accept() {
                                Ok((stream, _)) => break stream,
                                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                                    assert!(started.elapsed() < Duration::from_secs(5));
                                    thread::sleep(Duration::from_millis(10));
                                }
                                Err(error) => panic!("failed to accept request: {error}"),
                            }
                        };
                        stream.set_nonblocking(false).unwrap();
                        stream
                            .set_read_timeout(Some(Duration::from_secs(2)))
                            .unwrap();
                        let mut request = Vec::new();
                        let mut byte = [0];
                        while !request.ends_with(b"\r\n\r\n") {
                            stream.read_exact(&mut byte).unwrap();
                            request.push(byte[0]);
                        }
                        let headers = String::from_utf8(request).unwrap();
                        let content_length: usize = headers
                            .lines()
                            .find_map(|line| {
                                let (name, value) = line.split_once(':')?;
                                name.eq_ignore_ascii_case("content-length")
                                    .then(|| value.trim().parse().unwrap())
                            })
                            .unwrap();
                        let mut body = vec![0; content_length];
                        stream.read_exact(&mut body).unwrap();
                        let body: serde_json::Value = serde_json::from_slice(&body).unwrap();
                        if operation == "sign_in" {
                            assert!(headers.starts_with("POST /sign-in "));
                            assert_eq!(body["username"], "hunter");
                        } else {
                            assert!(headers.starts_with(if operation == "create" {
                                "POST /characters "
                            } else {
                                "DELETE /characters/7 "
                            }));
                            assert_eq!(body["session_token"], "0123456789abcdef");
                        }
                        stream.write_all(partial_response.as_bytes()).unwrap();
                        let result = receiver
                            .recv_timeout(REQUEST_TIMEOUT + Duration::from_secs(3))
                            .unwrap();
                        assert!(
                            matches!(result, Err(Error::Transport(ureq::Error::Timeout(_)))),
                            "{operation}: {result:?}"
                        );
                        assert!(started.elapsed() < REQUEST_TIMEOUT + Duration::from_secs(3));
                    });
                }
            }
        });
    }
}
