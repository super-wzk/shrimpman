use std::{collections::BTreeSet, future::Future, io, sync::Arc};

use shrimpman_domain::world::LandKey;
use tokio::{net::TcpListener, task::JoinSet};
use tracing::Instrument;

use crate::{WorldLandConfig, WorldService};

/// Owns the TCP listeners for every Land in one World process.
pub struct WorldServer {
    lands: Vec<BoundLand>,
    service: Arc<WorldService>,
}

impl WorldServer {
    /// Validates and binds every configured Land listener around one shared service.
    pub async fn bind(lands: &[WorldLandConfig], service: WorldService) -> io::Result<Self> {
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

        Ok(Self {
            lands: bound_lands,
            service: Arc::new(service),
        })
    }

    #[cfg(test)]
    fn local_addrs(&self) -> io::Result<Vec<std::net::SocketAddr>> {
        self.lands
            .iter()
            .map(|land| land.listener.local_addr())
            .collect()
    }

    /// Accepts Land connections until shutdown or a listener failure.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future,
    {
        let mut listeners = JoinSet::new();
        for land in self.lands {
            listeners.spawn(run_land(land, Arc::clone(&self.service)));
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

async fn run_land(land: BoundLand, service: Arc<WorldService>) -> io::Result<()> {
    let BoundLand { key, listener } = land;
    let mut sessions = JoinSet::new();

    loop {
        tokio::select! {
            biased;

            Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                if let Err(error) = result {
                    tracing::error!(%error, "Land connection task terminated unexpectedly");
                }
            }
            accepted = listener.accept() => {
                let (connection, peer_addr) = accepted?;
                let service = Arc::clone(&service);
                let land = key.clone();
                let span = tracing::info_span!("land_connection", ?land, %peer_addr);

                sessions.spawn(
                    async move {
                        tracing::debug!("Accepted Land connection");
                        match service.serve_land_connection(connection, land).await {
                            Ok(()) => tracing::debug!("Closed Land connection"),
                            Err(error) => tracing::warn!(%error, "Land connection failed"),
                        }
                    }
                    .instrument(span),
                );
            }
        }
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
        let server = WorldServer::bind(
            &[land("one", 54_001), land("two", 54_002)],
            WorldService::new().unwrap(),
        )
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
            WorldServer::bind(&[], WorldService::new().unwrap())
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            WorldServer::bind(&duplicate_keys, WorldService::new().unwrap())
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            WorldServer::bind(&duplicate_ports, WorldService::new().unwrap())
                .await
                .err()
                .unwrap()
                .kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
