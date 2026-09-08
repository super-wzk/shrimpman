mod http;
mod tcp;

use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_mhf_launcher::{
    PasswordCredentials, SignCharacter, SignInSuccess, runtime::SignEncoding,
};
use std::{fmt, time::Duration};

pub(crate) const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) enum Client {
    Http(http::Client),
    Tcp(tcp::Client),
}

pub(crate) enum CharacterCreated {
    Character(SignCharacter),
    SignedIn(SignInSuccess),
}

impl Client {
    pub(crate) fn new(endpoint: &str, encoding: SignEncoding) -> Result<Self, Error> {
        let endpoint = endpoint.trim();
        let uri = endpoint.parse::<ureq::http::Uri>().map_err(|_| {
            Error::InvalidRequest(
                "Sign endpoint must be an absolute http://, https:// or tcp:// URI".into(),
            )
        })?;
        let authority = uri
            .authority()
            .ok_or_else(|| Error::InvalidRequest("Sign endpoint requires a host".into()))?;
        // port() also returns None for an invalid explicit port.
        let has_port = authority.as_str().len() > authority.host().len();
        if authority.host().is_empty()
            || authority.as_str().contains('@')
            || (has_port && authority.port_u16().is_none_or(|port| port == 0))
            || uri.query().is_some()
            || endpoint.contains('#')
        {
            return Err(Error::InvalidRequest("Sign endpoint has an invalid host or port, or contains user information, a query or a fragment".into()));
        }
        match uri.scheme_str() {
            Some("http" | "https") if encoding == SignEncoding::Utf8 => {
                http::Client::new(endpoint).map(Self::Http)
            }
            Some("http" | "https") => Err(Error::InvalidRequest(
                "Sign HTTP JSON requires utf8 encoding".into(),
            )),
            Some("tcp") if matches!(uri.path(), "" | "/") => {
                tcp::Client::new(authority.as_str(), encoding).map(Self::Tcp)
            }
            Some("tcp") => Err(Error::InvalidRequest(
                "Sign TCP endpoint cannot contain a path".into(),
            )),
            _ => Err(Error::InvalidRequest(
                "Sign endpoint must use http://, https:// or tcp://".into(),
            )),
        }
    }

    pub(crate) fn credential_target(&self) -> String {
        match self {
            Self::Http(client) => client.base_url.clone(),
            Self::Tcp(client) => format!("tcp://{}", client.address),
        }
    }

    pub(crate) fn sign_in(
        &self,
        credentials: &PasswordCredentials,
        on_done: impl FnOnce(Result<SignInSuccess, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        match self {
            Self::Http(client) => client.sign_in(credentials, on_done),
            Self::Tcp(client) => client.sign_in(credentials, false, on_done),
        }
    }

    pub(crate) fn create_character(
        &self,
        credentials: &PasswordCredentials,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        on_done: impl FnOnce(Result<CharacterCreated, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        match self {
            Self::Http(client) => {
                client.create_character(session_id, session_token, move |result| {
                    on_done(result.map(CharacterCreated::Character));
                })
            }
            Self::Tcp(client) => client.sign_in(credentials, true, move |result| {
                on_done(result.map(CharacterCreated::SignedIn));
            }),
        }
    }

    pub(crate) fn delete_character(
        &self,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        character_id: CharacterId,
        on_done: impl FnOnce(Result<CharacterId, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        match self {
            Self::Http(client) => {
                client.delete_character(session_id, session_token, character_id, on_done)
            }
            Self::Tcp(client) => {
                client.delete_character(session_id, session_token, character_id, on_done)
            }
        }
    }
}

#[derive(Debug)]
pub(crate) enum Error {
    InvalidRequest(String),
    Http(ureq::Error),
    Tcp(shrimpman_transport::TransportError),
    Timeout,
    TcpResponse(u8),
    HttpResponse { status: u16, code: Option<String> },
    InvalidResponse(String),
}

impl Error {
    pub(crate) fn code(&self) -> Option<&str> {
        match self {
            Self::HttpResponse { code, .. } => code.as_deref(),
            Self::TcpResponse(3) => Some("illegal_input"),
            Self::TcpResponse(12) => Some("wrong_password"),
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
            Self::Tcp(error) => write!(formatter, "Sign TCP request failed: {error}"),
            Self::Timeout => formatter.write_str("Sign TCP request timed out"),
            Self::TcpResponse(status) => write!(formatter, "Sign TCP returned status {status}"),
            Self::Http(message) => write!(formatter, "Sign HTTP request failed: {message}"),
            Self::HttpResponse {
                status,
                code: Some(code),
            } => write!(formatter, "Sign HTTP returned {status} ({code})"),
            Self::HttpResponse { status, code: None } => {
                write!(formatter, "Sign HTTP returned status {status}")
            }
            Self::InvalidResponse(message) => {
                write!(
                    formatter,
                    "Sign service returned an invalid response: {message}"
                )
            }
        }
    }
}

fn validate_sign_in(sign_in: SignInSuccess) -> Result<SignInSuccess, Error> {
    if u32::from(sign_in.session.session_id) == 0 {
        return Err(Error::invalid_response("Sign session ID must not be 0"));
    }
    if sign_in.characters.len() > 16 {
        return Err(Error::invalid_response(
            "at most 16 characters are supported",
        ));
    }
    for (index, character) in sign_in.characters.iter().enumerate() {
        if u32::from(character.id) == 0 {
            return Err(Error::invalid_response("character ID must not be 0"));
        }
        if sign_in.characters[..index]
            .iter()
            .any(|other| other.id == character.id)
        {
            return Err(Error::invalid_response("duplicate character ID"));
        }
    }
    if let Some(id) = sign_in.last_character_id
        && !sign_in
            .characters
            .iter()
            .any(|character| character.id == id)
    {
        return Err(Error::invalid_response(
            "last_character_id does not identify a returned character",
        ));
    }
    if sign_in
        .entrance_servers
        .iter()
        .any(|server| server.port() == 0)
    {
        return Err(Error::invalid_response(
            "entrance server port must not be 0",
        ));
    }
    Ok(sign_in)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn endpoint_scheme_selects_the_client_and_credential_scope() {
        for (endpoint, target) in [
            (" http://localhost:53001/ ", "http://localhost:53001"),
            ("https://example.com/sign/", "https://example.com/sign"),
            ("http://[::1]", "http://[::1]"),
            ("tcp://127.0.0.1:53000", "tcp://127.0.0.1:53000"),
            ("tcp://[::1]:53000/", "tcp://[::1]:53000"),
        ] {
            let client = Client::new(endpoint, SignEncoding::Utf8).unwrap();
            assert_eq!(client.credential_target(), target);
            assert_eq!(
                matches!(client, Client::Tcp(_)),
                target.starts_with("tcp://")
            );
        }
    }

    #[test]
    fn invalid_endpoints_are_rejected_before_sign_in() {
        for endpoint in [
            "",
            "localhost:53000",
            "ftp://localhost:53000",
            "tcp://localhost",
            "tcp://localhost:0",
            "tcp://localhost:65536",
            "tcp://localhost:53000/sign",
            "http://",
            "http://localhost:0",
            "http://localhost:65536",
            "http://user:password@localhost",
            "http://localhost?query",
            "http://localhost#fragment",
        ] {
            assert!(
                Client::new(endpoint, SignEncoding::Utf8).is_err(),
                "{endpoint}"
            );
        }
    }
}
