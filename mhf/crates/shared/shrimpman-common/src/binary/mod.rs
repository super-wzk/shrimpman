use std::error::Error as StdError;

use binrw::Error as BinError;

mod counted_vec;
mod length;
mod scalar;
mod string;

pub use counted_vec::CountedVec;
pub use length::{LengthEncoding, U8OrU16Length, U16OrU32Length};
pub use scalar::{Bool8, UnixTimestamp32};
pub use string::{FixedCString, FixedCStringLengthError, PrefixedCString};

pub(super) fn custom_error(
    position: u64,
    error: impl StdError + Send + Sync + 'static,
) -> BinError {
    BinError::Custom {
        pos: position,
        err: Box::new(error),
    }
}
