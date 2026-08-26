mod config;

use std::{collections::BTreeMap, time::Duration};

pub use config::LeaseKvClientConfig;
use etcd_client::{Client, PutOptions};
use thiserror::Error;
use tokio::sync::{mpsc, watch};
use tracing::{debug, info, warn};

use crate::KvWatch;

/// Cloneable client for values bound to one automatically renewed etcd lease.
#[derive(Clone)]
pub struct LeaseKvClient {
    commands: mpsc::UnboundedSender<ClientCommand>,
    connection: watch::Receiver<Option<Client>>,
    reconnect_delay: Duration,
}

impl LeaseKvClient {
    /// Starts a reconnecting leased etcd client on the current Tokio runtime.
    pub fn connect(config: LeaseKvClientConfig) -> Result<Self, LeaseKvClientError> {
        let lease_ttl = lease_ttl_seconds(config.lease_ttl)?;
        if config.endpoints.is_empty()
            || config
                .endpoints
                .iter()
                .any(|endpoint| endpoint.trim().is_empty())
        {
            return Err(LeaseKvClientError::InvalidEndpoints);
        }
        if config.reconnect_delay.is_zero() {
            return Err(LeaseKvClientError::InvalidReconnectDelay);
        }

        let reconnect_delay = config.reconnect_delay;
        let (command_tx, command_rx) = mpsc::unbounded_channel();
        let (connection_tx, connection_rx) = watch::channel(None);
        tokio::spawn(run(config, lease_ttl, command_rx, connection_tx));

        Ok(Self {
            commands: command_tx,
            connection: connection_rx,
            reconnect_delay,
        })
    }

    /// Publishes or replaces a value bound to this client's lease.
    pub fn put(
        &self,
        key: impl Into<String>,
        value: impl Into<Vec<u8>>,
    ) -> Result<(), LeaseKvClientError> {
        let key = key.into();
        validate_key(&key)?;
        self.commands
            .send(ClientCommand::Put(key, value.into()))
            .map_err(|_| LeaseKvClientError::Stopped)
    }

    /// Removes a value previously published by this client.
    pub fn delete(&self, key: impl Into<String>) -> Result<(), LeaseKvClientError> {
        let key = key.into();
        validate_key(&key)?;
        self.commands
            .send(ClientCommand::Delete(key))
            .map_err(|_| LeaseKvClientError::Stopped)
    }

    /// Watches an arbitrary key prefix through the shared etcd connection.
    pub fn watch_prefix(&self, prefix: impl Into<String>) -> KvWatch {
        crate::watch::start(prefix.into(), self.reconnect_delay, self.connection.clone())
    }
}

/// Failure to configure or send a command to a leased key-value client.
#[derive(Debug, Error)]
pub enum LeaseKvClientError {
    #[error("at least one non-empty etcd endpoint is required")]
    InvalidEndpoints,
    #[error("the key-value lease TTL must be a positive whole number of seconds")]
    InvalidLeaseTtl,
    #[error("the key-value reconnect delay must be positive")]
    InvalidReconnectDelay,
    #[error("key-value keys must be non-empty")]
    InvalidKey,
    #[error("leased key-value client has stopped")]
    Stopped,
}

enum ClientCommand {
    Put(String, Vec<u8>),
    Delete(String),
}

async fn run(
    config: LeaseKvClientConfig,
    lease_ttl: i64,
    mut commands: mpsc::UnboundedReceiver<ClientCommand>,
    connection: watch::Sender<Option<Client>>,
) {
    let mut values = BTreeMap::<String, Vec<u8>>::new();
    info!(
        endpoint_count = config.endpoints.len(),
        lease_ttl_seconds = lease_ttl,
        reconnect_delay = ?config.reconnect_delay,
        "Starting leased key-value client"
    );

    loop {
        match Client::connect(&config.endpoints, None).await {
            Ok(mut client) => {
                match serve_connection(
                    &mut client,
                    lease_ttl,
                    &mut commands,
                    &mut values,
                    &connection,
                )
                .await
                {
                    Ok(()) => break,
                    Err(error) => {
                        warn!(
                            %error,
                            reconnect_delay = ?config.reconnect_delay,
                            "Lost the etcd connection; reconnecting"
                        );
                    }
                }
            }
            Err(error) => {
                warn!(
                    %error,
                    reconnect_delay = ?config.reconnect_delay,
                    "Failed to connect to etcd; retrying"
                );
            }
        }

        connection.send_replace(None);
        if !wait_to_reconnect(config.reconnect_delay, &mut commands, &mut values).await {
            break;
        }
    }

    connection.send_replace(None);
    info!("Leased key-value client stopped");
}

async fn serve_connection(
    client: &mut Client,
    lease_ttl: i64,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    values: &mut BTreeMap<String, Vec<u8>>,
    connection: &watch::Sender<Option<Client>>,
) -> Result<(), ConnectionError> {
    let lease_id = client.lease_grant(lease_ttl, None).await?.id();
    let (mut lease_keeper, mut lease_responses) = client.lease_keep_alive(lease_id).await?;

    for (key, value) in values.iter() {
        put_value(client, lease_id, key, value).await?;
    }

    connection.send_replace(Some(client.clone()));
    info!(
        lease_id,
        published_values = values.len(),
        "Connected to etcd and restored leased key-value entries"
    );

    let keep_alive_interval = Duration::from_secs((lease_ttl / 3).max(1) as u64);
    let mut keep_alive = tokio::time::interval(keep_alive_interval);
    keep_alive.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = keep_alive.tick() => {
                lease_keeper.keep_alive().await?;
            }
            response = lease_responses.message() => {
                let response = response?.ok_or(ConnectionError::LeaseClosed)?;
                if response.ttl() <= 0 {
                    return Err(ConnectionError::LeaseExpired);
                }
            }
            command = commands.recv() => match command {
                Some(ClientCommand::Put(key, value)) => {
                    values.insert(key.clone(), value.clone());
                    put_value(client, lease_id, &key, &value).await?;
                    debug!(%key, "Published leased key-value entry");
                }
                Some(ClientCommand::Delete(key)) => {
                    if values.remove(&key).is_some() {
                        client.delete(key.as_str(), None).await?;
                        debug!(%key, "Removed leased key-value entry");
                    }
                }
                None => {
                    let _ = client.lease_revoke(lease_id).await;
                    return Ok(());
                }
            }
        }
    }
}

async fn put_value(
    client: &mut Client,
    lease_id: i64,
    key: &str,
    value: &[u8],
) -> Result<(), ConnectionError> {
    client
        .put(key, value, Some(PutOptions::new().with_lease(lease_id)))
        .await?;
    Ok(())
}

async fn wait_to_reconnect(
    delay: Duration,
    commands: &mut mpsc::UnboundedReceiver<ClientCommand>,
    values: &mut BTreeMap<String, Vec<u8>>,
) -> bool {
    let sleep = tokio::time::sleep(delay);
    tokio::pin!(sleep);

    loop {
        tokio::select! {
            () = &mut sleep => return true,
            command = commands.recv() => match command {
                Some(ClientCommand::Put(key, value)) => {
                    values.insert(key, value);
                }
                Some(ClientCommand::Delete(key)) => {
                    values.remove(&key);
                }
                None => return false,
            }
        }
    }
}

fn lease_ttl_seconds(ttl: Duration) -> Result<i64, LeaseKvClientError> {
    if ttl.is_zero() || ttl.subsec_nanos() != 0 {
        return Err(LeaseKvClientError::InvalidLeaseTtl);
    }

    i64::try_from(ttl.as_secs()).map_err(|_| LeaseKvClientError::InvalidLeaseTtl)
}

fn validate_key(key: &str) -> Result<(), LeaseKvClientError> {
    if key.is_empty() {
        return Err(LeaseKvClientError::InvalidKey);
    }
    Ok(())
}

#[derive(Debug, Error)]
enum ConnectionError {
    #[error(transparent)]
    Etcd(#[from] etcd_client::Error),
    #[error("etcd lease keep-alive stream closed")]
    LeaseClosed,
    #[error("etcd lease expired")]
    LeaseExpired,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_client_configuration() {
        assert!(
            LeaseKvClient::connect(LeaseKvClientConfig {
                endpoints: Vec::new(),
                ..LeaseKvClientConfig::default()
            })
            .is_err()
        );
        assert!(
            LeaseKvClient::connect(LeaseKvClientConfig {
                lease_ttl: Duration::from_millis(500),
                ..LeaseKvClientConfig::default()
            })
            .is_err()
        );
        assert!(
            LeaseKvClient::connect(LeaseKvClientConfig {
                reconnect_delay: Duration::ZERO,
                ..LeaseKvClientConfig::default()
            })
            .is_err()
        );
    }

    #[test]
    fn validates_keys() {
        assert!(validate_key("/services/world/instance").is_ok());
        assert!(validate_key("").is_err());
    }
}
