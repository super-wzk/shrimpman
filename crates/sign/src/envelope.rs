use std::{io::Cursor, string::FromUtf8Error};

use binrw::{BinRead, Endian, NullString};
use bytes::Bytes;
use shrimpman_protocol::{CommandDecoder, DecodedCommand};
use thiserror::Error;

/// A textual command used by the Sign service.
#[derive(Debug, Clone)]
pub struct Command(Box<str>);

impl Command {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A client version encoded as three ASCII digits after a Sign command.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ClientVersion(u16);

impl ClientVersion {
    pub(crate) const ENCODED_LEN: usize = 3;

    const MAX: u16 = 999;

    pub(crate) const fn new(value: u16) -> Self {
        assert!(
            value <= Self::MAX,
            "client version must fit three decimal digits"
        );
        Self(value)
    }

    pub(crate) fn parse(encoded: &[u8]) -> Option<Self> {
        let encoded = std::str::from_utf8(encoded).ok()?;

        if encoded.len() != Self::ENCODED_LEN
            || !encoded.bytes().all(|digit| digit.is_ascii_digit())
        {
            return None;
        }

        encoded.parse::<u16>().ok().map(Self::new)
    }

    pub(crate) fn values() -> impl Iterator<Item = Self> {
        (0..=Self::MAX).map(Self::new)
    }

    pub(crate) const fn number(self) -> u16 {
        self.0
    }
}

/// An invalid Sign command envelope.
#[derive(Debug, Error)]
pub enum CommandDecodeError {
    #[error("failed to read Sign command at byte {position}: {source}")]
    Binary {
        position: u64,
        #[source]
        source: binrw::Error,
    },

    #[error("Sign command at byte {position} is missing its three-digit client version")]
    MissingVersion { position: u64 },

    #[error("Sign command at byte {position} has a non-numeric client version")]
    InvalidVersion { position: u64 },

    #[error("Sign command at byte {position} is not valid UTF-8: {source}")]
    InvalidEncoding {
        position: u64,
        #[source]
        source: FromUtf8Error,
    },

    #[error("Sign command at byte {position} must be non-empty ASCII ending with ':'")]
    InvalidCommand { position: u64 },
}

/// Decodes the command and client version prefix of a Sign packet.
pub(crate) struct SignCommandDecoder;

impl CommandDecoder for SignCommandDecoder {
    type Command = Command;
    type Metadata = ClientVersion;
    type Error = CommandDecodeError;

    fn decode_command(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<DecodedCommand<Self::Command, Self::Metadata>, Self::Error> {
        let position = payload.position();
        let mut encoded = NullString::read_options(payload, Endian::Big, ())
            .map_err(|source| CommandDecodeError::Binary { position, source })?
            .0;
        let Some(version_offset) = encoded.len().checked_sub(ClientVersion::ENCODED_LEN) else {
            return Err(CommandDecodeError::MissingVersion { position });
        };
        let version = ClientVersion::parse(&encoded[version_offset..])
            .ok_or(CommandDecodeError::InvalidVersion { position })?;
        encoded.truncate(version_offset);
        let command = String::from_utf8(encoded)
            .map_err(|source| CommandDecodeError::InvalidEncoding { position, source })?;

        if command.is_empty() || !command.is_ascii() || !command.ends_with(':') {
            return Err(CommandDecodeError::InvalidCommand { position });
        }

        Ok(DecodedCommand::new(
            Command(command.into_boxed_str()),
            version,
        ))
    }
}

#[cfg(test)]
mod tests {
    use shrimpman_protocol::CommandDecoder;

    use super::*;

    fn decode(
        input: &'static [u8],
    ) -> Result<DecodedCommand<Command, ClientVersion>, CommandDecodeError> {
        SignCommandDecoder.decode_command(&mut Cursor::new(Bytes::from_static(input)))
    }

    #[test]
    fn separates_command_and_version() {
        for (input, expected_command, expected_version) in [
            (b"SIGN:000\0".as_slice(), "SIGN:", 0),
            (b"SIGN:041\0".as_slice(), "SIGN:", 41),
            (b"DELETE:100\0".as_slice(), "DELETE:", 100),
            (b"DELETE:999\0".as_slice(), "DELETE:", 999),
        ] {
            let (command, version) = decode(input).unwrap().into_parts();

            assert_eq!(command.as_str(), expected_command);
            assert_eq!(version.number(), expected_version);
        }
    }

    #[test]
    fn rejects_malformed_commands() {
        for input in [
            b"SI\0".as_slice(),
            b"SIGN:04x\0".as_slice(),
            b"SIGN041\0".as_slice(),
            b"SIGN:1000\0".as_slice(),
        ] {
            assert!(decode(input).is_err());
        }
    }
}
