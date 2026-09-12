//! Client resource formats. File offsets and encoded values are not native pointers.
//!
//! Parsers retain source bytes and unknown fields. A field receives a semantic
//! name only when its use is established by the client or a verified sample.

use std::fmt;

pub mod binary;
pub mod container;
pub mod crypto;
pub mod dat;
pub mod dds;
mod decoded;
pub mod effect;
pub mod effect_archive;
pub mod event_camera;
pub mod fmod;
pub mod fskl;
pub mod inf;
pub mod jkr;
pub mod material;
pub mod motion;
pub mod png;
pub mod stage;
pub mod txb;

pub use decoded::Decoded;

/// An offset in an input resource, not a pointer into the game process.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub offset: usize,
    pub message: String,
}

impl Error {
    pub fn new(offset: usize, message: impl Into<String>) -> Self {
        Self {
            offset,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:#x}: {}", self.offset, self.message)
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;
