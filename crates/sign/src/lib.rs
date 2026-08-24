//! Sign service connection handling, command decoding, and routing.

#![warn(unreachable_pub)]

mod application;
mod config;
mod database;
mod envelope;
mod router;
mod server;

pub use application::{
    ConnectionError, InternalError, SignRepositories, SignService, SignServiceContext,
    SignSessionContext,
};
pub use config::{
    DiscoveryClientConfig, SignConfig, SignDatabaseConfig, SignLoggingConfig, SignServerConfig,
    SignSessionConfig,
};
pub use database::SignDatabase;
pub use envelope::{Command, CommandDecodeError};
pub use router::{SignRouteError, SignRouterBuildError};
pub use server::SignServer;

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
    async fn decodes_the_registered_character_deletion() {
        let mut payload = b"DELETE:041\0".to_vec();
        payload.extend_from_slice(b"0123456789ABCDEF\0");
        payload.extend_from_slice(&42_u32.to_be_bytes());
        payload.extend_from_slice(&7_u32.to_be_bytes());
        let transport = stream::iter([Ok::<_, Infallible>(Bytes::from(payload))]);
        let decoder = CommandPacketDecoder::new(SignCommandDecoder, SignRouter::new().unwrap());
        let mut packets = PacketStream::new(transport, decoder);
        let decoded = packets.next().await.unwrap().unwrap();

        assert_eq!(decoded.command().as_str(), "DELETE:");
        assert_eq!(decoded.metadata().number(), 41);
        assert_eq!(decoded.packet().mode(), DispatchMode::Ordered);
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
