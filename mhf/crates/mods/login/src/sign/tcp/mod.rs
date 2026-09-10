use crate::config::SignEncoding;
mod response;
#[cfg(test)]
mod tests;

use super::{Error, REQUEST_TIMEOUT};
use crate::model::{PasswordCredentials, SignInSuccess};
use bytes::Bytes;
use futures_util::{SinkExt, StreamExt};
use shrimpman_domain::{
    character::CharacterId,
    session::{SIGN_SESSION_TOKEN_LEN, SignSessionId},
};
use shrimpman_transport::{MhfConnection, TransportError};
use std::{io, time::Duration};
use tokio::{io::AsyncWriteExt, net::TcpStream, time::timeout};

pub(crate) struct Client {
    pub(super) address: String,
    encoding: SignEncoding,
}

impl Client {
    pub(crate) fn new(address: &str, encoding: SignEncoding) -> Result<Self, Error> {
        let address = address.trim();
        let authority = address.parse::<ureq::http::uri::Authority>().map_err(|_| {
            Error::InvalidRequest("Sign TCP address must be host:port or [IPv6]:port".into())
        })?;
        if authority.host().is_empty()
            || authority.port_u16().is_none_or(|port| port == 0)
            || address.contains('@')
        {
            return Err(Error::InvalidRequest(
                "Sign TCP address requires a host and a nonzero port".into(),
            ));
        }
        Ok(Self {
            address: address.to_owned(),
            encoding,
        })
    }

    pub(crate) fn sign_in(
        &self,
        credentials: &PasswordCredentials,
        create_character: bool,
        on_done: impl FnOnce(Result<SignInSuccess, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let payload = sign_in_request(credentials, create_character, self.encoding)?;
        self.request(payload, move |result| {
            on_done(result.and_then(|bytes| response::sign_in(&bytes)));
        })
    }

    pub(crate) fn delete_character(
        &self,
        session_id: SignSessionId,
        session_token: [u8; SIGN_SESSION_TOKEN_LEN],
        character_id: CharacterId,
        on_done: impl FnOnce(Result<CharacterId, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let mut payload = b"DELETE:100\0".to_vec();
        payload.extend_from_slice(&session_token);
        payload.push(0);
        payload.extend_from_slice(&u32::from(character_id).to_be_bytes());
        payload.extend_from_slice(&u32::from(session_id).to_be_bytes());
        self.request(payload, move |result| {
            on_done(result.and_then(|bytes| match bytes.as_ref() {
                [1] => Ok(character_id),
                _ => Err(Error::invalid_response(
                    "invalid character deletion acknowledgement",
                )),
            }));
        })
    }

    fn request(
        &self,
        payload: Vec<u8>,
        on_done: impl FnOnce(Result<Bytes, Error>) + Send + 'static,
    ) -> Result<(), Error> {
        let address = self.address.clone();
        std::thread::Builder::new()
            .name("sign-tcp".into())
            .spawn(move || {
                let result = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(tcp_io_error)
                    .and_then(|runtime| {
                        let result = runtime.block_on(async {
                            timeout(REQUEST_TIMEOUT, exchange(&address, payload.into()))
                                .await
                                .map_err(|_| Error::Timeout)?
                        });
                        // A timed-out OS DNS lookup must not delay delivery to the UI.
                        runtime.shutdown_background();
                        result
                    });
                on_done(result);
            })
            .map_err(|error| {
                Error::InvalidRequest(format!("failed to start Sign request: {error}"))
            })?;
        Ok(())
    }
}

fn sign_in_request(
    credentials: &PasswordCredentials,
    create_character: bool,
    encoding: SignEncoding,
) -> Result<Vec<u8>, Error> {
    if credentials.username.is_empty()
        || credentials.username.contains('\0')
        || credentials.password.contains('\0')
    {
        return Err(Error::InvalidRequest(
            "Sign TCP requires a nonempty username and credentials without NUL bytes".into(),
        ));
    }
    // The server reserves a trailing '+' for character creation. Reject it here
    // so a regular sign-in cannot authenticate a different account by accident.
    if credentials.username.ends_with('+') {
        return Err(Error::InvalidRequest(
            "Sign TCP usernames cannot end with '+'".into(),
        ));
    }
    // Erupe 9.2 matches the complete DSGN:100 command. Shrimpman and newer
    // Erupe versions also accept this command and version.
    let mut payload = b"DSGN:100\0".to_vec();
    payload.extend_from_slice(
        &encoding
            .encode(&credentials.username)
            .map_err(Error::InvalidRequest)?,
    );
    if create_character {
        payload.push(b'+');
    }
    payload.push(0);
    payload.extend_from_slice(
        &encoding
            .encode(&credentials.password)
            .map_err(Error::InvalidRequest)?,
    );
    payload.extend_from_slice(&[0, 0]);
    Ok(payload)
}

async fn exchange(address: &str, payload: Bytes) -> Result<Bytes, Error> {
    let mut io = timeout(Duration::from_secs(5), TcpStream::connect(address))
        .await
        .map_err(|_| Error::Timeout)?
        .map_err(tcp_io_error)?;
    io.write_all(&[0; 8]).await.map_err(tcp_io_error)?;
    let mut connection = MhfConnection::new(io);
    connection.send(payload).await.map_err(Error::Tcp)?;
    connection
        .next()
        .await
        .ok_or_else(|| {
            tcp_io_error(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "Sign closed the connection before replying",
            ))
        })?
        .map_err(Error::Tcp)
}

fn tcp_io_error(error: io::Error) -> Error {
    Error::Tcp(TransportError::Io(error))
}
