use std::io;

use thiserror::Error;

use crate::frame::PacketChecksums;

/// Errors produced while reading, framing, or encrypting transport payloads.
#[derive(Debug, Error)]
pub enum TransportError {
    #[error("transport I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("binary frame error: {0}")]
    Binary(#[from] binrw::Error),

    #[error("invalid frame pf0 value: {0:#04x}")]
    InvalidPf0(u8),

    #[error("frame body length {len} cannot be represented by the frame header")]
    BodyLengthNotRepresentable { len: usize },

    #[error("frame header declares {declared} body bytes but contains {actual}")]
    BodyLengthMismatch { declared: usize, actual: usize },

    #[error(
        "transport checksum mismatch: expected {:04x?}, calculated {:04x?}",
        .expected.values(),
        .actual.values()
    )]
    ChecksumMismatch {
        expected: PacketChecksums,
        actual: PacketChecksums,
    },
}
