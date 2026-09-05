//! Lease-bound key-value coordination backed by etcd.

#![warn(unreachable_pub)]

mod client;
mod watch;

pub use client::{LeaseKvClient, LeaseKvClientConfig, LeaseKvClientError};
pub use watch::{KvEntry, KvWatch, KvWatchEvent};
