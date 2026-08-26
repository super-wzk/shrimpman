use shrimpman_protocol::{CommandPacketDecodeError, DispatchError, OutboundSendError, PacketError};
use shrimpman_transport::TransportError;
use thiserror::Error;

use crate::LandRouteError;

/// An internal failure while handling a Land packet.
#[derive(Debug, Error)]
pub enum InternalError {
    #[error("failed to send a Land packet: {0}")]
    Outbound(#[from] OutboundSendError),
}

/// An invalid single-packet Land payload.
#[derive(Debug, Error)]
pub enum PacketDecodeError {
    #[error("Land payload is missing its big-endian MSG_SYS_END marker")]
    MissingEndMarker,

    #[error("expected Land MSG_SYS_END 0x0010, received {actual:#06x}")]
    InvalidEndMarker { actual: u16 },

    #[error("failed to decode Land packet: {0}")]
    Packet(#[source] CommandPacketDecodeError<binrw::Error, LandRouteError, binrw::Error>),

    #[error("Land packet left {remaining} bytes before MSG_SYS_END")]
    TrailingBody { remaining: u64 },
}

/// A failure while serving one Land connection.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("failed to receive Land packet: {0}")]
    Receive(#[source] PacketError<TransportError, PacketDecodeError>),

    #[error("failed to dispatch Land packet: {0}")]
    Dispatch(#[source] DispatchError<InternalError>),

    #[error("failed to send Land packet: {0}")]
    Send(#[source] PacketError<TransportError, binrw::Error>),
}
