//! Mod metadata, dependency resolution and portable package archives.
//!
//! Discovery only reads metadata. Loading native libraries and configuring a
//! running game belong to the host.

mod archive;
mod diagnostics;
mod manifest;
mod profile;
mod resolve;
mod runtime_config;

pub use archive::{Pack, PackEntry, export_archive, import_archive};
pub use diagnostics::{DependencyIssue, DependencyIssueKind, ModDiagnostic, diagnose_resolution};
pub use manifest::{Candidate, Kind, Manifest, Source, discover};
pub use profile::BuiltinCatalog;
pub use resolve::{Resolved, Selection, resolve};
pub use runtime_config::{ModSettings, RuntimeConfig};
pub use semver::{Version, VersionReq};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0}")]
    Message(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Zip(#[from] zip::result::ZipError),
}

impl Error {
    pub(crate) fn new(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests;
