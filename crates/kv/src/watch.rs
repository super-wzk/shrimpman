use std::{str, time::Duration};

use etcd_client::{Client, EventType, GetOptions, WatchOptions};
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tracing::{debug, warn};

/// One key-value pair returned by a prefix watch.
#[derive(Debug)]
pub struct KvEntry {
    key: String,
    value: Vec<u8>,
}

impl KvEntry {
    pub fn into_parts(self) -> (String, Vec<u8>) {
        (self.key, self.value)
    }
}

/// A synchronized prefix state or one subsequent key change.
#[derive(Debug)]
pub enum KvWatchEvent {
    Synchronized(Vec<KvEntry>),
    Put(KvEntry),
    Delete(String),
    Unavailable,
}

/// Reconnecting event stream for one etcd key prefix.
pub struct KvWatch {
    events: mpsc::UnboundedReceiver<KvWatchEvent>,
}

impl KvWatch {
    /// Waits for the next synchronization, change, or availability event.
    pub async fn recv(&mut self) -> Option<KvWatchEvent> {
        self.events.recv().await
    }
}

pub(crate) fn start(
    prefix: String,
    reconnect_delay: Duration,
    connection: watch::Receiver<Option<Client>>,
) -> KvWatch {
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    tokio::spawn(run(prefix, reconnect_delay, connection, event_tx));
    KvWatch { events: event_rx }
}

async fn run(
    prefix: String,
    reconnect_delay: Duration,
    mut connection: watch::Receiver<Option<Client>>,
    events: mpsc::UnboundedSender<KvWatchEvent>,
) {
    loop {
        let client = tokio::select! {
            client = wait_for_connection(&mut connection) => client,
            () = events.closed() => return,
        };
        let Some(mut client) = client else {
            return;
        };

        let result = tokio::select! {
            result = watch_prefix(&mut client, &prefix, &events) => Some(result),
            changed = connection.changed() => {
                if changed.is_err() {
                    return;
                }
                None
            }
            () = events.closed() => return,
        };
        if events.send(KvWatchEvent::Unavailable).is_err() {
            return;
        }

        let Some(Err(error)) = result else {
            continue;
        };
        warn!(%prefix, %error, "Lost an etcd prefix watch; restarting");

        tokio::select! {
            () = tokio::time::sleep(reconnect_delay) => {}
            changed = connection.changed() => {
                if changed.is_err() {
                    return;
                }
            }
            () = events.closed() => return,
        }
    }
}

async fn wait_for_connection(connection: &mut watch::Receiver<Option<Client>>) -> Option<Client> {
    connection
        .wait_for(Option::is_some)
        .await
        .ok()?
        .as_ref()
        .cloned()
}

async fn watch_prefix(
    client: &mut Client,
    prefix: &str,
    events: &mpsc::UnboundedSender<KvWatchEvent>,
) -> Result<(), WatchError> {
    let response = client
        .get(prefix, Some(GetOptions::new().with_prefix()))
        .await?;
    let revision = response.header().map_or(0, |header| header.revision());
    let entries = response
        .kvs()
        .iter()
        .filter_map(decode_entry)
        .collect::<Vec<_>>();
    let start_revision = revision
        .checked_add(1)
        .ok_or(WatchError::RevisionOverflow)?;
    let mut responses = client
        .watch(
            prefix,
            Some(
                WatchOptions::new()
                    .with_prefix()
                    .with_start_revision(start_revision),
            ),
        )
        .await?;
    let first_response = responses.message().await?.ok_or(WatchError::Closed)?;
    validate_watch(&first_response)?;
    if !first_response.created() {
        return Err(WatchError::NotCreated);
    }
    let entry_count = entries.len();
    events
        .send(KvWatchEvent::Synchronized(entries))
        .map_err(|_| WatchError::ConsumerClosed)?;
    debug!(%prefix, revision, entry_count, "Started etcd prefix watch");

    loop {
        let response = responses.message().await?.ok_or(WatchError::Closed)?;
        validate_watch(&response)?;
        for event in response.events() {
            let Some(kv) = event.kv() else {
                continue;
            };
            let Some(key) = decode_key(kv.key()) else {
                continue;
            };
            let event = match event.event_type() {
                EventType::Put => KvWatchEvent::Put(KvEntry {
                    key,
                    value: kv.value().to_vec(),
                }),
                EventType::Delete => KvWatchEvent::Delete(key),
            };
            events.send(event).map_err(|_| WatchError::ConsumerClosed)?;
        }
    }
}

fn decode_entry(entry: &etcd_client::KeyValue) -> Option<KvEntry> {
    Some(KvEntry {
        key: decode_key(entry.key())?,
        value: entry.value().to_vec(),
    })
}

fn decode_key(key: &[u8]) -> Option<String> {
    match str::from_utf8(key) {
        Ok(key) => Some(key.to_owned()),
        Err(error) => {
            warn!(%error, "Ignored non-UTF-8 key-value entry");
            None
        }
    }
}

fn validate_watch(response: &etcd_client::WatchResponse) -> Result<(), WatchError> {
    if response.canceled() {
        return Err(WatchError::Canceled(response.cancel_reason().to_owned()));
    }
    Ok(())
}

#[derive(Debug, Error)]
enum WatchError {
    #[error(transparent)]
    Etcd(#[from] etcd_client::Error),
    #[error("etcd prefix watch stream closed")]
    Closed,
    #[error("etcd did not acknowledge the prefix watch")]
    NotCreated,
    #[error("etcd canceled the prefix watch: {0}")]
    Canceled(String),
    #[error("etcd revision overflowed")]
    RevisionOverflow,
    #[error("the prefix watch consumer has stopped")]
    ConsumerClosed,
}
