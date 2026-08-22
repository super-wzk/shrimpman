use std::{error::Error, fmt, io};

use crate::frame::{FrameSizeMode, PacketChecksums};

/// Errors produced while reading, framing, or encrypting transport payloads.
#[derive(Debug)]
pub enum TransportError {
    Io(io::Error),
    Binary(binrw::Error),
    InvalidPf0(u8),
    BodyLengthNotRepresentable {
        len: usize,
        mode: FrameSizeMode,
    },
    BodyLengthMismatch {
        declared: usize,
        actual: usize,
    },
    ChecksumMismatch {
        expected: PacketChecksums,
        actual: PacketChecksums,
    },
}

impl fmt::Display for TransportError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(formatter, "transport I/O error: {error}"),
            Self::Binary(error) => write!(formatter, "binary frame error: {error}"),
            Self::InvalidPf0(value) => write!(formatter, "invalid frame pf0 value: {value:#04x}"),
            Self::BodyLengthNotRepresentable { len, mode } => {
                write!(
                    formatter,
                    "frame body length {len} cannot be represented in {mode:?} mode"
                )
            }
            Self::BodyLengthMismatch { declared, actual } => {
                write!(
                    formatter,
                    "frame header declares {declared} body bytes but contains {actual}"
                )
            }
            Self::ChecksumMismatch { expected, actual } => write!(
                formatter,
                "transport checksum mismatch: expected {:04x?}, calculated {:04x?}",
                expected.values(),
                actual.values()
            ),
        }
    }
}

impl Error for TransportError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Binary(error) => Some(error),
            _ => None,
        }
    }
}

impl From<io::Error> for TransportError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<binrw::Error> for TransportError {
    fn from(error: binrw::Error) -> Self {
        Self::Binary(error)
    }
}
