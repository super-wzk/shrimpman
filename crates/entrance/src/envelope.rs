use std::{io::Cursor, string::FromUtf8Error};

use binrw::{BinRead, Endian, NullString};
use bytes::Bytes;
use shrimpman_protocol::{CommandDecoder, DecodedCommand};
use thiserror::Error;

/// A textual command used by the Entrance service.
#[derive(Debug, Clone)]
pub struct Command(Box<str>);

impl Command {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// An invalid Entrance command envelope.
#[derive(Debug, Error)]
pub enum CommandDecodeError {
    #[error("failed to read Entrance envelope at byte {position}: {source}")]
    Binary {
        position: u64,
        #[source]
        source: binrw::Error,
    },

    #[error("Entrance command at byte {position} is not valid UTF-8: {source}")]
    InvalidEncoding {
        position: u64,
        #[source]
        source: FromUtf8Error,
    },

    #[error("Entrance command at byte {position} must be non-empty visible ASCII")]
    InvalidCommand { position: u64 },
}

/// Decodes the null-terminated command prefix of an Entrance packet.
pub(crate) struct EntranceCommandDecoder;

impl CommandDecoder for EntranceCommandDecoder {
    type Command = Command;
    type Metadata = ();
    type Error = CommandDecodeError;

    fn decode_command(
        &mut self,
        payload: &mut Cursor<Bytes>,
    ) -> Result<DecodedCommand<Self::Command, Self::Metadata>, Self::Error> {
        let position = payload.position();
        let encoded = NullString::read_options(payload, Endian::Big, ())
            .map_err(|source| CommandDecodeError::Binary { position, source })?
            .0;
        let command = String::from_utf8(encoded)
            .map_err(|source| CommandDecodeError::InvalidEncoding { position, source })?;

        if command.is_empty() || !command.bytes().all(|byte| byte.is_ascii_graphic()) {
            return Err(CommandDecodeError::InvalidCommand { position });
        }

        Ok(DecodedCommand::new(Command(command.into_boxed_str()), ()))
    }
}

#[cfg(test)]
mod tests {
    use shrimpman_protocol::CommandDecoder;

    use super::*;

    fn decode(input: &'static [u8]) -> Result<DecodedCommand<Command, ()>, CommandDecodeError> {
        EntranceCommandDecoder.decode_command(&mut Cursor::new(Bytes::from_static(input)))
    }

    #[test]
    fn separates_a_null_terminated_command_from_its_body() {
        let mut payload = Cursor::new(Bytes::from_static(b"ALL+\0\0\x01\0\0\0\x2a"));

        let (command, ()) = EntranceCommandDecoder
            .decode_command(&mut payload)
            .unwrap()
            .into_parts();

        assert_eq!(command.as_str(), "ALL+");
        assert_eq!(payload.position(), 5);
        assert_eq!(&payload.get_ref()[5..], b"\0\x01\0\0\0\x2a");
    }

    #[test]
    fn rejects_malformed_envelopes() {
        for input in [
            b"ALL".as_slice(),
            b"ALL+".as_slice(),
            b"\0".as_slice(),
            b"ALL COMMAND\0".as_slice(),
            b"\xf0\x9f\xa6\x90\0".as_slice(),
            b"\xffLL+\0".as_slice(),
        ] {
            assert!(decode(input).is_err(), "accepted {input:?}");
        }
    }

    #[test]
    fn leaves_command_length_to_the_router() {
        let (command, ()) = decode(b"LONG-COMMAND\0").unwrap().into_parts();

        assert_eq!(command.as_str(), "LONG-COMMAND");
    }
}
