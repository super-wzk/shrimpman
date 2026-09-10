//! Mod metadata, dependency resolution and portable package archives.
//!
//! Discovery only reads metadata. Loading native libraries and configuring a
//! running game belong to the host.

mod archive;
mod manifest;
mod profile;
mod resolve;
mod runtime_config;

pub use archive::{Pack, PackEntry, export_archive, import_archive};
pub use manifest::{Candidate, Kind, Manifest, Source, discover};
pub use profile::BuiltinCatalog;
pub use resolve::{Resolved, Selection, resolve};
pub use runtime_config::{ModSettings, RuntimeConfig};
pub use semver::{Version, VersionReq};

#[derive(Debug)]
pub struct Error(String);

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(formatter)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self(error.to_string())
    }
}

impl From<zip::result::ZipError> for Error {
    fn from(error: zip::result::ZipError) -> Self {
        Self(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
