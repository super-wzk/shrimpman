//! Sign service connection handling, command decoding, and routing.

#![warn(unreachable_pub)]

mod application;
mod envelope;
mod router;

pub use application::{ConnectionError, InternalError, SignContext, SignService};
pub use envelope::{Command, CommandDecodeError};
pub use router::{SignRouteError, SignRouterBuildError};

#[cfg(test)]
mod tests {
    use std::convert::Infallible;

    use bytes::Bytes;
    use futures_util::{StreamExt, stream};
    use shrimpman_protocol::{CommandPacketDecoder, DispatchMode, PacketStream};

    use crate::{envelope::SignCommandDecoder, router::SignRouter};

    #[tokio::test]
    async fn decodes_handlers_selected_by_registered_commands() {
        for command in ["SIGN:", "DSGN:", "DLTSKEYSIGN:"] {
            let payload = Bytes::from(format!("{command}041\0user\0pass\0\0"));
            let transport = stream::iter([Ok::<_, Infallible>(payload)]);
            let decoder = CommandPacketDecoder::new(SignCommandDecoder, SignRouter::new().unwrap());
            let mut packets = PacketStream::new(transport, decoder);
            let decoded = packets.next().await.unwrap().unwrap();

            assert_eq!(decoded.command().as_str(), command);
            assert_eq!(decoded.metadata().number(), 41);
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
