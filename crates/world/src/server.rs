use std::{collections::BTreeSet, future::Future, io, sync::Arc};

use shrimpman_domain::world::LandKey;
use tokio::{
    net::TcpListener,
    task::{JoinError, JoinSet},
};
use tokio_util::sync::CancellationToken;
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

    /// Accepts Land connections until shutdown or a listener failure.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future<Output = ()>,
    {
        let mut listeners = JoinSet::new();
        let cancellation = CancellationToken::new();
        for land in self.lands {
            listeners.spawn(run_land(
                land,
                Arc::clone(&self.service),
                cancellation.child_token(),
            ));
        }
        tokio::pin!(shutdown);

        tokio::select! {
            biased;

            _ = &mut shutdown => {
                tracing::info!("Stopping World server");
                cancellation.cancel();
                while let Some(result) = listeners.join_next().await {
                    match result {
                        Ok(result) => result?,
                        Err(error) => return Err(io::Error::other(error)),
                    }
                }
                Ok(())
            }
            result = listeners.join_next() => {
                listeners.shutdown().await;
                match result {
                    Some(Ok(result)) => result,
                    Some(Err(error)) => Err(io::Error::other(error)),
                    None => Ok(()),
                }
            }
        }
    }
}

struct BoundLand {
    key: LandKey,
    listener: TcpListener,
}

async fn run_land(
    land: BoundLand,
    service: Arc<WorldService>,
    cancellation: CancellationToken,
) -> io::Result<()> {
    let BoundLand { key, listener } = land;
    let mut sessions = JoinSet::new();

    loop {
        tokio::select! {
            biased;

            () = cancellation.cancelled() => {
                tracing::info!(
                    land = ?key,
                    active_connections = sessions.len(),
                    "Stopping Land server"
                );
                break;
            }
            Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                report_join_error(result);
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

    drop(listener);
    drain_sessions(&key, sessions).await;

    Ok(())
}

async fn drain_sessions(land: &LandKey, mut sessions: JoinSet<()>) {
    if sessions.is_empty() {
        return;
    }

    tracing::info!(
        ?land,
        active_connections = sessions.len(),
        "Waiting for active Land connections"
    );
    while let Some(result) = sessions.join_next().await {
        report_join_error(result);
    }
    tracing::info!(?land, "All active Land connections completed");
}

fn report_join_error(result: Result<(), JoinError>) {
    if let Err(error) = result {
        tracing::error!(%error, "Land connection task terminated unexpectedly");
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

    use tokio::sync::oneshot;

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
        let db = crate::test_database().await;
        let service = WorldService::new(crate::test_repositories(&db)).unwrap();
        let server = WorldServer::bind(&[land("one", 54_001), land("two", 54_002)], service)
            .await
            .unwrap();

        assert_eq!(server.lands.len(), 2);
        assert!(server.lands.iter().all(|land| {
            land.listener
                .local_addr()
                .is_ok_and(|address| address.port() != 0)
        }));
        server.run(future::ready(())).await.unwrap();
    }

    #[tokio::test]
    async fn waits_for_active_sessions_during_shutdown() {
        let (complete, completion) = oneshot::channel();
        let mut sessions = JoinSet::new();
        sessions.spawn(async move {
            let _ = completion.await;
        });
        let land = LandKey::from("one".to_owned());
        let draining = drain_sessions(&land, sessions);
        tokio::pin!(draining);

        tokio::select! {
            () = &mut draining => panic!("active session drained before it completed"),
            () = tokio::task::yield_now() => {}
        }
        complete.send(()).unwrap();
        draining.await;
    }

    #[test]
    fn rejects_ambiguous_land_metadata() {
        let duplicate_keys = [land("same", 54_001), land("same", 54_002)];
        let duplicate_ports = [land("one", 54_001), land("two", 54_001)];

        assert_eq!(
            validate_lands(&[]).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            validate_lands(&duplicate_keys).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
        assert_eq!(
            validate_lands(&duplicate_ports).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
}
