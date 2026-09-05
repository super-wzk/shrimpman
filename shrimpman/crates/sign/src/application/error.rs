use shrimpman_common::{binary::FixedCStringLengthError, encoding::ShiftJisEncodeError};
use shrimpman_protocol::{CommandPacketDecodeError, DispatchError, OutboundSendError, PacketError};
use shrimpman_transport::TransportError;
use thiserror::Error;

use crate::{CommandDecodeError, SignRouteError};

/// An internal failure while handling a Sign packet.
#[derive(Debug, Error)]
pub enum InternalError {
    #[error("Sign database operation failed: {0}")]
    Database(#[from] toasty::Error),

    #[error("password hashing operation failed: {0}")]
    PasswordHash(#[from] argon2::password_hash::Error),

    #[error("password hashing task failed: {0}")]
    PasswordHashTask(#[from] tokio::task::JoinError),

    #[error("failed to encode a Sign string: {0}")]
    StringEncoding(#[from] ShiftJisEncodeError),

    #[error("a Sign response value exceeds its wire representation: {0}")]
    IntegerConversion(#[from] std::num::TryFromIntError),

    #[error("a Sign C string contains an interior null byte: {0}")]
    CString(#[from] std::ffi::NulError),

    #[error("failed to encode a fixed-width Sign C string: {0}")]
    FixedCString(#[from] FixedCStringLengthError),

    #[error("failed to send a Sign packet: {0}")]
    Outbound(#[from] OutboundSendError),

    #[error("a newly issued Sign session could not be authenticated")]
    InvalidIssuedSession,
}

/// A failure while serving one Sign connection.
#[derive(Debug, Error)]
pub enum ConnectionError {
    #[error("failed to read Sign connection initialization: {0}")]
    Initialization(#[source] std::io::Error),

    #[error("Sign connection closed before sending a request")]
    UnexpectedEof,

    #[error("failed to receive Sign request: {0}")]
    Receive(
        #[source]
        PacketError<
            TransportError,
            CommandPacketDecodeError<CommandDecodeError, SignRouteError, binrw::Error>,
        >,
    ),

    #[error("failed to dispatch Sign request: {0}")]
    Dispatch(#[source] DispatchError<InternalError>),

    #[error("failed to send Sign response: {0}")]
    Send(#[source] PacketError<TransportError, binrw::Error>),

    #[error("failed to close Sign connection: {0}")]
    Close(#[source] TransportError),
}
