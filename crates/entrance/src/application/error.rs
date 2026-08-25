use thiserror::Error;

use shrimpman_common::encoding::ShiftJisEncodeError;
use shrimpman_protocol::{CommandPacketDecodeError, DispatchError, OutboundSendError, PacketError};
use shrimpman_transport::TransportError;

use crate::{CommandDecodeError, EntranceRouteError};

/// An internal failure while handling an Entrance packet.
#[derive(Debug, Error)]
pub enum InternalError {
    #[error("failed to encode an Entrance string: {0}")]
    StringEncoding(#[from] ShiftJisEncodeError),

    #[error("an Entrance C string contains an interior null byte: {0}")]
    CString(#[from] std::ffi::NulError),

    #[error("failed to send an Entrance packet: {0}")]
    Outbound(#[from] OutboundSendError),
}

/// A failure while serving one Entrance connection.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("failed to read Entrance connection initialization: {0}")]
    Initialization(#[source] std::io::Error),

    #[error("Entrance connection closed before sending a request")]
    UnexpectedEof,

    #[error("failed to receive Entrance request: {0}")]
    Receive(
        #[source]
        PacketError<
            TransportError,
            CommandPacketDecodeError<CommandDecodeError, EntranceRouteError, binrw::Error>,
        >,
    ),

    #[error("failed to dispatch Entrance request: {0}")]
    Dispatch(#[source] DispatchError<InternalError>),

    #[error("failed to send Entrance response: {0}")]
    Send(#[source] PacketError<TransportError, binrw::Error>),

    #[error("failed to close Entrance connection: {0}")]
    Close(#[source] TransportError),
}
