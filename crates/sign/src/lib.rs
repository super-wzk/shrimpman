//! Sign service command decoding, routing, and packet handling.

#![warn(unreachable_pub)]

mod application;
mod command;
mod router;
mod version;

pub use application::{InternalError, SignContext};
pub use command::{Command, CommandDecodeError, SignCommandDecoder};
pub use router::{SignRouteError, SignRouter, SignRouterBuildError};
pub use version::ClientVersion;

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use bytes::Bytes;
    use futures_util::{StreamExt, stream};
    use shrimpman_protocol::{CommandPacketDecoder, DispatchMode, PacketStream};

    use super::*;

    #[tokio::test]
    async fn decodes_handlers_selected_by_registered_commands() {
        for command in ["SIGN:", "DSGN:", "DLTSKEYSIGN:"] {
            let payload = Bytes::from(format!("{command}041\0user\0pass\0\0"));
            let transport = stream::iter([Ok::<_, Infallible>(payload)]);
            let decoder = CommandPacketDecoder::new(SignCommandDecoder, SignRouter::new().unwrap());
            let mut packets = PacketStream::new(transport, decoder);
            let decoded = packets.next().await.unwrap().unwrap();

            assert_eq!(decoded.command().as_str(), command);
            assert_eq!(decoded.metadata().digits(), *b"041");
            assert_eq!(decoded.packet().mode(), DispatchMode::Ordered);
        }
    }

    #[tokio::test]
    async fn reports_a_later_unknown_command_after_yielding_the_first() {
        let valid = b"SIGN:041\0alice\0secret\0\0";
        let unknown = b"OTHER:041\0";
        let payload = Bytes::from([valid.as_slice(), unknown.as_slice()].concat());
        let transport = stream::iter([Ok::<_, Infallible>(payload)]);
        let decoder = CommandPacketDecoder::new(SignCommandDecoder, SignRouter::new().unwrap());
        let mut packets = PacketStream::new(transport, decoder);

        assert!(packets.next().await.unwrap().is_ok());
        assert!(packets.next().await.unwrap().is_err());
        assert!(packets.next().await.is_none());
    }
}
