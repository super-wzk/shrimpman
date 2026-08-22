use thiserror::Error;

/// An internal failure while handling a Sign packet.
#[derive(Debug, Error)]
pub enum InternalError {}
