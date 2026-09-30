//! Common encrypted MHF transport used by shrimpman services.

#![warn(unreachable_pub)]

/// The result of attempting to decode one item from a byte stream.
///
/// `needed` is the total number of buffered bytes required to retry, rather
/// than the number of additional bytes to append.
#[derive(Debug, Clone, PartialEq, Eq)]
enum DecodeStep<T> {
    /// The input does not contain a complete item yet.
    NeedMore { needed: usize },
    /// One complete item was decoded.
    Complete { value: T, consumed: usize },
}

mod codec;
mod connection;
mod crypto;
mod error;
mod frame;

pub use connection::MhfConnection;
pub use error::TransportError;
pub use frame::PacketChecksums;
