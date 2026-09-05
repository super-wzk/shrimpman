use std::{future::Future, io, net::SocketAddr};

use tokio::{
    net::TcpListener,
    task::{JoinError, JoinSet},
};
use tracing::Instrument;

use crate::{SignServerConfig, SignService};

/// Accepts TCP connections and dispatches them to the Sign service.
pub struct SignServer {
    listener: TcpListener,
    service: SignService,
}

impl SignServer {
    /// Binds the configured listener around an initialized Sign service.
    pub async fn bind(config: SignServerConfig, service: SignService) -> io::Result<Self> {
        let listener = TcpListener::bind(config.listen_addr).await?;

        Ok(Self { listener, service })
    }

    /// Returns the listener's effective local address.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    /// Serves connections until the shutdown future completes or the listener fails.
    ///
    /// After accepting stops, waits for active connections to finish.
    pub async fn run<Shutdown>(self, shutdown: Shutdown) -> io::Result<()>
    where
        Shutdown: Future<Output = ()>,
    {
        let Self { listener, service } = self;
        let mut sessions = JoinSet::new();
        tokio::pin!(shutdown);

        let result = loop {
            tokio::select! {
                biased;

                _ = &mut shutdown => {
                    tracing::info!(
                        active_connections = sessions.len(),
                        "Stopping Sign server"
                    );
                    break Ok(());
                }
                Some(result) = sessions.join_next(), if !sessions.is_empty() => {
                    report_join_error(result);
                }
                accepted = listener.accept() => {
                    let (io, peer_addr) = match accepted {
                        Ok(connection) => connection,
                        Err(error) => break Err(error),
                    };
                    let service = service.clone();
                    let span = tracing::info_span!("sign_connection", %peer_addr);

                    sessions.spawn(
                        async move {
                            tracing::debug!("Accepted Sign connection");
                            match service.serve_connection(io).await {
                                Ok(()) => tracing::debug!("Closed Sign connection"),
                                Err(error) => tracing::warn!(%error, "Sign connection failed"),
                            }
                        }
                        .instrument(span),
                    );
                }
            }
        };

        drop(listener);
        if let Err(error) = result {
            sessions.shutdown().await;
            return Err(error);
        }
        drain_sessions(sessions).await;

        Ok(())
    }
}

fn report_join_error(result: Result<(), JoinError>) {
    if let Err(error) = result {
        tracing::error!(%error, "Sign connection task terminated unexpectedly");
    }
}

async fn drain_sessions(mut sessions: JoinSet<()>) {
    if sessions.is_empty() {
        return;
    }

    tracing::info!(
        active_connections = sessions.len(),
        "Waiting for active Sign connections"
    );
    while let Some(result) = sessions.join_next().await {
        report_join_error(result);
    }
    tracing::info!("All active Sign connections completed");
}

#[cfg(test)]
mod tests {
    use std::future;

    use crate::SignServiceContext;

    use super::*;

    #[tokio::test]
    async fn binds_the_configured_listener() {
        let config = SignServerConfig {
            listen_addr: "127.0.0.1:0".parse().unwrap(),
            advertise_addr: "127.0.0.1:53312".to_owned(),
        };
        let listen_addr = config.listen_addr;
        let service = SignService::new(SignServiceContext::for_test(true).await).unwrap();
        let server = SignServer::bind(config, service).await.unwrap();
        let local_addr = server.local_addr().unwrap();

        assert_eq!(local_addr.ip(), listen_addr.ip());
        assert_ne!(local_addr.port(), listen_addr.port());
        server.run(future::ready(())).await.unwrap();
    }
}
