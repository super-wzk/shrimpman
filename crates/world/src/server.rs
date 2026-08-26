use std::{collections::BTreeSet, future::Future, io};

use shrimpman_domain::world::LandKey;
use tokio::{net::TcpListener, task::JoinSet};

use crate::WorldLandConfig;

/// Owns the TCP listeners for every Land in one World process.
pub struct WorldServer {
    lands: Vec<BoundLand>,
}

impl WorldServer {
    /// Validates and binds every configured Land listener.
    pub async fn bind(lands: &[WorldLandConfig]) -> io::Result<Self> {
        validate_lands(lands)?;

        let mut bound_lands = Vec::with_capacity(lands.len());
        for land in lands {
            let listener = TcpListener::bind(land.listen_addr).await?;
            let listen_addr = listener.local_addr()?;
            tracing::info!(
                land = ?land.key,
                %listen_addr,
                advertise_port = land.port,
                "Land server is listening"
            );
            bound_lands.push(BoundLand {
                key: land.key.clone(),
                listener,
            });
        }

        Ok(Self { lands: bound_lands })
    }

    #[cfg(test)]
    fn local_addrs(&self) -> io::Result<Vec<std::net::SocketAddr>> {
        self.lands
            .iter()
            .map(|land| land.listener.local_addr())
            .collect()
    }

    /// Accepts Land connections until shutdown or a listener failure.
    ///
    /// The protocol session is intentionally not part of this initial skeleton;
    /// accepted connections are closed immediately.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future,
    {
        let mut listeners = JoinSet::new();
        for land in self.lands {
            listeners.spawn(run_land(land));
        }
        tokio::pin!(shutdown);

        let result = tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("Stopping World server");
                Ok(())
            }
            result = listeners.join_next() => match result {
                Some(Ok(result)) => result,
                Some(Err(error)) => Err(io::Error::other(error)),
                None => Ok(()),
            },
        };

        listeners.shutdown().await;
        result
    }
}

struct BoundLand {
    key: LandKey,
    listener: TcpListener,
}

async fn run_land(land: BoundLand) -> io::Result<()> {
    loop {
        let (connection, peer_addr) = land.listener.accept().await?;
        tracing::debug!(
            land = ?land.key,
            %peer_addr,
            "Closing Land connection because the World protocol is not implemented"
        );
        drop(connection);
    }
}

fn validate_lands(lands: &[WorldLandConfig]) -> io::Result<()> {
    if lands.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "a World must configure at least one Land",
        ));
    }

    let mut keys = BTreeSet::new();
    let mut ports = BTreeSet::new();
    for land in lands {
        if !keys.insert(&land.key) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Land keys must be unique within a World",
            ));
        }
        if land.port == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "advertised Land ports must be non-zero",
            ));
        }
        if !ports.insert(land.port) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "advertised Land ports must be unique within a World",
            ));
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::future;

    use super::*;

    fn land(key: &str, port: u16) -> WorldLandConfig {
        WorldLandConfig {
            key: LandKey::from(key.to_owned()),
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            port,
            max_players: 100,
        }
    }

    #[tokio::test]
    async fn binds_every_configured_land() {
        let server = WorldServer::bind(&[land("one", 54_001), land("two", 54_002)])
            .await
            .unwrap();
        let addresses = server.local_addrs().unwrap();

        assert_eq!(addresses.len(), 2);
        assert!(addresses.iter().all(|address| address.port() != 0));
        server.run(future::ready(())).await.unwrap();
    }

    #[tokio::test]
    async fn rejects_ambiguous_land_metadata() {
        let duplicate_keys = [land("same", 54_001), land("same", 54_002)];
        let duplicate_ports = [land("one", 54_001), land("two", 54_001)];

        assert_eq!(
            WorldServer::bind(&[]).await.err().unwrap().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            WorldServer::bind(&duplicate_keys)
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            WorldServer::bind(&duplicate_ports)
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
