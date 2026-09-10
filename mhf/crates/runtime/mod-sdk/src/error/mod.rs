use crate::abi as api;
use std::fmt;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorKind {
    OperationFailed,
    BufferTooSmall,
    NotFound,
    InvalidState,
    Conflict,
    Other,
}

/// A typed error. The private code preserves a provider's original failure for
/// ABI forwarding without exposing numeric status codes to ordinary callers.
pub struct Error {
    pub(crate) raw_status: i32,
    pub(crate) message: String,
}

impl Error {
    #[inline]
    pub fn new(message: impl Into<String>) -> Self {
        Self::with_kind(ErrorKind::OperationFailed, message)
    }

    #[inline]
    pub fn with_kind(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            raw_status: status_for_kind(kind),
            message: message.into(),
        }
    }

    #[inline]
    pub fn kind(&self) -> ErrorKind {
        kind_for_status(self.raw_status)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Error")
            .field("kind", &self.kind())
            .field("message", &self.message)
            .finish()
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

#[inline]
pub(crate) fn status_for_kind(kind: ErrorKind) -> api::Status {
    match kind {
        ErrorKind::OperationFailed => api::ERROR,
        ErrorKind::BufferTooSmall => api::BUFFER_TOO_SMALL,
        ErrorKind::NotFound => api::NOT_FOUND,
        ErrorKind::InvalidState => api::INVALID_STATE,
        ErrorKind::Conflict => api::CONFLICT,
        ErrorKind::Other => -1,
    }
}

#[inline]
pub(crate) fn kind_for_status(status: api::Status) -> ErrorKind {
    match status {
        api::ERROR => ErrorKind::OperationFailed,
        api::BUFFER_TOO_SMALL => ErrorKind::BufferTooSmall,
        api::NOT_FOUND => ErrorKind::NotFound,
        api::INVALID_STATE => ErrorKind::InvalidState,
        api::CONFLICT => ErrorKind::Conflict,
        _ => ErrorKind::Other,
    }
}

/// Translate a foreign failure without discarding an unknown provider code.
/// A success status cannot construct a successful Error and becomes a failure.
#[inline]
pub fn error_from_status(status: api::Status, message: impl Into<String>) -> Error {
    Error {
        raw_status: if status == api::OK {
            api::ERROR
        } else {
            status
        },
        message: message.into(),
    }
}

#[inline]
pub fn error_status(error: &Error) -> api::Status {
    error.raw_status
}

#[inline]
pub fn status_result(status: api::Status, operation: &str) -> Result<()> {
    if status == api::OK {
        Ok(())
    } else {
        Err(error_from_status(
            status,
            format!("{operation} failed ({status})"),
        ))
    }
}
